// rawr
// A crate trait's `&mut self` method of its impl for `&str` that makes the
// string another one, called through a generic body's bound, as iced's
// widget code calls a string's `Widget` methods: the program's string is
// copied in and back out, as a `&mut String` input is, so the program
// reads the trimmed text after.

use events::visits::Trimmed;

/// How many bytes `text` loses to its trim.
fn cut<T: Trimmed>(text: &mut T) -> usize {
    text.trimmed()
}

fn main() {
    let mut text = "  calm  ";
    let cut = cut(&mut text);
    println!("{cut} [{text}]");
}
