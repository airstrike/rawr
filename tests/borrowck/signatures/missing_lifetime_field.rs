struct S {
    r: &i32,
}

fn main() {
    let x = 1;
    let s = S { r: &x };
    println!("{}", s.r);
}
