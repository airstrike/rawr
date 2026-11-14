// rawr
// ferris's `*self.rotation.radians_mut() = (self.rotation.radians() + ..)
// % ..` (#1945): an assignment's value is evaluated before its place, as
// rustc evaluates it, so the value's reads of the buffer end before
// `get_mut` lends the pixel the assignment writes.
use events::pixels::{Buffer, Pixel};

struct Wheel {
    buffer: Buffer,
}

impl Wheel {
    fn spin(&mut self) {
        *self.buffer.get_mut(0) = Pixel([self.buffer.get(1).0[0] + 1, 0, 0, 0]);
        self.buffer.get_mut(1).0 = [self.buffer.get(0).0[0] * 2, 0, 0, 0];
    }
}

fn main() {
    let mut wheel = Wheel {
        buffer: Buffer::new(2),
    };
    wheel.buffer.get_mut(1).0[0] = 4;
    wheel.spin();
    println!("{}", wheel.buffer.describe());
}
