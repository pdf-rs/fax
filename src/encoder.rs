use crate::{Color, BitWriter, Transitions, maps::{Mode, mode, black, white, EDFB_HALF}};

fn absdiff(a: u16, b: u16) -> u16 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

pub struct Encoder<W> {
    writer: W,
    reference: Vec<u16>,
    current: Vec<u16>,
}
fn encode_color(writer: &mut impl BitWriter, color: Color, mut n: u16) {
    let table = match color {
        Color::White => &white::ENTRIES,
        Color::Black => &black::ENTRIES,
    };
    let mut write = |n: u16| {
        let idx = if n >= 64 { 63 + n / 64 } else { n } as usize;
        let (v, bits) = table[idx];
        assert_eq!(v, n);
        //println!("{}", n);
        writer.write(bits);
    };
    
    while n >= 2560 {
        write(2560);
        n -= 2560;
    }
    if n >= 64 {
        let d = n & !63;
        write(d);
        n -= d;
    }

    write(n);
}
impl<W: BitWriter> Encoder<W> {
    pub fn new(writer: W) -> Self {
        Encoder {
            writer,
            reference: vec![],
            current: vec![],
        }
    }
    pub fn encode_line(&mut self, pels: impl Iterator<Item=Color>, width: u16) {
        let mut color = Color::White;
        let mut transitions = Transitions::new(&self.reference);
        let mut a0 = 0;
        let mut start_of_row = true;
        let mut pels = pels.enumerate()
        .scan(Color::White, |state, (i, c)| {
            Some(if c != *state {
                *state = c;
                Some(i as u16)
            } else {
                None
            })
        }).filter_map(|x| x);
        let writer = &mut self.writer;
        self.current.clear();

        while let Some(a1) = pels.next() {
            //println!("a1={}", a1);
            self.current.push(a1);
            loop {
                transitions.seek_back(a0);
                let b1 = transitions.next_color(a0, !color, start_of_row);
                let b2 = transitions.peek();

                start_of_row = false;
                //println!("b1={:?}, b2={:?}", b1, b2);
                match (b1, b2) {
                    (Some(_b1), Some(b2)) if b2 < a1 => {
                        //println!("Pass");
                        let bits = mode::encode(Mode::Pass).unwrap();
                        writer.write(bits);
                        transitions.skip(1);
                        a0 = b2;
                        continue;
                    }
                    (Some(b1), _) if absdiff(a1, b1) <= 3 => {
                        let delta = a1 as i16 - b1 as i16;
                        //println!("Vertical({})", delta);
                        let bits = mode::encode(Mode::Vertical(delta as i8)).unwrap();
                        writer.write(bits);
                        a0 = a1;
                        color = !color;
                    }
                    _ => {
                        let a2 = match pels.next() {
                            Some(a2) => {
                                self.current.push(a2);
                                a2
                            },
                            None => width
                        };
                        let bits = mode::encode(Mode::Horizontal).unwrap();
                        writer.write(bits);
                        let a0a1 = a1 - a0;
                        let a1a2 = a2 - a1;
                        //println!("Horizontal({}, {})", a0a1, a1a2);
                        encode_color(writer, color, a0a1);
                        encode_color(writer, !color, a1a2);
                        a0 = a2;
                    }
                }
                break;
            }
        }
        transitions.seek_back(a0);
        loop {
            let b1 = transitions.next_color(a0, !color, start_of_row);
            let b2 = transitions.peek();
            //println!("b1={:?}, b2={:?}", b1, b2);
            if let Some(b1) = b1 {
                //println!("Pass");
                let bits = mode::encode(Mode::Pass).unwrap();
                writer.write(bits);
                transitions.skip(1);
                if let Some(b2) = b2 {
                    a0 = b2;
                } else {
                    a0 = b1;
                    break;
                }
            } else {
                break;
            }
        }
        if a0 < width {
            //println!("Vertical(0)");
            let bits = mode::encode(Mode::Vertical(0)).unwrap();
            writer.write(bits);
        }
        std::mem::swap(&mut self.reference, &mut self.current);
    }
    pub fn finish(mut self) -> W {
        self.writer.write(EDFB_HALF);
        self.writer.write(EDFB_HALF);
        self.writer
    }
}