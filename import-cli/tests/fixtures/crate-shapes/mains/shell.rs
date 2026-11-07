// rawr
use natives::shell::{self, Shell, Widget};

struct Rainbow {
    seen: u32,
}

// iced's geometry: `update` takes `&mut Shell<'_, Message>`.
impl Widget<u32> for Rainbow {
    fn update(&mut self, cursor: u32, shell: &mut Shell<'_, u32>) {
        self.seen += 1;
        if cursor > 2 && shell.is_empty() {
            shell.request_redraw();
        }
    }
}

fn main() {
    let mut rainbow = Rainbow { seen: 0 };
    let redraws = shell::drive(&mut rainbow, &[1, 3, 5, 2]);
    println!("{redraws} {}", rainbow.seen);
}
