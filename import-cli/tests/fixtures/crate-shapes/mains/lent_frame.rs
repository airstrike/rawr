// rawr
use parts::surface::{Frame, Page, Surface};

struct Counter {
    value: i32,
}

impl Counter {
    fn draw(&self, frame: &mut Frame) {
        frame.write(&format!("value {}", self.value));
    }
}

fn main() -> std::io::Result<()> {
    let mut surface = Surface::new(Page)?;
    let mut counter = Counter { value: 1 };
    for _ in 0..2 {
        let drawn = surface.draw(|frame| counter.draw(frame))?;
        println!("drew {} line(s), frame {}", drawn.lines.len(), drawn.count);
        counter.value += 1;
    }
    Ok(())
}
