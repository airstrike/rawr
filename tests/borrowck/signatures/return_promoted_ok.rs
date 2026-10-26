fn f() -> &'static i32 {
    &5
}

fn g() -> &'static [i32] {
    &[1, 2, 3]
}

fn main() {
    println!("{} {:?}", f(), g());
}
