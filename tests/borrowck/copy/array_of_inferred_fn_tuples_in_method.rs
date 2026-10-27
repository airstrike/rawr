mod style {
    pub struct Theme {
        pub base: i32,
    }

    #[derive(Clone, Copy)]
    pub enum Status {
        Active,
        Hovered,
    }

    pub fn primary(theme: &Theme, status: Status) -> i32 {
        match status {
            Status::Active => theme.base,
            Status::Hovered => theme.base + 1,
        }
    }

    pub fn secondary(theme: &Theme, _status: Status) -> i32 {
        theme.base * 10
    }
}

use style::{Status, Theme};

struct App {
    theme: Theme,
}

impl App {
    fn view(&self) -> (Vec<String>, Vec<i32>) {
        let styles = [("Primary", style::primary as fn(&Theme, _) -> _), ("Secondary", style::secondary)];
        let label = |name: &str| format!("<{name}>");
        let first = styles.into_iter().map(|(name, style)| label(name) + &style(&self.theme, Status::Active).to_string()).collect();
        let second = styles.into_iter().map(|(_, style)| style(&self.theme, Status::Hovered)).collect();
        (first, second)
    }
}

fn main() {
    let app = App { theme: Theme { base: 2 } };
    let (first, second) = app.view();
    println!("{first:?} {second:?}");
}
