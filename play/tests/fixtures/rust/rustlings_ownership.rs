// From rustlings (https://github.com/rust-lang/rustlings), solutions/06_move_semantics/move_semantics5.rs.
// Copyright (c) 2016 Carol (Nichols || Goulding). MIT licence:
// https://github.com/rust-lang/rustlings/blob/main/LICENSE

#![allow(clippy::ptr_arg)]

// Borrows instead of taking ownership.
// It is recommended to use `&str` instead of `&String` here. But this is
// enough for now because we didn't handle strings yet.
fn get_char(data: &String) -> char {
    data.chars().last().unwrap()
}

// Takes ownership instead of borrowing.
fn string_uppercase(mut data: String) {
    data = data.to_uppercase();

    println!("{data}");
}

fn main() {
    let data = "Rust is great!".to_string();

    get_char(&data);

    string_uppercase(data);
}
