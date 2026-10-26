fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut r = &mut a;
    r.push('1');
    let view = &b;
    r = &mut b;
    r.push('2');
    println!("{view} {a} {b}");
}
