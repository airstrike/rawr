//! This script defines a function with two parameters and local variables.

let a = 3;

fn add(mut a: i64, b: i64) -> i64 {
    a = 42;             // notice that 'a' is passed by value
    a + b               // the tail expression is the return value
}

let result = add(a, 4);

println!("add(a, 4) should be 46: {result}");

println!("a should still be 3: {a}");     // prints 3: 'a' is never changed
