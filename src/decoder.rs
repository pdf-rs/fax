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

fn color_change() -> impl Iterator<Item=Color> {
    let mut c = Color::Black;
    std::iter::from_fn(move || {
        c = !c;
        Some(c)
    })
}

pub fn decode(input: impl Iterator<Item=u8>, width: u16, mut line_cb: impl FnMut(&[u16])) -> Option<()> {
    let mut reader = SliceBits::new(input);
    let mut reference: Vec<u16> = vec![];
    let mut current: Vec<u16> = vec![];

    'outer: loop {
        let mut transitions = reference.iter().cloned().zip(color_change());
        let mut a0 = 0;
        let mut color = Color::White;

        loop {
            let mode = match mode(&mut reader) {
                Some(mode) => mode,
                None => break 'outer,
            };
            //println!("{:?}", mode);
            

            match mode {
                Mode::Pass => {
                    let (b1, _) = transitions.by_ref().skip_while(|&(b, c)| b <= a0 || c != color).next().unwrap();
                    let (b2, _) = transitions.next().unwrap();
                    a0 = b2;
                }
                Mode::Vertical(delta) => {
                    let (b1, _) = match transitions.by_ref().skip_while(|&(b, c)| b <= a0 || c != color).next() {
                        Some(p) => p,
                        None => break
                    };
                    let a1 = (b1 as i16 + delta as i16) as u16;
                    current.push(a1);
                    color = !color;
                    a0 = a1;
                }
                Mode::Horizontal => {
                    let a0a1 = colored(color, &mut reader)?;
                    let a1a2 = colored(!color, &mut reader)?;
                    let a1 = a0 + a0a1;
                    let a2 = a1 + a1a2;
                    //println!("a0a1={}, a1a2={}, a1={}, a2={}", a0a1, a1a2, a1, a2);
                    current.push(a1);
                    current.push(a2);
                    a0 = a2;
                }
            }

            if a0 >= width {
                break;
            }
        }

        line_cb(&current);
        std::mem::swap(&mut reference, &mut current);
        current.clear();
    }

    Some(())
}
