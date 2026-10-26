fn main() {
    let mut s = String::from("a");
    let r = &mut s;
    let s = String::from("x");
    r.push('b');
    let s = s + "y";
    r.push('c');
    println!("{s} {r}");
}
