fn main() {
    let mut v = vec![1];
    let w = vec![0];
    let mut r = &w;
    let mut n = 0;
    'w: while {
        v.push(n);
        let k = r.len();
        n += 1;
        loop {
            r = &v;
            if n % 2 == 1 && k > 0 {
                continue 'w;
            }
            break;
        }
        r = &w;
        n < 5
    } {}
    println!("{} {:?}", r.len(), v);
}
