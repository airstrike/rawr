// rawr
use natives::button::{Button, Status, Style, Theme};

fn main() {
    let button = Button::new(1).on_press_with(|| 2);
    println!("pressed {}", button.press());
    let button = button.style(|theme: &Theme, status: Status| match status {
        Status::Active => Style { width: theme.scale },
        Status::Hovered => Style {
            width: theme.scale * 2,
        },
    });
    let theme = Theme { scale: 3 };
    let (active, hovered) = button.widths(&theme);
    println!("widths {active} {hovered}");
}
