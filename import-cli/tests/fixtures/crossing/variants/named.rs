// rawr
// String-like generics follow their bound, not their spelling (#713):
// `S: Into<String>` takes what `impl Into<String>` takes, and `AsRef<str>`,
// `AsRef<Path>`, `Into<PathBuf>`, `AsRef<[u8]>` and `ToString` take what
// std implements them for: a literal, a `String`, a borrow of one, a
// `PathBuf` and a borrow of one. Nothing is copied where Rust borrows.

use std::path::PathBuf;

use events::named::{measured, named, opened, owned, shown, spelled, summed};

fn main() {
    let name = String::from("Ada");
    println!("{}", named("Grace"));
    println!("{}", named(name.clone()));
    println!("{}", spelled("Grace"));
    println!("{}", spelled(name));

    let text = String::from("four");
    println!(
        "{} {} {}",
        measured("three"),
        measured(&text),
        measured(text)
    );

    let path = PathBuf::from("assets/fonts/inter.ttf");
    println!("{}", opened("assets/icon.png"));
    println!("{}", opened(&path));
    let place = String::from("docs/readme.md");
    println!("{}", opened(&place));
    println!("{}", opened(path.clone()));
    println!("{}", owned("target"));
    println!("{}", owned(path));
    println!("{}", owned(String::from("build")));

    let bytes = String::from("AB");
    println!("{} {}", summed("abc"), summed(&bytes));

    println!("{} {} {}", shown(7), shown("seven"), shown(2.5));
}
