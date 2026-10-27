fn main() {
    let mut a = [1, 2];
    let mut c = || a[0] += 1;
    let x = a[1];
    c();
    println!("{} {:?}", x, a);
}
