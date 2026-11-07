// From rustlings (https://github.com/rust-lang/rustlings), solutions/15_traits/traits1.rs.
// Copyright (c) 2016 Carol (Nichols || Goulding). MIT licence:
// https://github.com/rust-lang/rustlings/blob/main/LICENSE
// Changed: the tests module is dropped.

// The trait `AppendBar` has only one function which appends "Bar" to any object
// implementing this trait.
trait AppendBar {
    fn append_bar(self) -> Self;
}

impl AppendBar for String {
    fn append_bar(self) -> Self {
        self + "Bar"
    }
}

fn main() {
    let s = String::from("Foo");
    let s = s.append_bar();
    println!("s: {s}");
}
