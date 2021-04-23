use crate::{SliceBits, BitReader};
use crate::maps::{Mode, black, white, mode};
use std::ops::Not;

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

fn with_markup<D, R>(decoder: D, reader: &mut R) -> Option<u16>
    where D: Fn(&mut R) -> Option<u16>
{
    let mut sum = 0;
    while let Some(n) = decoder(reader) {
        //print!("{} ", n);
        sum += n;
        if n < 64 {
            //println!("= {}", sum);
            return Some(sum)
        }
    }
    None
}

fn colored(current: Color, reader: &mut impl BitReader) -> Option<u16> {
    //print!("{:?} ", current);
    match current {
        Color::Black => with_markup(black, reader),
        Color::White => with_markup(white, reader),
    }
}

pub fn pels(line: &[u16], width: u16) -> impl Iterator<Item=Color> + '_ {
    use std::iter::{repeat};
    let mut color = Color::White;
    let mut last = 0;
    line.iter().flat_map(move |&p| {
        let c = color;
        color = !color;
        let n = p.saturating_sub(last);
        last = p;
        repeat(c).take(n as usize)
    }).chain(repeat(color)).take(width as usize)
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
    fn next_color(&mut self, start: u16, color: Color) -> Option<u16> {
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
}

pub fn decode(input: impl Iterator<Item=u8>, width: u16, mut line_cb: impl FnMut(&[u16])) -> Option<()> {
    let mut reader = SliceBits::new(input);
    let mut reference: Vec<u16> = vec![];
    let mut current: Vec<u16> = vec![];

    'outer: for y in 0 .. {
        let mut transitions = Transitions::new(&reference);
        let mut a0 = 0;
        let mut color = Color::White;
        //println!("\n\nline {}", y);

        loop {
            //reader.print();
            let mode = match mode(&mut reader) {
                Some(mode) => mode,
                None => break 'outer,
            };
            //println!("{:?}, color={:?}, a0={}", mode, color, a0);
            

            match mode {
                Mode::Pass => {
                    let b1 = transitions.next_color(a0, !color).unwrap();
                    //println!("b1={}", b1);
                    if let Some(b2) = transitions.next() {
                        //println!("b2={}", b2);
                        a0 = b2;
                    }
                }
                Mode::Vertical(delta) => {
                    let b1 = match transitions.next_color(a0, !color) {
                        Some(p) => p,
                        None => break
                    };
                    let a1 = (b1 as i16 + delta as i16) as u16;
                    //println!("transition to {:?} at {}", !color, a1);
                    current.push(a1);
                    color = !color;
                    a0 = a1;
                    if delta < 0 {
                        transitions.seek_back(a0);
                    }
                }
                Mode::Horizontal => {
                    let a0a1 = colored(color, &mut reader)?;
                    let a1a2 = colored(!color, &mut reader)?;
                    let a1 = a0 + a0a1;
                    let a2 = a1 + a1a2;
                    //println!("a0a1={}, a1a2={}, a1={}, a2={}", a0a1, a1a2, a1, a2);
                    
                    current.push(a1);
                    if a2 >= width {
                        break;
                    }
                    current.push(a2);
                    a0 = a2;
                }
                Mode::Extension => {
                    let xxx = reader.peek(3).unwrap();
                    //println!("extension: {:03b}", xxx);
                    reader.consume(3);
                    //println!("{:?}", current);
                    break 'outer;
                }
            }

            if a0 >= width {
                break;
            }
        }
        //println!("{:?}", current);

        line_cb(&current);
        std::mem::swap(&mut reference, &mut current);
        current.clear();
    }
    //reader.print();

    Some(())
}
