fn pick(n: i32) -> (Vec<i32>, Vec<i32>) {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    if n > 2 {
        r = &mut b;
    }
    r.push(n);
    (a, b)
}

fn main() {
    println!("{:?}", pick(1));
    println!("{:?}", pick(5));
}
