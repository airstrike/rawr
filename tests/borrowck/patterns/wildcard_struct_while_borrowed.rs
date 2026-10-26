struct S {
    a: i64,
}
fn main() {
    let mut s = S { a: 1 };
    let m = &mut s;
    match s {
        _ => println!("any"),
    }
    m.a += 1;
    println!("{}", s.a);
}
