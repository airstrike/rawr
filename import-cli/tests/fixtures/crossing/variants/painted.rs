// rawr
use events::pixels::{self, Buffer, Image, Pixel, Strip};

fn main() {
    let mut buffer = Buffer::new(4);
    buffer.get_mut(0).0 = [1, 2, 3, 4];
    *buffer.get_mut(1) = Pixel([5, 6, 7, 8]);
    buffer.get_mut(1).0[3] += 1;
    let pixel = buffer.get_mut(2);
    pixel.0[0] = 9;
    pixel.0[1] += 3;
    *pixel = Pixel([pixel.0[0], pixel.0[1], 11, 12]);
    buffer.pixel_mut(3).0 = [13, 14, 15, 16];
    buffer.pixel_mut(3).0[0] *= 2;
    pixels::fill(buffer.get_mut(0), 7);
    println!("{:?}", buffer.get(1));
    for index in 0..4 {
        buffer.get_mut(index).0[2] += 1;
    }
    if let Some(pixel) = buffer.pixel(3) {
        pixel.0[1] += 1;
        pixel.0[2] = 40;
    }
    println!("{}", buffer.pixel(4).is_none());
    println!("{}", buffer.describe());
    let name = String::from("strip");
    let mut strip = Strip::new(&name, 3);
    strip.get_mut(0).0[0] += 2;
    let pixel = strip.get_mut(1);
    pixel.0[1] = 5;
    pixel.0[2] += 6;
    if let Some(pixel) = strip.pixel(2) {
        pixel.0[3] = 7;
        pixel.0[3] *= 3;
    }
    println!("{}", strip.pixel(3).is_none());
    println!("{}", strip.describe());
    println!("lent {} times", buffer.lent());
}
