use fax::{VecWriter, encoder::Encoder, BitWriter, Bits, Color, ByteReader, BitReader};
use std::io::Write;
use std::fs::{self, File};

fn main() {
    let mut args = std::env::args().skip(1);
    let input: String = args.next().unwrap();
    let output = args.next().unwrap();

    let data = fs::read(&input).unwrap();
    let reference_data = fs::read(&output).unwrap();
    let mut parts = data.splitn(3, |&b| b == b'\n');
    assert_eq!(parts.next().unwrap(), b"P4");
    let mut size = parts.next().unwrap().splitn(2, |&b| b == b' ');
    let width: u16 = std::str::from_utf8(size.next().unwrap()).unwrap().parse().unwrap();

    //let writer = VecWriter::new();
    let writer = Validator { reader: ByteReader::from_slice(&reference_data) };
    let mut encoder = Encoder::new(writer);
    
    for (y, line) in parts.next().unwrap().chunks((width as usize + 7) / 8).enumerate() {
        println!("\nline {}", y);
        let line = ByteReader::new(line.iter().cloned()).into_bits().take(width as usize)
        .map(|b| match b {
            false => Color::Black,
            true => Color::White
        });
        encoder.encode_line(line, width);
    }
    let mut writer = encoder.finish();
    writer.reader.print_remaining();
    

    //let (data, _) = encoder.into_writer().finish();
    //fs::write(&output, &data).unwrap();
}

struct Validator<R: Iterator<Item=u8>> {
    reader: ByteReader<R>
}
impl<R: Iterator<Item=u8>> BitWriter for Validator<R> {
    fn write(&mut self, bits: Bits) {
        let expected = Bits { data: self.reader.peek(bits.len).unwrap(), len: bits.len };
        assert_eq!(expected, bits);
        self.reader.consume(bits.len);
    }
}