#![feature(slice_split_once)]

use fax::{VecWriter, decoder, decoder::pels, BitWriter, Bits, Color};
use std::io::Write;
use std::fs::{self, File};
use std::path::Path;

#[test]
fn main() {
    let data_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("stream");

    let mut fails = vec![];

    for r in data_path.read_dir().unwrap() {
        let e = r.unwrap();
        let p = e.path();
        if p.extension().map(|e| e == "pbm").unwrap_or(false) {
            let base = data_path.join(p.file_stem().unwrap());
            let success = test_file(&base, &p);
            println!("{base:?} {success:?}");
            if !success {
                fails.push(p);
            }
        }
    }

    if fails.len() > 0 {
        println!("failures: {fails:?}");
        panic!("");
    }
}

fn test_file(fax_path: &Path, pbm_path: &Path) -> bool {
    let ref_data = std::fs::read(pbm_path).unwrap();
    let (header1, data) = ref_data.split_once(|&b| b == b'\n').unwrap();
    assert_eq!(header1, b"P4");
    let (header2, ref_image) = data.split_once(|&b| b == b'\n').unwrap();
    let header2 = std::str::from_utf8(header2).unwrap();
    let (w, h) = header2.split_once(" ").unwrap();
    let width: u16 = w.parse().unwrap();
    let h: u16 = h.parse().unwrap();

    let mut ref_lines = ref_image.chunks_exact((width as usize + 7) / 8);

    let data = fs::read(fax_path).unwrap();
    let mut height = 0;
    let mut errors = 0;
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
        let data = writer.finish();
        let ref_line = ref_lines.next().unwrap();
        if ref_line != data {
            println!("line {height} mismatch");
            errors += 1;
        }
        height += 1;
    });

    dbg!(height, h, errors);
    height == h && errors == 0
}
