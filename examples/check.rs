#![feature(slice_split_once)]

use fax::{VecWriter, decoder, decoder::pels, BitWriter, Bits, Color};
use std::io::Write;
use std::fs::{self, File};
use itertools::Itertools;

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
        println!("line {height}");
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
        println!("ref: {:08b}", ref_line.iter().format(" "));
        println!("dec: {:08b}", data.iter().format(" "));
        println!();
        assert_eq!(ref_line, data);
    });
}