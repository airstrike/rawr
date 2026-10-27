fn main() {
    let mut v = vec![1];
    let w = vec![0];
    let mut r = &w;
    let mut n = 0;
    'w: while {
        r = &w;
        n += 1;
        loop {
            v.push(n);
            let k = r.len();
            r = &v;
            if n % 2 == 1 && k > 0 {
                continue 'w;
            }
            r = &w;
            break;
        }
        n < 5
    } {}
    println!("{} {:?}", r.len(), v);
}
