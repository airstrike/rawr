fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut c = vec![3];
    for n in [Some(1), None, Some(2)] {
        let mut r = &mut a;
        match n {
            Some(1) => r = &mut b,
            Some(_) => r = &mut c,
            None => {}
        }
        r.push(10);
    }
    println!("{a:?} {b:?} {c:?}");
}
