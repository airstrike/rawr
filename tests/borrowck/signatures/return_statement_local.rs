fn f(c: bool) -> &'static i32 {
    let x = 1;
    if c {
        return &x;
    }
    &2
}

fn main() {
    println!("{}", f(false));
}
