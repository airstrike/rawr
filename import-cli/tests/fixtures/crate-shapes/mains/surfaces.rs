// rawr
// A frame bound by value out of a crate enum's variant moves into the call
// that takes it, as integration's `queue.present(frame)` moves the
// `SurfaceTexture` its `Success(frame)` arm bound: it is presented once and
// never discarded, though its type is `Clone` (#2000). The borrow
// `frame.texture.format()` makes of the frame ends with its statement.
use natives::surfaces::{Current, Queue, Surface};

fn present(queue: &Queue, surface: Surface) {
    match surface.current() {
        Current::Success(frame) => {
            let format = frame.texture.format();
            println!("{format:?}");
            queue.present(frame);
        }
        Current::Lost => println!("lost"),
    }
}

fn main() {
    let queue = Queue;
    present(&queue, Surface(7));
    present(&queue, Surface(8));
    println!("end");
}
