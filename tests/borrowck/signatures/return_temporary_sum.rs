fn f(n: i32) -> &'static i32 {
    &(n + 1)
}

fn main() {
    println!("{}", f(1));
}
