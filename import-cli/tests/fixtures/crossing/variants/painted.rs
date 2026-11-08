// rawr
use events::pixels::{self, Buffer, Image, Pixel};

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
    println!("{}", buffer.describe());
}
