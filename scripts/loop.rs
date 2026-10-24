//! This script runs an infinite loop, ending it with a break statement.

let mut x = 10;

// simulate do..while using loop
loop {
    println!("{x}");

    x -= 1;

    if x <= 0 { break; }
}
