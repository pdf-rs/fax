#![feature(slice_split_once)]

use fax::{VecWriter, decoder, decoder::pels, BitWriter, Bits, Color};
use std::io::Write;
use std::fs::{self, File};

fn main() {
    let mut args = std::env::args().skip(1);
    let input: String = args.next().unwrap();
    let reference = args.next().unwrap();

    let ref_data = std::fs::read(&reference).unwrap();
    let (header1, data) = ref_data.split_once(|&b| b == b'\n').unwrap();
    assert_eq!(header1, b"P4");
    let (header2, ref_image) = data.split_once(|&b| b == b'\n').unwrap();
    let header2 = std::str::from_utf8(header2).unwrap();
    dbg!(header2);
    let (w, h) = header2.split_once(" ").unwrap();
    let width: u16 = w.parse().unwrap();
    let h: u16 = h.parse().unwrap();

    let mut ref_lines = ref_image.chunks_exact((width as usize + 7) / 8);

    let data = fs::read(&input).unwrap();
    let mut height = 0;
    decoder::decode_g4(data.iter().cloned(), width, None,  |transitions| {
        let mut writer = VecWriter::new();
        for c in pels(transitions, width) {
            let bit = match c {
                Color::Black => Bits { data: 1, len: 1 },
                Color::White => Bits { data: 0, len: 1 }
            };
            writer.write(bit);
        }
        writer.pad();
        height += 1;
        let data = writer.finish();
        let ref_line = ref_lines.next().unwrap();
        println!("{height:3} dec: {}", Line(&data));
        if ref_line != data {
            println!("    ref: {}", Line(ref_line));
            panic!("decode error");
        }
    });
}

struct Line<'a>(&'a [u8]);
impl<'a> std::fmt::Display for Line<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {       
        let codes = [' ', '▐', '▌', '█'];
        for b in self.0.iter().flat_map(|b| [b >> 6, (b >> 4) & 3, (b >>2) & 3, b & 3]) {
            write!(f, "{}", codes[b as usize])?;
        }
        Ok(())
    }
}
