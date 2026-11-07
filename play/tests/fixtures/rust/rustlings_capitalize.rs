// From rustlings (https://github.com/rust-lang/rustlings), solutions/18_iterators/iterators2.rs.
// Copyright (c) 2016 Carol (Nichols || Goulding). MIT licence:
// https://github.com/rust-lang/rustlings/blob/main/LICENSE
// Changed: the tests module is dropped, and main prints what the tests check.

// In this exercise, you'll learn some of the unique advantages that iterators
// can offer.

// "hello" -> "Hello"
fn capitalize_first(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    }
}

// Apply the `capitalize_first` function to a slice of string slices.
// Return a vector of strings.
// ["hello", "world"] -> ["Hello", "World"]
fn capitalize_words_vector(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| capitalize_first(word)).collect()
}

// Apply the `capitalize_first` function again to a slice of string
// slices. Return a single string.
// ["hello", " ", "world"] -> "Hello World"
fn capitalize_words_string(words: &[&str]) -> String {
    words.iter().map(|word| capitalize_first(word)).collect()
}

fn main() {
    println!("{}", capitalize_first("hello"));
    println!("{:?}", capitalize_first(""));

    let words = vec!["hello", "world"];
    println!("{:?}", capitalize_words_vector(&words));

    let words = vec!["hello", " ", "world"];
    println!("{}", capitalize_words_string(&words));
}
