mod maps;
pub mod decoder;

pub use decoder::decode;

pub trait BitReader {
    fn peek(&self, bits: u8) -> Option<u16>;
    fn consume(&mut self, bits: u8);
}

struct SliceBits<R> {
    read: R,
    partial: u32,
    valid: u8,
}
impl<R: Iterator<Item=u8>> SliceBits<R> {
    pub fn new(read: R) -> Self {
        let mut bits = SliceBits {
            read,
            partial: 0,
            valid: 0
        };
        bits.fill();
        bits
    }
    fn fill(&mut self) {
        while self.valid < 16 {
            if let Some(byte) = self.read.next() {
                self.partial = self.partial << 8 | byte as u32;
                self.valid += 8;
            } else {
                break
            }
        }
    }
    /*
    fn print(&self) {
        println!("partial: {:0w$b}, valid: {}", self.partial, self.valid, w=self.valid as usize);
    }
    */
}
impl<R: Iterator<Item=u8>> BitReader for SliceBits<R> {
    fn peek(&self, bits: u8) -> Option<u16> {
        assert!(bits <= 16);
        if self.valid >= bits {
            let shift = self.valid - bits;
            let out = (self.partial >> shift) as u16;
            Some(out)
        } else {
            None
        }
    }
    fn consume(&mut self, bits: u8) {
        self.valid -= bits;
        self.partial &= (1<<self.valid)-1;
        self.fill();
    }
}

#[test]
fn test_bits() {
    let mut bits = SliceBits::new([0b0000_1101, 0b1010_0000].iter().cloned());
    assert_eq!(maps::black(&mut bits), Some(42));
}
