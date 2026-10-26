fn main() {
    let mut v = vec![1];
    let w = vec![2];
    let mut k = 0;
    while {
        let mut r = &w;
        loop {
            v.push(k);
            let n = r.len();
            println!("{}", n);
            r = &v;
            k += 1;
            if k % 3 != 0 {
                continue;
            }
            break;
        }
        k < 6
    } {}
    println!("{:?}", v);
}
