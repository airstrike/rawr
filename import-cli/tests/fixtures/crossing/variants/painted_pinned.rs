// rawr
use events::pixels::Buffer;

fn main() {
    let mut buffer = Buffer::new(2);
    let pixel = buffer.get_mut(0);
    println!("{}", buffer.lent());
    pixel.0[0] = 1;
}
