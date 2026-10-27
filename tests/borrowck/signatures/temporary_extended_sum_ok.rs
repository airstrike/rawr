fn main() {
    let n = 2;
    let r = &(n + 1);
    let v = &vec![n];
    let f = &format!("{n}");
    println!("{r} {v:?} {f}");
}
