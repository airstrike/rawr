// rawr
use std::cell::RefCell;
use std::rc::Rc;

use natives::painter::Painter;
use parts::surface::{Page, Surface};

fn main() -> std::io::Result<()> {
    let frames = Rc::new(RefCell::new(0));
    let surface = Surface::new(Page)?;
    surface.paint(move |frame| {
        *frames.borrow_mut() += 1;
        frame.write(&format!("frame {}", frames.borrow()));
    });
    Ok(())
}
