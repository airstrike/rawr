//! This script runs for-loops.

let arr = [1, 2, 3, 42];

// Loop over an array with a counter
for (i, a) in arr.iter().enumerate() {
    for (j, b) in [7, 42, 123, 99].iter().enumerate() {
        if *b > 100 { continue; }

        println!("({i}, {j}) = ({a}, {b})");
    }

    if *a == 3 { break; }
}
//println!("{a}");          // <- if you uncomment this line, the script will fail to compile
                            //    because 'a' is not defined here

for i in (1..=5).rev() {    // runs from 5 down to 1
    println!("{i}");
}
