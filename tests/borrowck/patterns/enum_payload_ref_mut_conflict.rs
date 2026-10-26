enum E {
    A(Vec<i64>),
    B(i64),
}
fn main() {
    let mut e = E::A(vec![1]);
    match e {
        E::A(ref mut v) => {
            let m = matches!(e, E::B(_));
            v.push(2);
            println!("{}", m);
        }
        E::B(ref mut n) => *n += 1,
    }
}
