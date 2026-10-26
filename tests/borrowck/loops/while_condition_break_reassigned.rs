fn main() {
    let mut v = vec![1];
    let w = vec![2, 3];
    let mut r = &w;
    let mut n = 0;
    while {
        loop {
            r = &v;
            break;
        }
        r = &w;
        n += 1;
        n < 3
    } {}
    v.push(n);
    println!("{} {:?}", r.len(), v);
}
