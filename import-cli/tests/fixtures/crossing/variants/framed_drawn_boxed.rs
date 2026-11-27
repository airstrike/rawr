// rawr
// A crate trait's provided method at the program's frame of a crate value
// whose type parameter only the frame's impl fixes, as iced's
// `container(text(..))` gives the text the container's `Theme` through
// `impl Widget for Container<W, Theme> where W: Widget<.., Theme, ..>`.
use events::framed::{framed, Canvas, Drawn};
use events::held::{drawings, swatch};

/// A mood the program draws.
struct Mood(&'static str);

impl Drawn<Canvas> for Mood {
    fn draw(&self, canvas: &mut Canvas) {
        canvas.mark("~");
        canvas.mark(self.0);
    }
}

fn main() {
    let items = [
        framed(swatch("teal")).drawing(),
        framed(framed(swatch("rust"))).edge('#').drawing(),
        Mood("calm").drawing(),
    ];
    println!("{}", drawings(&items));
}
