fn main() {
    let mut a = vec![0];
    let mut b = vec![0];
    let mut c = vec![0];
    let mut prev = &mut a;
    let mut next = &mut b;
    prev.push(1);
    next.push(2);
    prev = next;
    next = &mut c;
    prev.push(3);
    next.push(4);
    println!("{a:?} {b:?} {c:?}");
}
