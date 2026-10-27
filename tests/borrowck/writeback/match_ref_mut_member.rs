fn main() {
    let mut p = (Some(vec![1]), 2);
    match p {
        (Some(ref mut v), _) => {
            p.1 = 3;
            v.push(4);
        }
        (None, _) => p.1 = 0,
    }
    println!("{:?}", p);
}
