fn f() -> Option<&'static i32> {
    let x = 1;
    Some(&x)
}

fn main() {
    println!("{:?}", f());
}
