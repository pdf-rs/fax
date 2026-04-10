//! Tests against external CCITT test images from Pillow (MIT-CMU) and libtiff (BSD).
//! These are small TIFF files that exercise features not covered by inline test data.

use fax::unified::{decode, pels32, DecodeOptions, EncodingMode};
use fax::{BitWriter, Bits, Color, VecWriter};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;
use tiff::decoder::Decoder as TiffDecoder;
use tiff::tags::Tag;

/// Extract G4/G3 raw strip data and metadata from a single-strip TIFF.
fn read_ccitt_tiff(path: &Path) -> Option<(Vec<u8>, u32, u32, u16, bool)> {
    let data = std::fs::read(path).ok()?;
    let reader = std::io::Cursor::new(data.as_slice());
    let mut dec = TiffDecoder::new(reader).ok()?;
    let width = dec.get_tag(Tag::ImageWidth).ok()?.into_u32().ok()?;
    let height = dec.get_tag(Tag::ImageLength).ok()?.into_u32().ok()?;
    let compression = dec.get_tag(Tag::Compression).ok()?.into_u16().ok()?;
    let strip_offset = dec.get_tag(Tag::StripOffsets).ok()?.into_u32().ok()? as usize;
    let strip_bytes = dec.get_tag(Tag::StripByteCounts).ok()?.into_u32().ok()? as usize;
    let fill_order_lsb = dec
        .get_tag(Tag::FillOrder)
        .ok()
        .and_then(|v| v.into_u16().ok())
        .unwrap_or(1)
        == 2;
    let strip = data[strip_offset..strip_offset + strip_bytes].to_vec();
    Some((strip, width, height, compression, fill_order_lsb))
}

fn try_decode_tiff(path: &Path) -> Result<(u32, Vec<Vec<u32>>), String> {
    let (strip, width, height, compression, fill_order_lsb) =
        read_ccitt_tiff(path).ok_or_else(|| format!("failed to read TIFF: {}", path.display()))?;

    let encoding = match compression {
        4 => EncodingMode::Group4,
        3 | 2 => EncodingMode::Group3_1D,
        _ => return Err(format!("unsupported compression: {compression}")),
    };

    let mut opts = DecodeOptions::new(encoding, width, Some(height));
    opts.msb_first = !fill_order_lsb;
    opts.end_of_block = false;

    let mut lines = Vec::new();
    decode(strip.iter().copied(), opts, |t| lines.push(t.to_vec()))
        .map_err(|e| format!("decode error: {e} (got {} of {height} lines)", lines.len()))?;
    Ok((height, lines))
}

/// G4 with FillOrder=2 (LSB-first bit order).
#[test]
fn pillow_g4_fillorder_lsb() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/pillow/g4-fillorder-test.tif");
    let (height, lines) = try_decode_tiff(&path).unwrap();
    assert_eq!(lines.len() as u32, height);
    assert!(lines.iter().any(|l| !l.is_empty()), "all-white — likely decode failure");
}

/// Standard G4, 128x128 reference image.
#[test]
fn pillow_hopper_g4() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/pillow/hopper_g4.tif");
    let (height, lines) = try_decode_tiff(&path).unwrap();
    assert_eq!(lines.len() as u32, height);
    assert!(lines.iter().any(|l| !l.is_empty()));
}

/// Crash regression: must not panic on malformed G4 data.
#[test]
fn pillow_crash_g4_0da0() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/pillow/crash-g4-0da0.tif");
    let _ = try_decode_tiff(&path);
}

/// Crash regression: must not panic on malformed G4 data.
#[test]
fn pillow_crash_g4_74d2() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/pillow/crash-g4-74d2.tif");
    let _ = try_decode_tiff(&path);
}

/// G3 1D WITHOUT EOL markers (CCITT Modified Huffman, compression=2).
/// Lines terminate when run-lengths sum to width, not by EOL markers.
#[test]
fn pillow_g3_no_eol() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/pillow/g3-noeol-176x8.tif");
    let (strip, width, height, compression, fill_order_lsb) =
        read_ccitt_tiff(&path).unwrap();
    assert_eq!(compression, 2, "expected CCITT RLE (compression=2)");

    let mut opts = DecodeOptions::new(EncodingMode::Group3_1D, width, Some(height));
    opts.msb_first = !fill_order_lsb;
    opts.end_of_line = false; // no EOL markers
    opts.end_of_block = false; // no RTC

    let mut lines = Vec::new();
    decode(strip.iter().copied(), opts, |t| lines.push(t.to_vec())).unwrap();
    assert_eq!(lines.len() as u32, height, "line count mismatch");
    assert!(lines.iter().any(|l| !l.is_empty()), "all-white — likely decode failure");
}

/// Minimal G3 image (32x2). From libtiff test suite (BSD).
#[test]
fn libtiff_g3_minimal() {
    // testfax3_bug_513.tiff — 198 bytes, inlined to avoid file dependency
    #[rustfmt::skip]
    const TIFF: &[u8] = &[
        0x49, 0x49, 0x2a, 0x00, 0x30, 0x00, 0x00, 0x00, 0x00, 0x13, 0x54, 0x3a,
        0x1d, 0x0e, 0x87, 0x43, 0xa1, 0xd0, 0xe8, 0x74, 0x3a, 0x1d, 0x0e, 0x87,
        0x43, 0xa1, 0xd0, 0xe8, 0x70, 0x01, 0x1d, 0x0e, 0x87, 0x43, 0xa1, 0xd0,
        0xe8, 0x74, 0x3a, 0x1d, 0x0e, 0x87, 0x43, 0xa1, 0xd0, 0xe8, 0x74, 0x3a,
        0x0c, 0x00, 0x00, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x20, 0x00,
        0x00, 0x00, 0x01, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00,
        0x00, 0x00, 0x02, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x03, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00,
        0x00, 0x00, 0x06, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x11, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x00,
        0x00, 0x00, 0x15, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x16, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00,
        0x00, 0x00, 0x17, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x28, 0x00,
        0x00, 0x00, 0x1a, 0x01, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0xbe, 0x00,
        0x00, 0x00, 0x1b, 0x01, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0xc6, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    ];
    let reader = std::io::Cursor::new(TIFF);
    let mut dec = TiffDecoder::new(reader).unwrap();
    let width = dec.get_tag(Tag::ImageWidth).unwrap().into_u32().unwrap();
    let height = dec.get_tag(Tag::ImageLength).unwrap().into_u32().unwrap();
    let off = dec.get_tag(Tag::StripOffsets).unwrap().into_u32().unwrap() as usize;
    let len = dec.get_tag(Tag::StripByteCounts).unwrap().into_u32().unwrap() as usize;
    let strip = &TIFF[off..off + len];

    let opts = DecodeOptions::new(EncodingMode::Group3_1D, width, Some(height));
    let mut lines = Vec::new();
    decode(strip.iter().copied(), opts, |t| lines.push(t.to_vec())).unwrap();
    assert_eq!(lines.len() as u32, height);
}

// ---- Unified decoder parity with legacy on all committed test data ----

// ---- Hash-based verification against libtiff ground truth ----

struct RefEntry {
    width: u32,
    height: u32,
    sha256: String,
}

fn load_reference_hashes() -> HashMap<String, RefEntry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/reference-hashes.tsv");
    let content = std::fs::read_to_string(&path).expect("reference-hashes.tsv not found");
    let mut map = HashMap::new();
    for line in content.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert!(f.len() == 5, "bad line: {line}");
        map.insert(
            format!("{}:{}", f[1], f[0]),
            RefEntry {
                width: f[2].parse().unwrap(),
                height: f[3].parse().unwrap(),
                sha256: f[4].to_string(),
            },
        );
    }
    map
}

fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for byte in hash {
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

/// Render u32 transitions to PBM bytes for hashing.
fn render_pbm(transitions: &[u32], width: u32, pbm: &mut Vec<u8>) {
    let mut writer = VecWriter::new();
    for c in pels32(transitions, width) {
        let bit = match c {
            Color::Black => Bits { data: 1, len: 1 },
            Color::White => Bits { data: 0, len: 1 },
        };
        writer.write(bit).unwrap();
    }
    writer.pad();
    pbm.extend(writer.finish());
}

fn parse_raw_filename(name: &str) -> Option<(&str, u16)> {
    let name = name.strip_suffix(".raw")?;
    let (id, rest) = name.split_once('_')?;
    let width_str = rest.strip_prefix("0-w")?;
    let width = width_str.parse().ok()?;
    Some((id, width))
}

/// Decode all 36 errors/*.raw through the unified decoder and verify
/// SHA-256 of decoded PBM output matches libtiff reference hashes.
#[test]
fn unified_errors_match_libtiff_hashes() {
    let refs = load_reference_hashes();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/errors");
    let mut tested = 0;
    let mut failures = vec![];

    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let (id, _width) = match parse_raw_filename(&name) {
            Some(v) => v,
            None => continue,
        };
        let reference = match refs.get(&format!("errors:{id}")) {
            Some(r) => r,
            None => { failures.push(format!("{id}: no hash in TSV")); tested += 1; continue; }
        };
        let data = std::fs::read(&path).unwrap();
        let opts = DecodeOptions::new(EncodingMode::Group4, reference.width, Some(reference.height));
        let mut pbm = Vec::new();
        write!(pbm, "P4\n{} {}\n", reference.width, reference.height).unwrap();
        let _ = decode(data.iter().copied(), opts, |t| render_pbm(t, reference.width, &mut pbm));
        let hash = sha256_hex(&pbm);
        if hash != reference.sha256 {
            failures.push(format!("{id}: hash mismatch"));
        }
        tested += 1;
    }
    assert!(tested > 0);
    assert!(failures.is_empty(), "{} of {tested} failed:\n  {}", failures.len(), failures.join("\n  "));
}

/// Decode all files/*.fax and files/*.tiff through the unified decoder
/// and verify SHA-256 matches libtiff reference hashes.
#[test]
fn unified_files_match_libtiff_hashes() {
    let refs = load_reference_hashes();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-files/files");
    let mut tested = 0;
    let mut failures = vec![];

    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let p = entry.path();
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();

        let stream = if p.extension().is_some_and(|e| e == "fax") {
            std::fs::read(&p).unwrap()
        } else if p.extension().is_some_and(|e| e == "tiff") {
            let data = std::fs::read(&p).unwrap();
            let reader = std::io::Cursor::new(data.as_slice());
            let mut dec = TiffDecoder::new(reader).unwrap();
            let off = dec.get_tag(Tag::StripOffsets).unwrap().into_u32().unwrap() as usize;
            let len = dec.get_tag(Tag::StripByteCounts).unwrap().into_u32().unwrap() as usize;
            data[off..off + len].to_vec()
        } else {
            continue;
        };

        let reference = match refs.get(&format!("files:{stem}")) {
            Some(r) => r,
            None => { failures.push(format!("{stem}: no hash in TSV")); tested += 1; continue; }
        };

        let opts = DecodeOptions::new(EncodingMode::Group4, reference.width, Some(reference.height));
        let mut pbm = Vec::new();
        write!(pbm, "P4\n{} {}\n", reference.width, reference.height).unwrap();
        let _ = decode(stream.iter().copied(), opts, |t| render_pbm(t, reference.width, &mut pbm));
        let hash = sha256_hex(&pbm);
        if hash != reference.sha256 {
            failures.push(format!("{stem}: hash mismatch"));
        }
        tested += 1;
    }
    assert!(tested > 0);
    assert!(failures.is_empty(), "{} of {tested} failed:\n  {}", failures.len(), failures.join("\n  "));
}
