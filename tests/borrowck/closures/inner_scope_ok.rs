fn main() {
    let n = {
        let v = vec![1, 2];
        let c = || v.len();
        c()
    };
    println!("{}", n);
}
