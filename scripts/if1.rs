//! This script runs if statements.

let a = 42;
let b = 123;
let x = 999;

if a > b {
    println!("Oops! a > b");
} else if a < b {
    println!("a < b, x should be 0");

    let x = 0;          // <- this 'x' shadows the outer 'x'
    println!("{x}");    // should print 0
} else {
    println!("Oops! a == b");
}

println!("x should be 999 again: {x}");
