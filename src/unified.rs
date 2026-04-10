//! Unified CCITT Group 3 / Group 4 decoder.
//!
//! Provides a single [`Decoder`] struct that handles all encoding modes,
//! configurable via [`DecodeOptions`]. Maps directly to PDF `CCITTFaxDecode`
//! parameters and TIFF `Group3Options` / `Group4Options` tags.

use std::convert::Infallible;

use crate::decoder::{
    colored, is_eol_ahead, skip_to_eol, DecodeError, DecodeStatus,
};
use crate::maps::{mode, Mode};
use crate::{BitReader, ByteReader, Color};

/// Error type for the unified decoder.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error<E> {
    /// Error from the underlying reader.
    Reader(E),
    /// Invalid or malformed data.
    Invalid,
    /// Unsupported feature (e.g., extension mode).
    Unsupported,
    /// A resource limit was exceeded.
    LimitExceeded,
}

impl<E> From<DecodeError<E>> for Error<E> {
    fn from(e: DecodeError<E>) -> Self {
        match e {
            DecodeError::Reader(e) => Error::Reader(e),
            DecodeError::Invalid => Error::Invalid,
            DecodeError::Unsupported => Error::Unsupported,
        }
    }
}

impl<E> std::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Reader(_) => write!(f, "reader error"),
            Error::Invalid => write!(f, "invalid data"),
            Error::Unsupported => write!(f, "unsupported feature"),
            Error::LimitExceeded => write!(f, "resource limit exceeded"),
        }
    }
}

impl<E: std::error::Error> std::error::Error for Error<E> {}

/// Coding mode for CCITT fax decoding.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodingMode {
    /// Group 4 two-dimensional coding (T.6 / MMR).
    Group4,
    /// Group 3 one-dimensional coding (T.4 / MH, K=0).
    Group3_1D,
    /// Group 3 mixed 1D/2D coding (T.4 / MR, K>0).
    /// The `k` parameter is the maximum number of consecutive 2D-coded
    /// lines before a mandatory 1D line.
    Group3_2D { k: u32 },
}

/// Resource limits for untrusted input.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Maximum total decoded pixels (width × height). None = unlimited.
    pub max_pixels: Option<u64>,
    /// Maximum input bytes to consume. None = unlimited.
    pub max_input_bytes: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            // Default to u16 range. Raise explicitly for wider images.
            max_pixels: Some(u16::MAX as u64 * u16::MAX as u64),
            max_input_bytes: None,
        }
    }
}

/// Options for the unified CCITT decoder.
///
/// Maps directly to the PDF `CCITTFaxDecode` filter parameters and
/// TIFF Group3Options / Group4Options tags.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DecodeOptions {
    /// Image width in pixels.
    pub columns: u32,
    /// Image height in rows. `None` = decode until end-of-block marker
    /// or end of data.
    pub rows: Option<u32>,
    /// Coding mode (Group 4, Group 3 1D, Group 3 2D).
    pub encoding: EncodingMode,
    /// Each row's encoded data is padded to a byte boundary.
    /// TIFF: Group3Options bit 2. PDF: `EncodedByteAlign`.
    pub rows_are_byte_aligned: bool,
    /// EOL markers are present between rows (Group 3).
    /// PDF: `EndOfLine`.
    pub end_of_line: bool,
    /// Data ends with EOFB (Group 4) or RTC (Group 3).
    /// When false, uses `rows` to determine when to stop.
    /// PDF: `EndOfBlock`.
    pub end_of_block: bool,
    /// Invert black/white polarity.
    /// TIFF: PhotometricInterpretation. PDF: `BlackIs1`.
    pub black_is_1: bool,
    /// Bit order within bytes. `true` = MSB first (standard).
    /// TIFF: FillOrder tag.
    pub msb_first: bool,
    /// Resource limits for untrusted input.
    pub limits: Option<Limits>,
}

impl DecodeOptions {
    /// Create options for the given encoding mode and dimensions.
    /// All other options use sensible defaults (MSB-first, EndOfBlock=true, etc.).
    pub fn new(encoding: EncodingMode, columns: u32, rows: Option<u32>) -> Self {
        let end_of_line = matches!(
            encoding,
            EncodingMode::Group3_1D | EncodingMode::Group3_2D { .. }
        );
        DecodeOptions {
            columns,
            rows,
            encoding,
            end_of_line,
            ..DecodeOptions::default()
        }
    }
}

impl Default for DecodeOptions {
    fn default() -> Self {
        DecodeOptions {
            columns: 1728,
            rows: None,
            encoding: EncodingMode::Group4,
            rows_are_byte_aligned: false,
            end_of_line: false,
            end_of_block: true,
            black_is_1: false,
            msb_first: true,
            limits: Some(Limits::default()),
        }
    }
}

/// Unified CCITT Group 3 / Group 4 decoder.
///
/// Handles all encoding modes (G4, G3 1D, G3 2D) through a single
/// struct, configurable via [`DecodeOptions`].
pub struct Decoder<R> {
    reader: ByteReader<R>,
    reference: Vec<u32>,
    current: Vec<u32>,
    options: DecodeOptions,
    lines_decoded: u32,
    pixels_emitted: u64,
    g3_2d_consecutive: u32,
}

impl<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>> Decoder<R> {
    /// Create a new decoder with the given options.
    pub fn new(reader: R, options: DecodeOptions) -> Result<Self, Error<E>> {
        // Pre-check pixel limits
        if let Some(ref limits) = options.limits {
            if let (Some(max_pixels), Some(rows)) = (limits.max_pixels, options.rows) {
                if (options.columns as u64) * (rows as u64) > max_pixels {
                    return Err(Error::LimitExceeded);
                }
            }
        }

        let mut reader = if options.msb_first {
            ByteReader::new(reader).map_err(Error::Reader)?
        } else {
            ByteReader::new_lsb(reader).map_err(Error::Reader)?
        };

        // G3 modes: consume the initial EOL marker
        match options.encoding {
            EncodingMode::Group3_1D | EncodingMode::Group3_2D { .. } => {
                if options.end_of_line {
                    skip_to_eol(&mut reader).map_err(|_: DecodeError<E>| Error::Invalid)?;
                }
            }
            EncodingMode::Group4 => {}
        }

        Ok(Decoder {
            reader,
            reference: Vec::new(),
            current: Vec::new(),
            options,
            lines_decoded: 0,
            pixels_emitted: 0,
            g3_2d_consecutive: 0,
        })
    }

    /// Decode the next line. Returns `DecodeStatus::End` when there are
    /// no more lines (EOFB/RTC found, or row limit reached).
    ///
    /// After a successful `Incomplete` return, call [`transitions()`] to
    /// get the decoded line data.
    pub fn advance(&mut self) -> Result<DecodeStatus, Error<E>> {
        // Check row limit
        if let Some(rows) = self.options.rows {
            if self.lines_decoded >= rows {
                return Ok(DecodeStatus::End);
            }
        }

        self.current.clear();
        let mut is_eof = false;
        let width = self.options.columns;

        match self.options.encoding {
            EncodingMode::Group4 => {
                if decode_2d_line(
                    &mut self.reader,
                    &self.reference,
                    &mut self.current,
                    width,
                )? {
                    return Ok(DecodeStatus::End);
                }
            }
            EncodingMode::Group3_1D => {
                decode_1d_line(
                    &mut self.reader,
                    &mut self.current,
                    self.options.end_of_line,
                    width,
                )?;
                // Skip trailing EOL/RTC unless this is the last row
                // (some encoders omit the EOL after the final line).
                let is_last_row = self
                    .options
                    .rows
                    .is_some_and(|r| self.lines_decoded + 1 >= r);
                if !is_last_row || self.options.end_of_block {
                    is_eof = self.handle_g3_line_end()?;
                }
            }
            EncodingMode::Group3_2D { k } => {
                let tag = self.reader.peek(1).ok_or(Error::Invalid)?;
                self.reader.consume(1).map_err(Error::Reader)?;

                if tag == 1 {
                    decode_1d_line(
                        &mut self.reader,
                        &mut self.current,
                        self.options.end_of_line,
                        width,
                    )?;
                    self.g3_2d_consecutive = 0;
                } else {
                    if decode_2d_line(
                        &mut self.reader,
                        &self.reference,
                        &mut self.current,
                        width,
                    )? {
                        return Ok(DecodeStatus::End);
                    }
                    self.g3_2d_consecutive += 1;
                    if k > 0 && self.g3_2d_consecutive >= k {
                        self.g3_2d_consecutive = 0;
                    }
                }
                // Skip trailing EOL/RTC unless this is the last row
                let is_last_row = self
                    .options
                    .rows
                    .is_some_and(|r| self.lines_decoded + 1 >= r);
                if !is_last_row || self.options.end_of_block {
                    is_eof = self.handle_g3_line_end()?;
                }
            }
        }

        if self.options.rows_are_byte_aligned {
            self.reader.align_to_byte().map_err(Error::Reader)?;
        }

        // Always commit the decoded line before signaling end.
        // The last line before RTC contains valid data.
        std::mem::swap(&mut self.reference, &mut self.current);
        self.current.clear();
        self.lines_decoded += 1;
        self.pixels_emitted += self.options.columns as u64;

        if is_eof {
            return Ok(DecodeStatus::End);
        }

        if let Some(ref limits) = self.options.limits {
            if let Some(max_pixels) = limits.max_pixels {
                if self.pixels_emitted > max_pixels {
                    return Err(Error::LimitExceeded);
                }
            }
            if let Some(max_bytes) = limits.max_input_bytes {
                if self.reader.bytes_consumed() > max_bytes {
                    return Err(Error::LimitExceeded);
                }
            }
        }

        Ok(DecodeStatus::Incomplete)
    }

    /// Handle G3 line ending: consume EOL (and tag bit for 2D), check for RTC.
    /// Returns true if end-of-document (RTC) was detected.
    fn handle_g3_line_end(&mut self) -> Result<bool, Error<E>> {
        let is_2d = matches!(self.options.encoding, EncodingMode::Group3_2D { .. });

        if self.options.end_of_line {
            skip_to_eol(&mut self.reader).map_err(|_: DecodeError<E>| Error::Invalid)?;
        }

        // Check for RTC (6 consecutive EOLs = 5 more after the one above).
        // In G3 2D mode, each EOL in the RTC is followed by a tag bit of 1.
        if self.options.end_of_block && self.options.end_of_line {
            for _ in 0..5 {
                if is_2d {
                    // In 2D RTC, each EOL is followed by tag=1.
                    // Peek 10 bits: if bit 9 (tag) is 1 and bits 8..0 are
                    // all zeros, this is a tag-1 followed by the start of
                    // an EOL — i.e., RTC continuation.
                    match self.reader.peek(1) {
                        Some(1) => {
                            if let Some(val) = self.reader.peek(10) {
                                if val & 0x1FF == 0 {
                                    // tag=1 + 9 zeros → RTC EOL ahead
                                    self.reader.consume(1).map_err(Error::Reader)?;
                                } else {
                                    return Ok(false); // normal 1D line follows
                                }
                            } else {
                                return Ok(false);
                            }
                        }
                        _ => return Ok(false), // tag=0 or no data → normal 2D line
                    }
                }
                if is_eol_ahead(&self.reader) {
                    skip_to_eol(&mut self.reader)
                        .map_err(|_: DecodeError<E>| Error::Invalid)?;
                } else {
                    return Ok(false);
                }
            }
            return Ok(true);
        }

        Ok(false)
    }

    /// Get the decoded transitions for the current line.
    pub fn transitions(&self) -> &[u32] {
        &self.reference
    }

    /// Whether black/white polarity is inverted.
    pub fn black_is_1(&self) -> bool {
        self.options.black_is_1
    }

    /// Number of lines decoded so far.
    pub fn lines_decoded(&self) -> u32 {
        self.lines_decoded
    }
}

/// Decode a CCITT-encoded image using the unified decoder.
///
/// The callback `line_cb` is called for each decoded line with the
/// list of color-change positions (transitions) as `&[u32]`.
///
/// When `rows` is known and EOFB/RTC arrives early, remaining lines
/// are emitted as all-white (empty transitions).
pub fn decode(
    input: impl Iterator<Item = u8>,
    options: DecodeOptions,
    mut line_cb: impl FnMut(&[u32]),
) -> Result<(), Error<Infallible>> {
    let rows = options.rows;
    let is_g3 = matches!(
        options.encoding,
        EncodingMode::Group3_1D | EncodingMode::Group3_2D { .. }
    );
    let reader = input.map(Result::<u8, Infallible>::Ok);
    let mut decoder = Decoder::new(reader, options)?;
    let mut lines_emitted = 0u32;

    loop {
        match decoder.advance()? {
            DecodeStatus::Incomplete => {
                line_cb(decoder.transitions());
                lines_emitted += 1;
            }
            DecodeStatus::End => {
                // Two paths reach End:
                // 1. Row limit (top of advance) — no line decoded, lines_decoded == lines_emitted
                // 2. G3 RTC — last line decoded and committed, lines_decoded > lines_emitted
                // Emit the committed line in case 2.
                if is_g3 && decoder.lines_decoded() > lines_emitted {
                    line_cb(decoder.transitions());
                }
                break;
            }
        }
    }

    // Pad remaining lines with white if rows is known
    if let Some(rows) = rows {
        for _ in decoder.lines_decoded()..rows {
            line_cb(&[]);
        }
    }

    Ok(())
}

/// Turn a list of u32 color-change positions into pixel colors.
///
/// Like [`crate::decoder::pels`] but for u32 transitions from the
/// unified decoder.
pub fn pels32(line: &[u32], width: u32) -> impl Iterator<Item = Color> + '_ {
    use std::iter::repeat;
    let mut color = Color::White;
    let mut last = 0u32;
    let pad_color = if line.len() & 1 == 1 { !color } else { color };
    line.iter()
        .flat_map(move |&p| {
            let c = color;
            color = !color;
            let n = p.saturating_sub(last);
            last = p;
            repeat(c).take(n as usize)
        })
        .chain(repeat(pad_color))
        .take(width as usize)
}

// ---- Private helpers: native u32 line decoders ----

/// Reference line tracker for 2D decoding (u32 version).
struct Transitions32<'a> {
    edges: &'a [u32],
    pos: usize,
}

impl<'a> Transitions32<'a> {
    fn new(edges: &'a [u32]) -> Self {
        Transitions32 { edges, pos: 0 }
    }
    fn seek_back(&mut self, start: u32) {
        self.pos = self.pos.min(self.edges.len().saturating_sub(1));
        while self.pos > 0 {
            if start < self.edges[self.pos - 1] {
                self.pos -= 1;
            } else {
                break;
            }
        }
    }
    fn next_color(&mut self, start: u32, color: Color, start_of_row: bool) -> Option<u32> {
        if start_of_row {
            if color == Color::Black {
                self.pos = 1;
                return self.edges.first().copied();
            } else {
                self.pos = 2;
                return self.edges.get(1).copied();
            }
        }
        while self.pos < self.edges.len() {
            if self.edges[self.pos] <= start {
                self.pos += 1;
                continue;
            }
            if (self.pos % 2 == 0) != (color == Color::Black) {
                self.pos += 1;
            }
            break;
        }
        if self.pos < self.edges.len() {
            let val = self.edges[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }
    fn next(&mut self) -> Option<u32> {
        if self.pos < self.edges.len() {
            let val = self.edges[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }
}

/// Decode a single 1D (run-length) coded line into u32 transitions.
///
/// When `has_eol` is true, stops at the next EOL marker.
/// When false, stops when cumulative run-lengths reach `width`.
fn decode_1d_line<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>>(
    reader: &mut ByteReader<R>,
    current: &mut Vec<u32>,
    has_eol: bool,
    width: u32,
) -> Result<(), Error<E>> {
    let mut a0: u32 = 0;
    let mut color = Color::White;
    loop {
        if has_eol && is_eol_ahead(reader) {
            break;
        }
        match colored(color, reader) {
            Some(p) => {
                a0 = a0.checked_add(p as u32).ok_or(Error::Invalid)?;
                current.push(a0);
                color = !color;
                if !has_eol && a0 >= width {
                    break;
                }
            }
            None => break,
        }
    }
    Ok(())
}

/// Decode a single 2D (mode-coded) line into u32 transitions.
/// Returns true if EOFB/EOF marker was encountered.
fn decode_2d_line<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>>(
    reader: &mut ByteReader<R>,
    reference: &[u32],
    current: &mut Vec<u32>,
    width: u32,
) -> Result<bool, Error<E>> {
    let mut transitions = Transitions32::new(reference);
    let mut a0: u32 = 0;
    let mut color = Color::White;
    let mut start_of_row = true;

    loop {
        let m = match mode::decode(reader) {
            Some(m) => m,
            None => return Err(Error::Invalid),
        };

        match m {
            Mode::Pass => {
                if start_of_row && color == Color::White {
                    transitions.pos += 1;
                } else {
                    transitions
                        .next_color(a0, !color, false)
                        .ok_or(Error::Invalid)?;
                }
                if let Some(b2) = transitions.next() {
                    a0 = b2;
                }
            }
            Mode::Vertical(delta) => {
                let b1 = transitions
                    .next_color(a0, !color, start_of_row)
                    .unwrap_or(width);
                let a1_i64 = b1 as i64 + delta as i64;
                if a1_i64 < 0 || a1_i64 > width as i64 {
                    break;
                }
                let a1 = a1_i64 as u32;
                if a1 < width {
                    current.push(a1);
                }
                color = !color;
                a0 = a1;
                if delta < 0 {
                    transitions.seek_back(a0);
                }
            }
            Mode::Horizontal => {
                let a0a1 = colored(color, reader).ok_or(Error::Invalid)? as u32;
                let a1a2 = colored(!color, reader).ok_or(Error::Invalid)? as u32;
                let a1 = a0.checked_add(a0a1).ok_or(Error::Invalid)?;
                let a2 = a1.checked_add(a1a2).ok_or(Error::Invalid)?;
                if a1 < width {
                    current.push(a1);
                }
                if a2 >= width {
                    break;
                }
                current.push(a2);
                a0 = a2;
            }
            Mode::Extension => {
                let _ext = reader.peek(3).ok_or(Error::Invalid)?;
                let _ = reader.consume(3);
                return Err(Error::Unsupported);
            }
            Mode::EOF => return Ok(true),
        }
        start_of_row = false;

        if a0 >= width {
            break;
        }
    }
    Ok(false)
}
