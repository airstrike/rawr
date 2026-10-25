// rawr
//! Which lines match, and how they print.

pub struct Search {
    pub pattern: String,
    pub ignore_case: bool,
    pub line_numbers: bool,
    pub count: bool,
    pub invert: bool,
}

impl Search {
    pub fn new() -> Search {
        Search {
            pattern: String::new(),
            ignore_case: false,
            line_numbers: false,
            count: false,
            invert: false,
        }
    }

    /// Whether `line` is one to print.
    pub fn matches(&self, line: &str) -> bool {
        let found = if self.ignore_case {
            line.to_lowercase().contains(&self.pattern.to_lowercase())
        } else {
            line.contains(&self.pattern)
        };
        found != self.invert
    }

    /// How line `number` prints, after `name` when several files are
    /// searched.
    pub fn show(&self, name: Option<&str>, number: usize, line: &str) -> String {
        let mut shown = String::new();
        if let Some(name) = name {
            shown.push_str(&format!("{name}:"));
        }
        if self.line_numbers {
            shown.push_str(&format!("{number}:"));
        }
        shown.push_str(line);
        shown
    }

    /// The count `-c` prints, after `name` when several files are searched.
    pub fn total(&self, name: Option<&str>, count: usize) -> String {
        match name {
            Some(name) => format!("{name}:{count}"),
            None => format!("{count}"),
        }
    }
}
