fn main() {
    let mut v = vec![1, 2];
    let mut c = || {
        let r = &mut v;
        v.len();
        r.push(1);
    };
    c();
}
