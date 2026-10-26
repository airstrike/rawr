fn main() {
    let mut v = vec![0, 1];
    let x = v.get({
        v.push(2);
        0
    });
    println!("{x:?}");
}
