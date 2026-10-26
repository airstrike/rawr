fn main() {
    let mut v = vec![1, 2];
    let s: &mut [i64] = &mut v;
    let e = &mut s[0];
    let n = match s {
        [] => 0,
        _ => 1,
    };
    *e += 1;
    println!("{} {:?}", n, v);
}
