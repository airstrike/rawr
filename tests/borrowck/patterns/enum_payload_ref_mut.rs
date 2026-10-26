enum E {
    A(Vec<i64>),
    B(i64),
}
fn main() {
    let mut e = E::A(vec![1]);
    match e {
        E::A(ref mut v) => {
            v.push(2);
            println!("{}", v.len());
        }
        E::B(ref mut n) => *n += 1,
    }
    if let E::A(v) = &e {
        println!("{:?}", v);
    }
}
