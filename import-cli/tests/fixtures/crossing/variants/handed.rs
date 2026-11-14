// rawr
use events::handed::{self, Bytes, Line};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Elm,
}

impl From<Language> for String {
    fn from(language: Language) -> String {
        String::from(match language {
            Language::Rust => "Rust",
            Language::Elm => "Elm",
        })
    }
}

fn main() {
    println!(
        "{} {}",
        handed::labelled(Language::Rust),
        handed::labelled(Language::Elm)
    );
    let line = Line::from_iter([handed::span("tr"), handed::span("unk")]);
    println!("{}", line.text());
    let bytes = Bytes::new(&[3, 4, 5]);
    let total = handed::off_thread(move || {
        bytes
            .as_slice()
            .iter()
            .map(|byte| u32::from(*byte))
            .sum::<u32>()
    });
    println!("{total}");
}
