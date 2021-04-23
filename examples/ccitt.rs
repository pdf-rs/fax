use fax::decoder::{decode, pels, Color};
use image::{GrayImage, Luma, ImageFormat, buffer::Pixels};
use std::fs::File;
use std::io::BufReader;
fn load(p: &str) -> BufReader<File> {
    BufReader::new(File::open(p).unwrap())
}

fn fill_expected<'a>(buf: &mut Vec<u16>, row: Pixels<'a, Luma<u8>>) {
    let expected = row.enumerate().scan(255, |state, (i, p)| {
        let &Luma([p]) = p;
        Some(if p != *state {
            *state = p;
            Some(i as u16)
        } else {
            None
        })
    }).filter_map(|x| x);
    buf.clear();
    buf.extend(expected);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let reference = args.next().unwrap();
    let input: String = args.next().unwrap();
    let width: u16 = args.next().unwrap().parse().unwrap();
    let height: u16 = args.next().unwrap().parse().unwrap();
    let output = args.next().unwrap();


    let reference = image::load(load(&reference), ImageFormat::Pnm).unwrap();
    let reference = reference.as_luma8().unwrap();
    let mut reference_rows = reference.rows();

    let data = std::fs::read(&input).unwrap();
    let mut image = GrayImage::new(width as u32, height as u32);
    let mut rows = image.rows_mut();

    let mut row_nr = 0;
    let mut expected_buf = Vec::new();

    let t0 = std::time::Instant::now();
    decode(data.iter().cloned(), width, |transitions| {
        fill_expected(&mut expected_buf, reference_rows.next().unwrap());
        assert_eq!(expected_buf, transitions);

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
    let dt = t0.elapsed();
    println!("{}us ({}Mpixel/s", dt.as_micros(), (width as f64 * height as f64) / dt.as_secs_f64() * 1e-6);

    image.save(output).unwrap();
}