fn main() {
    let mut v = vec![1, 2, 3];
    if let Some(x) = v.get_mut(1) {
        let mut inc = || *x += 10;
        *x += 1;
        inc();
    }
    println!("{v:?}");
}
