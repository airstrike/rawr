fn main() {
    let v = vec![1, 2];
    let r = &v;
    let c = || {
        let w = v;
        w.len()
    };
    println!("{}", r.len());
    println!("{}", c());
}
