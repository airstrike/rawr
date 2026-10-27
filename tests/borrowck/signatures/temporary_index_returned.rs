fn first() -> &'static i32 {
    &vec![1, 2][0]
}

fn main() {
    println!("{}", first());
}
