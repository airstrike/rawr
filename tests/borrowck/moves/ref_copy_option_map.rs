fn f(o: &Option<i32>) -> i32 {
    o.map(|n| n + 1).unwrap_or(0)
}

fn main() {
    println!("{}", f(&Some(2)));
}
