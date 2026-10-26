fn main() {
    let mut v = vec![1, 2];
    let mut outer = || {
        let mut inner = || v.push(1);
        inner();
    };
    v.len();
    outer();
}
