fn main() {
    let mut v = vec![1];
    let w = vec![2];
    let mut r = &w;
    let mut k = 0;
    while {
        v.push(k);
        println!("{}", r.len());
        loop {
            r = &v;
            k += 1;
            if k % 2 == 1 {
                continue;
            }
            r = &w;
            break;
        }
        k < 6
    } {}
    println!("{:?}", v);
}
