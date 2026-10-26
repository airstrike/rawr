enum E {
    A(i64),
}
fn touch(_: &mut E) {}
fn main() {
    let mut e = E::A(1);
    let m = &mut e;
    match e {
        E::A(_) => println!("a"),
    }
    touch(m);
}
