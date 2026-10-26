fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let c = || {
        let n = 1;
        v.len() + n
    };
    r.push(1);
    println!("{}", c());
}
