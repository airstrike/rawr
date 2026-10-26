fn main() {
    let mut v = vec![1];
    let w = vec![2, 3];
    let mut r = &w;
    let mut n = 0;
    'w: while {
        loop {
            r = &v;
            if n > 1 {
                break 'w;
            }
            break;
        }
        r = &w;
        n += 1;
        true
    } {}
    v.push(n);
    println!("{} {:?}", r.len(), v);
}
