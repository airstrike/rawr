// rawr
// iced's solar_system in the fixture: a borrow of a crate value the
// program keeps fills a crate function's `impl Into<Handle>` and `impl
// Into<Image>`, as `canvas::Image::new(&self.earth)` and
// `frame.draw_image(bounds, &self.sun)` do (#954).

use events::image::{self, Handle, Image};

struct State {
    sun: Handle,
    earth: Handle,
}

fn main() {
    let state = State {
        sun: Handle::from_name("sun"),
        earth: Handle::from_name("earth"),
    };
    println!("{}", image::draw(&state.sun));
    println!("{}", image::draw(Image::new(&state.earth).rotation(2.5)));
    println!("{}", image::draw(&state.sun));
    // Bytes the program includes, as `Handle::from_bytes(include_bytes!(..)
    // .as_slice())`.
    let moon = Handle::from_bytes(include_bytes!("main.rs").as_slice());
    println!("{}", image::draw(&moon) == image::draw(Image::new(&moon)));
}
