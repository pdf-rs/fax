use std::ops::Not;
use std::fmt;

mod maps;

/// Decoder module
pub mod decoder;

/// Encoder module
pub mod encoder;

/// TIFF helper functions
pub mod tiff;

/// Trait used to read data bitwise.
/// 
/// For lazy people `ByteReader` is provided which implements this trait.
pub trait BitReader {
    /// look at the next (up to 16) bits of data
    /// 
    /// Data is returned in the lower bits of the `u16`.
    fn peek(&self, bits: u8) -> Option<u16>;

    /// Consume the given amount of bits from the input.
    fn consume(&mut self, bits: u8);

    /// Assert that the next bits matches the given pattern.
    /// 
    /// If it does not match, the found pattern is returned if enough bits are aviable.
    /// Otherwise None is returned.
    fn expect(&mut self, bits: Bits) -> Result<(), Option<Bits>> {
        match self.peek(bits.len) {
            None => Err(None),
            Some(val) if val == bits.data => Ok(()),
            Some(val) => Err(Some(Bits { data: val, len: bits.len }))
        }
    }

    fn bits_to_byte_boundary(&self) -> u8;
}

/// Trait to write data bitwise
/// 
/// The `VecWriter` struct is provided for convinience.
pub trait BitWriter {
    fn write(&mut self, bits: Bits);
}
pub struct VecWriter {
    data: Vec<u8>,
    partial: u32,
    len: u8
}
impl BitWriter for VecWriter {
    fn write(&mut self, bits: Bits) {
        self.partial |= (bits.data as u32) << (32 - self.len - bits.len);
        self.len += bits.len;
        while self.len >= 8 {
            self.data.push((self.partial >> 24) as u8);
            self.partial <<= 8;
            self.len -= 8;
        }
    }
}
impl VecWriter {
    pub fn new() -> Self {
        VecWriter {
            data: Vec::new(),
            partial: 0,
            len: 0
        }
    }
    // with capacity of `n` bits.
    pub fn with_capacity(n: usize) -> Self {
        VecWriter {
            data: Vec::with_capacity((n + 7) / 8),
            partial: 0,
            len: 0
        }
    }

    /// Pad the output with `0` bits until it is at a byte boundary.
    pub fn pad(&mut self) {
        if self.len > 0 {
            self.data.push((self.partial >> 24) as u8);
            self.partial = 0;
            self.len = 0;
        }
    }

    /// pad and return the accumulated bytes
    pub fn finish(mut self) -> Vec<u8> {
        self.pad();
        self.data
    }
}

pub struct ByteReader<R> {
    read: R,
    partial: u32,
    valid: u8,
}
impl<R: Iterator<Item=u8>> ByteReader<R> {
    /// Construct a new `ByteReader` from an iterator of `u8`
    pub fn new(read: R) -> Self {
        let mut bits = ByteReader {
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
}
impl<'a> ByteReader<std::iter::Cloned<std::slice::Iter<'a, u8>>> {
    /// Construct a new `ByteReader` from a slice of bytes.
    pub fn from_slice(slice: &'a [u8]) -> Self {
        ByteReader::new(slice.iter().cloned())
    }
}
impl<'a, R: Iterator<Item=u8> + 'a> ByteReader<R> {
    /// Turn the reader into an iterator of bits.
    /// 
    /// Yields one `bool` per bit, `1=true` and `0=false`.
    pub fn into_bits(mut self) -> impl Iterator<Item=bool> + 'a {
        std::iter::from_fn(move || {
            let bit = self.peek(1)? == 1;
            self.consume(1);
            Some(bit)
        })
    }
    
    /// Print the remaining data
    /// 
    /// Note: For debug purposes only, not part of the API.
    pub fn print_remaining(&mut self) {
        println!("partial: {:0w$b}, valid: {}", self.partial, self.valid, w=self.valid as usize);
        for b in self.read.by_ref() {
            print!("{:08b} ", b);
        }
        println!();
    }
}
impl<R: Iterator<Item=u8>> BitReader for ByteReader<R> {
    fn peek(&self, bits: u8) -> Option<u16> {
        assert!(bits <= 16);
        if self.valid >= bits {
            let shift = self.valid - bits;
            let out = (self.partial >> shift) as u16 & ((1 << bits) - 1);
            Some(out)
        } else {
            None
        }
    }
    fn consume(&mut self, bits: u8) {
        self.valid -= bits;
        self.fill();
    }
    fn bits_to_byte_boundary(&self) -> u8 {
        self.valid & 7
    }
}

#[test]
fn test_bits() {
    let mut bits = ByteReader::new([0b0000_1101, 0b1010_0000].iter().cloned());
    assert_eq!(maps::black::decode(&mut bits), Some(42));
}

/// Enum used to signal black/white.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Color {
    Black,
    White
}
impl Not for Color {
    type Output = Self;
    fn not(self) -> Self {
        match self {
            Color::Black => Color::White,
            Color::White => Color::Black,
        }
    }
}

struct Transitions<'a> {
    edges: &'a [u16],
    pos: usize
}
impl<'a> Transitions<'a> {
    fn new(edges: &'a [u16]) -> Self {
        Transitions { edges, pos: 0 }
    }
    fn seek_back(&mut self, start: u16) {
        while self.pos > 0 {
            if start < self.edges[self.pos-1] {
                self.pos -= 1;
            } else {
                break;
            }
        }
    }
    fn next_color(&mut self, start: u16, color: Color, start_of_row: bool) -> Option<u16> {
        if start_of_row {
            if color == Color::Black {
                return self.edges.get(0).cloned()
            } else {
                return self.edges.get(1).cloned()
            }
        }
        while self.pos < self.edges.len() {
            if self.edges[self.pos] <= start {
                self.pos += 1;
                continue;
            }

            if (self.pos % 2 == 0) != (color == Color::Black) {
                self.pos += 1;
            }

            break;
        }
        if self.pos < self.edges.len() {
            let val = self.edges[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }
    fn next(&mut self) -> Option<u16> {
        if self.pos < self.edges.len() {
            let val = self.edges[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }
    fn peek(&self) -> Option<u16> {
        self.edges.get(self.pos).cloned()
    }
    fn skip(&mut self, n: usize) {
        self.pos += n;
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bits {
    pub data: u16,
    pub len: u8
}

impl fmt::Debug for Bits {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "d={:0b} w={}", self.data, self.len)
    }
}
