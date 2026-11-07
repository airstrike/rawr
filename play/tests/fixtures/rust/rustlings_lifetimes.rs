// From rustlings (https://github.com/rust-lang/rustlings), solutions/16_lifetimes/lifetimes2.rs.
// Copyright (c) 2016 Carol (Nichols || Goulding). MIT licence:
// https://github.com/rust-lang/rustlings/blob/main/LICENSE
// Changed: `longest` is laid out by rawr's rustfmt settings.

fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}

fn main() {
    let string1 = String::from("long string is long");
    // Solution 1: You can move `strings2` out of the inner block so that it is
    // not dropped before the print statement.
    let string2 = String::from("xyz");
    let result;
    {
        result = longest(&string1, &string2);
    }
    println!("The longest string is '{result}'");
    // `string2` dropped at the end of the function.

    // =========================================================================

    let string1 = String::from("long string is long");
    let result;
    {
        let string2 = String::from("xyz");
        result = longest(&string1, &string2);
        // Solution 2: You can move the print statement into the inner block so
        // that it is executed before `string2` is dropped.
        println!("The longest string is '{result}'");
        // `string2` dropped here (end of the inner scope).
    }
}
