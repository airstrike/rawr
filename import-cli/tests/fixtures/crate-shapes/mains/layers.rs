// rawr
// A method whose receiver's lifetime is the one its `&mut` input's type
// carries keeps its receiver borrowed for that input's life, as
// integration's `scene.draw(&mut render_pass)` lends the pass its scene.
// The layer is changed again once the pass has dropped.
use natives::passes::{Encoder, Layer, Target};

fn main() {
    let target = Target::new("frame");
    let mut layer = Layer::new("sky");
    let mut encoder = Encoder::new();
    {
        let mut pass = Target::clear(&target, &mut encoder, 7);
        layer.draw(&mut pass);
        pass.draw(1);
    }
    layer.rename("sea");
    println!("{}", layer.name());
    println!("{}", encoder.finish());
}
