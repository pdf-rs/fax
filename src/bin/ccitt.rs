use fax::decoder::{decode, pels, Color};
use image::{GrayImage, Luma};

fn main() {
    let mut args = std::env::args().skip(1);
    let input: String = args.next().unwrap();
    let width: u16 = args.next().unwrap().parse().unwrap();
    let height: u16 = args.next().unwrap().parse().unwrap();
    let output = args.next().unwrap();

    let data = std::fs::read(&input).unwrap();
    let mut image = GrayImage::new(width as u32, height as u32);
    let mut rows = image.rows_mut();

    let mut row_nr = 0;

    decode(data.iter().cloned(), width, |transitions| {
        let row = rows.next().unwrap();
        //println!("{} {:?}", row_nr, transitions);

        for (c, p) in pels(transitions, width).zip(row) {
            *p = match c {
                Color::Black => Luma([0]),
                Color::White => Luma([255])
            };
        }
        row_nr += 1;
    });

    image.save(output).unwrap();
}