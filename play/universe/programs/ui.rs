use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Paragraph, Widget};

fn draw(area: Rect, buffer: &mut Buffer) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    Paragraph::new("Water the plants")
        .block(Block::bordered().title(" New to-do "))
        .render(areas[0], buffer);
    Paragraph::new("✓ Buy milk\n☐ Walk the dog\n☐ Try rawr")
        .block(Block::bordered().title(" To do: 2 left "))
        .render(areas[1], buffer);
    Paragraph::new("a adds · Space checks off").render(areas[2], buffer);
}

pub fn main() {
    let area = Rect::new(0, 0, 36, 9);
    let mut buffer = Buffer::empty(area);
    draw(area, &mut buffer);
    for y in 0..area.height {
        let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
        println!("{}", row.trim_end());
    }
}
