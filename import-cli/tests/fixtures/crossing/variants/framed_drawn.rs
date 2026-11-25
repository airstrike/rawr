// rawr
// A crate trait whose method takes `&mut` of its parameter, as iced's
// `Widget::draw` takes `&mut Renderer`: the program's types and frames
// of them cross as its stand-in, which lends the crate's canvas.
use events::framed::{drawn_all, framed, Canvas, Drawn, Label, Meta, Shown};

/// A mood the program draws.
struct Mood(&'static str);

impl Drawn<Canvas> for Mood {
    fn draw(&self, canvas: &mut Canvas) {
        canvas.mark("~");
        canvas.mark(self.0);
        canvas.mark("~");
    }
}

impl Meta for Mood {}

impl Shown for Mood {
    fn shown(&self) -> String {
        self.0.to_owned()
    }
}

fn main() {
    let items: Vec<Box<dyn Drawn<Canvas>>> = vec![
        Box::new(framed(Label("calm".into()))),
        Box::new(Mood("plain")),
        Box::new(framed(framed(Mood("deep")))),
    ];
    println!("{}", drawn_all(&items));
}
