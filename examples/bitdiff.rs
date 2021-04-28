use fax::{BitWriter, Bits, ByteReader, BitReader};
use std::fs;

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

fn main() {
    let mut args = std::env::args().skip(1);
    let a = fs::read(&args.next().unwrap()).unwrap();
    let b = fs::read(&args.next().unwrap()).unwrap();

    for (i, (&a, &b)) in a.iter().zip(b.iter()).enumerate() {
        if a != b {
            println!("mismatch at byte {}: {:08b} vs. {:08b}", i, a, b);
            break;
        }
    }
    if a.len() > b.len() {
        println!("a has additional {:?}", &a[b.len()..]);
    }
    if b.len() > a.len() {
        println!("b has additional {:?}", &b[a.len()..]);
    }
}
