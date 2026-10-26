fn route<'a>(mut r: &'a mut Vec<i32>, other: &'a mut Vec<i32>, n: i32) {
    r.push(0);
    if n > 0 {
        r = other;
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
