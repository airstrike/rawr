fn f() -> &'static Vec<i32> {
    &vec![1, 2]
}

fn main() {
    println!("{:?}", f());
}
