fn f() -> &'static Vec<i32> {
    let v = vec![1, 2];
    let r = &v;
    r
}

fn main() {
    println!("{:?}", f());
}
