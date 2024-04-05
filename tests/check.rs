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

        let base = data_path.join(p.file_stem().unwrap());
        let pbm = base.with_extension("pbm");
        let success = if p.extension().is_some_and(|e| e == "fax") {
            read_pbm(&pbm).test_fax(&p)
        } else if p.extension().is_some_and(|e| e == "tiff") {
            read_pbm(&pbm).test_tiff(&p)
        } else {
            continue;
        };
        println!("{base:?} {success:?}");
        if !success {
            fails.push(p);
        }
    }

    if fails.len() > 0 {
        println!("failures: {fails:?}");
        panic!("");
    }
}

struct TestImage {
    width: u16,
    height: u16,
    data: Vec<u8>,
}
fn read_pbm(path: &Path) -> TestImage {
    let ref_data = std::fs::read(path).unwrap();
    let (header1, data) = ref_data.split_once(|&b| b == b'\n').unwrap();
    assert_eq!(header1, b"P4");
    let (header2, ref_image) = data.split_once(|&b| b == b'\n').unwrap();
    let header2 = std::str::from_utf8(header2).unwrap();
    let (w, h) = header2.split_once(" ").unwrap();
    let width: u16 = w.parse().unwrap();
    let h: u16 = h.parse().unwrap();

    TestImage { width, height: h, data: ref_image.to_vec() }
}
impl TestImage {
    fn test_fax(&self, fax_path: &Path) -> bool {
        let data = fs::read(fax_path).unwrap();
        self.test_stream(&data, false)
    }

    fn test_tiff(&self, path: &Path) -> bool {
        use tiff::{decoder::Decoder, tags::Tag};
        let data = std::fs::read(path).unwrap();
        let reader = std::io::Cursor::new(data.as_slice());
        let mut decoder = Decoder::new(reader).unwrap();
        let (w, h) = decoder.chunk_dimensions();
        let mut buf = vec![0; w as usize * h as usize];
        let strip_offset = decoder.get_tag(Tag::StripOffsets).unwrap().into_u32().unwrap() as usize;
        let strip_bytes = decoder.get_tag(Tag::StripByteCounts).unwrap().into_u32().unwrap() as usize;
        decoder.goto_offset_u64(strip_offset as _).unwrap();

        let white_is_1 = decoder.get_tag(Tag::PhotometricInterpretation).unwrap().into_u16().unwrap() != 0;

        let data = &data[strip_offset .. strip_offset + strip_bytes];
        self.test_stream(&data, white_is_1)
    }

    fn test_stream(&self, data: &[u8], white_is_1: bool) -> bool {
        let mut ref_lines = self.data.chunks_exact((self.width as usize + 7) / 8);

        let (black, white) = match white_is_1 {
            false => (Bits { data: 1, len: 1 }, Bits { data: 0, len: 1 }),
            true => (Bits { data: 0, len: 1 }, Bits { data: 1, len: 1 })
        };

        let mut height = 0;
        let mut errors = 0;
        let ok = decoder::decode_g4(data.iter().cloned(), self.width, None,  |transitions| {
            //println!("{}", transitions.len());
            let mut writer = VecWriter::new();
            for c in pels(transitions, self.width) {
                let bit = match c {
                    Color::Black => black,
                    Color::White => white
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
        }).is_some();

        ok && errors == 0
    }
}
