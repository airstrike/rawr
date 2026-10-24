//! This script implements one trait method for several data types.

trait Calc {
    fn calc(&self, x: i64) -> i64;
}

// For strings
impl Calc for &str {
    fn calc(&self, x: i64) -> i64 {
        self.len() as i64 + x
    }
}
// For integers
impl Calc for i64 {
    fn calc(&self, x: i64) -> i64 {
        self * x
    }
}
// For booleans
impl Calc for bool {
    fn calc(&self, x: i64) -> i64 {
        if *self { x } else { 0 }
    }
}
// For vectors
impl Calc for Vec<i64> {
    fn calc(&self, x: i64) -> i64 {
        self.len() as i64 + x
    }
}

println!("{}", "hello".calc(42));           // 47
println!("{}", 42_i64.calc(42));            // 1764
println!("{}", true.calc(42));              // 42
println!("{}", false.calc(42));             // 0
println!("{}", vec![1, 2, 3].calc(42));     // 45
