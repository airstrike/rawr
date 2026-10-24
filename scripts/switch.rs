//! This script runs a match expression in a for-loop.

let arr = [42, 123, 999, 1, 2, 3, 4, -7];

for item in arr {
    match item {
        // Match single integer
        42 => println!("The Answer!"),
        // Match one of several integers
        123 | 456 => println!("A favourite number: {item}"),
        // Match another integer
        999 => println!("Got 999: {item}"),
        // Match range with condition
        0..100 if item % 2 == 0 => println!("A small even number: {item}"),
        // Match another range
        0..100 => println!("A small odd number: {item}"),
        // Default case
        _ => println!("Something else: <{item}>"),
    }
}
