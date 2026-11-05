//! Regression tests for rows wider than 32,767 pixels.
//!
//! The encoder stores transition positions as `u32`, but the vertical-mode
//! delta used to be computed with `a1 as i16 - b1 as i16`, which overflows as
//! soon as the two positions straddle 32,767 — i.e. for lines wider than
//! 32,767 pixels — and panicked in any build with overflow checks.
//! See issue #30.

use fax::{decoder, encoder, Color, VecWriter};

const WIDTH: u32 = 40_000;

fn row(black_from: u32) -> Vec<Color> {
    (0..WIDTH)
        .map(|x| {
            if x >= black_from {
                Color::Black
            } else {
                Color::White
            }
        })
        .collect()
}

fn encode(rows: &[Vec<Color>]) -> Vec<u8> {
    let mut encoder = encoder::Encoder::new(VecWriter::new());
    for r in rows {
        encoder.encode_line(r.iter().copied(), WIDTH).unwrap();
    }
    encoder.finish().unwrap().finish()
}

fn decode(data: &[u8], height: u32) -> Vec<Vec<Color>> {
    let mut lines = vec![];
    let ok = decoder::decode_g4(data.iter().copied(), WIDTH, Some(height), |transitions| {
        lines.push(decoder::pels(transitions, WIDTH).collect::<Vec<_>>());
    });
    assert_eq!(ok, Some(()), "decode failed");
    lines
}

/// The exact case from the issue: `a1 = 32_768`, `b1 = 32_767`.
#[test]
fn vertical_delta_across_i16_boundary() {
    let rows = vec![row(32_767), row(32_768)];
    let data = encode(&rows);
    assert_eq!(decode(&data, rows.len() as u32), rows);
}

/// Vertical mode is chosen when |a1 - b1| <= 3, so exercise both signs of the
/// delta across the boundary: 32_765..=32_770 covers -2..=+3.
#[test]
fn vertical_delta_every_offset_across_i16_boundary() {
    let mut rows = vec![row(32_767)];
    rows.extend((32_764..=32_770).map(row));
    let data = encode(&rows);
    assert_eq!(decode(&data, rows.len() as u32), rows);
}

/// Rows taller and wider than a `u16` can address, with a run in each.
#[test]
fn wide_row_horizontal_and_pass_modes() {
    let mut rows = vec![];
    let mut r = vec![Color::White; WIDTH as usize];
    // A run that starts and ends past 32,767, so horizontal mode has to
    // encode a length larger than 32,767 bits in one line.
    for x in 33_000..36_000 {
        r[x as usize] = Color::Black;
    }
    rows.push(r);
    rows.push(row(32_767));
    let data = encode(&rows);
    assert_eq!(decode(&data, rows.len() as u32), rows);
}
