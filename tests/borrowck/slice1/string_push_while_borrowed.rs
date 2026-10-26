fn main() {
    let mut s = String::from("a");
    let r = &s;
    s.push('b');
    println!("{r}");
}
