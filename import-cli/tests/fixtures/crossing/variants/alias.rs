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
    // A provided method of the trait lends the renderer to a closure as
    // its `&mut Self`, the alias's instance, as iced's
    // `renderer.with_translation(v, |renderer| renderer.draw_mesh(mesh))`.
    let quad = Quad { size: 4, radius: 1 };
    renderer.with_translation(3, |renderer| {
        renderer.fill_quad(quad, events::Color::rgb(4, 5, 6));
    });
    println!("{}", renderer.rendered());
}
