fn main() {
    let mut v = vec![1];
    let mut n = 0;
    while {
        let mut r = &v;
        loop {
            r = &v;
            break;
        }
        v.push(n);
        n += 1;
        r.len() < 4
    } {}
    println!("{:?}", v);
}
