fn route(first: &mut Vec<i32>, second: &mut Vec<i32>, n: i32) {
    let mut r = first;
    if n > 0 {
        r = second;
    }
    r.push(n);
}

fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    route(&mut a, &mut b, 0);
    route(&mut a, &mut b, 7);
    println!("{a:?} {b:?}");
}
