// rawr
use events::custom::{self, Quad, Renderer};

fn main() {
    let mut renderer: custom::Aliased = custom::aliased();
    renderer.fill_quad(
        Quad {
            size: 10,
            radius: 2,
        },
        events::Color::rgb(1, 2, 3),
    );
    println!("{}", renderer.rendered());
}
