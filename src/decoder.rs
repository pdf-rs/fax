use std::convert::Infallible;
use std::io::{self, Bytes, Read};

use crate::maps::{black, mode, white, Mode, EDFB_HALF, EOL};
use crate::{BitReader, ByteReader, Color, Transitions};

fn with_markup<D, R>(decoder: D, reader: &mut R) -> Option<u16>
where
    D: Fn(&mut R) -> Option<u16>,
{
    let mut sum = 0;
    while let Some(n) = decoder(reader) {
        //print!("{} ", n);
        sum += n;
        if n < 64 {
            //debug!("= {}", sum);
            return Some(sum);
        }
    }
    None
}

fn colored(current: Color, reader: &mut impl BitReader) -> Option<u16> {
    //debug!("{:?}", current);
    match current {
        Color::Black => with_markup(black::decode, reader),
        Color::White => with_markup(white::decode, reader),
    }
}

/// Turn a list of color changing position into an iterator of pixel colors
///
/// The width of the line/image has to be given in `width`.
/// The iterator will produce exactly that many items.
pub fn pels(line: &[u16], width: u16) -> impl Iterator<Item = Color> + '_ {
    use std::iter::repeat;
    let mut color = Color::White;
    let mut last = 0;
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

/// Decode a Group 3 encoded image.
///
/// The callback `line_cb` is called for each decoded line.
/// The argument is the list of positions of color change, starting with white.
///
/// To obtain an iterator over the pixel colors, the `pels` function is provided.
pub fn decode_g3(input: impl Iterator<Item = u8>, mut line_cb: impl FnMut(&[u16])) -> Option<()> {
    let reader = input.map(Result::<u8, Infallible>::Ok);
    let mut decoder = Group3Decoder::new(reader).ok()?;

    while let Ok(status) = decoder.advance() {
        // Always emit the decoded line before checking for end-of-document.
        // The last line before the RTC (Return To Control) marker contains
        // valid data that should not be dropped.
        line_cb(decoder.transitions());
        if status == DecodeStatus::End {
            return Some(());
        }
    }
    None
}

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum DecodeStatus {
    Incomplete,
    End,
}

pub struct Group3Decoder<R> {
    reader: ByteReader<R>,
    current: Vec<u16>,
}
impl<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>> Group3Decoder<R> {
    pub fn new(reader: R) -> Result<Self, DecodeError<E>> {
        let mut reader = ByteReader::new(reader).map_err(DecodeError::Reader)?;
        // Skip any fill bits (zeros) then consume the initial EOL marker.
        skip_to_eol(&mut reader).map_err(|_| DecodeError::Invalid)?;

        Ok(Group3Decoder {
            reader,
            current: vec![],
        })
    }
    pub fn advance(&mut self) -> Result<DecodeStatus, DecodeError<E>> {
        self.current.clear();
        let mut a0 = 0;
        let mut color = Color::White;
        loop {
            // Check for EOL before attempting to parse a run-length code.
            // This prevents the prefix tree from destructively consuming
            // EOL bits that it can't match as a valid code.
            if is_eol_ahead(&self.reader) {
                break;
            }
            match colored(color, &mut self.reader) {
                Some(p) => {
                    a0 += p;
                    self.current.push(a0);
                    color = !color;
                }
                None => break,
            }
        }
        // Skip any fill bits and consume the EOL.
        skip_to_eol(&mut self.reader).map_err(|_| DecodeError::Invalid)?;

        // Check for end-of-document: 6 consecutive EOLs (5 more after the one above).
        for _ in 0..5 {
            if is_eol_ahead(&self.reader) {
                skip_to_eol(&mut self.reader).map_err(|_| DecodeError::Invalid)?;
            } else {
                return Ok(DecodeStatus::Incomplete);
            }
        }

        Ok(DecodeStatus::End)
    }
    pub fn transitions(&self) -> &[u16] {
        &self.current
    }
}

/// Check if the next bits form an EOL marker (possibly with fill bits).
///
/// An EOL is `000000000001` (11 zeros + 1). Fill bits add extra leading zeros
/// for byte alignment. No valid run-length code has more than 8 leading zeros,
/// so 9+ leading zeros guarantees we're looking at fill + EOL, not a code.
///
/// We check for EOL both with and without fill bits by peeking up to 16 bits.
fn is_eol_ahead<E, R: Iterator<Item = Result<u8, E>>>(reader: &ByteReader<R>) -> bool {
    // Check without fill bits: exactly 000000000001
    if reader.peek(EOL.len) == Some(EOL.data) {
        return true;
    }
    // Check with 1-4 fill bits: the pattern is (fill zeros)(000000000001)
    // Total bits = 12 + fill, and the value is still just 1 (all leading bits are zero).
    for fill in 1u8..=4 {
        let total = EOL.len + fill;
        if let Some(val) = reader.peek(total) {
            if val == EOL.data {
                return true;
            }
        }
    }
    false
}

/// Skip zero fill bits and consume the EOL marker (000000000001).
/// Returns Err if no valid EOL is found.
fn skip_to_eol<E: std::fmt::Debug, R: Iterator<Item = Result<u8, E>>>(
    reader: &mut ByteReader<R>,
) -> Result<(), DecodeError<E>> {
    // Skip zero fill bits (used for byte alignment in Group3Options bit 2).
    while reader.peek(1) == Some(0) {
        reader.consume(1).map_err(DecodeError::Reader)?;
    }
    // The next bit should be the '1' that terminates the EOL.
    if reader.peek(1) == Some(1) {
        reader.consume(1).map_err(DecodeError::Reader)?;
        Ok(())
    } else {
        Err(DecodeError::Invalid)
    }
}

/// Decode a Group 4 Image
///
/// - `width` is the width of the image.
/// - The callback `line_cb` is called for each decoded line.
///   The argument is the list of positions of color change, starting with white.
///
///   If `height` is specified, at most that many lines will be decoded,
///   otherwise data is decoded until the end-of-block marker (or end of data).
///
/// To obtain an iterator over the pixel colors, the `pels` function is provided.
pub fn decode_g4(
    input: impl Iterator<Item = u8>,
    width: u16,
    height: Option<u16>,
    mut line_cb: impl FnMut(&[u16]),
) -> Option<()> {
    let reader = input.map(Result::<u8, Infallible>::Ok);
    let mut decoder = Group4Decoder::new(reader, width).ok()?;

    for y in 0..height.unwrap_or(u16::MAX) {
        let status = decoder.advance().ok()?;
        if status == DecodeStatus::End {
            return Some(());
        }
        line_cb(decoder.transition());
    }
    Some(())
}

#[derive(Debug)]
pub enum DecodeError<E> {
    Reader(E),
    Invalid,
    Unsupported,
}
impl<E> std::fmt::Display for DecodeError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Decode Error")
    }
}
impl<E: std::error::Error> std::error::Error for DecodeError<E> {}

pub struct Group4Decoder<R> {
    reader: ByteReader<R>,
    reference: Vec<u16>,
    current: Vec<u16>,
    width: u16,
}
impl<E, R: Iterator<Item = Result<u8, E>>> Group4Decoder<R> {
    pub fn new(reader: R, width: u16) -> Result<Self, E> {
        Ok(Group4Decoder {
            reader: ByteReader::new(reader)?,
            reference: Vec::new(),
            current: Vec::new(),
            width,
        })
    }
    // when Complete::Complete is returned, there is no useful data in .transitions() or .line()
    pub fn advance(&mut self) -> Result<DecodeStatus, DecodeError<E>> {
        let mut transitions = Transitions::new(&self.reference);
        let mut a0 = 0;
        let mut color = Color::White;
        let mut start_of_row = true;
        //debug!("\n\nline {}", y);

        loop {
            //reader.print_peek();
            let mode = match mode::decode(&mut self.reader) {
                Some(mode) => mode,
                None => return Err(DecodeError::Invalid),
            };
            //debug!("  {:?}, color={:?}, a0={}", mode, color, a0);

            match mode {
                Mode::Pass => {
                    if start_of_row && color == Color::White {
                        transitions.pos += 1;
                    } else {
                        transitions
                            .next_color(a0, !color, false)
                            .ok_or(DecodeError::Invalid)?;
                    }
                    //debug!("b1={}", b1);
                    if let Some(b2) = transitions.next() {
                        //debug!("b2={}", b2);
                        a0 = b2;
                    }
                }
                Mode::Vertical(delta) => {
                    let b1 = transitions
                        .next_color(a0, !color, start_of_row)
                        .unwrap_or(self.width);
                    let a1 = (b1 as i16 + delta as i16) as u16;
                    if a1 >= self.width {
                        break;
                    }
                    //debug!("transition to {:?} at {}", !color, a1);
                    self.current.push(a1);
                    color = !color;
                    a0 = a1;
                    if delta < 0 {
                        transitions.seek_back(a0);
                    }
                }
                Mode::Horizontal => {
                    let a0a1 = colored(color, &mut self.reader).ok_or(DecodeError::Invalid)?;
                    let a1a2 = colored(!color, &mut self.reader).ok_or(DecodeError::Invalid)?;
                    let a1 = a0 + a0a1;
                    let a2 = a1 + a1a2;
                    //debug!("a0a1={}, a1a2={}, a1={}, a2={}", a0a1, a1a2, a1, a2);

                    self.current.push(a1);
                    if a2 >= self.width {
                        break;
                    }
                    self.current.push(a2);
                    a0 = a2;
                }
                Mode::Extension => {
                    let xxx = self.reader.peek(3).ok_or(DecodeError::Invalid)?;
                    // debug!("extension: {:03b}", xxx);
                    self.reader.consume(3);
                    // debug!("{:?}", current);
                    return Err(DecodeError::Unsupported);
                }
                Mode::EOF => return Ok(DecodeStatus::End),
            }
            start_of_row = false;

            if a0 >= self.width {
                break;
            }
        }
        //debug!("{:?}", current);

        std::mem::swap(&mut self.reference, &mut self.current);
        self.current.clear();

        Ok(DecodeStatus::Incomplete)
    }

    pub fn transition(&self) -> &[u16] {
        &self.reference
    }

    pub fn line(&self) -> Line {
        Line {
            transitions: &self.reference,
            width: self.width,
        }
    }
}

pub struct Line<'a> {
    pub transitions: &'a [u16],
    pub width: u16,
}
impl<'a> Line<'a> {
    pub fn pels(&self) -> impl Iterator<Item = Color> + 'a {
        pels(&self.transitions, self.width)
    }
}
