// rawr
use events::custom::{self, Length, Nested, Size, Tree};

struct Change;
impl custom::Layout for Change {
    fn change(&self, tree: &mut Tree) {
        tree.size = Size::new(12.0, 14.0);
        tree.size.width = tree.size.width + 1.0;
    }
}

fn returned() -> Size<u16> {
    Size {
        width: 3,
        height: 4,
    }
}

fn main() {
    let mut size = Size {
        width: Length::Fill,
        height: Length::Shrink,
    };
    size.width = Length::Shrink;
    println!("{}", custom::requested(size));
    let mut size: Size<u16> = Size {
        width: 3,
        height: 4,
    };
    size.width = 9;
    println!("{}", custom::numbers(size));
    let later = Size {
        width: 3,
        height: 4,
    };
    println!("{}", custom::numbers(later));
    println!("{}", custom::numbers(returned()));
    println!("{}", custom::nested(Nested { value: Some(7u8) }));
    println!("{}", custom::layout(Change));
}
