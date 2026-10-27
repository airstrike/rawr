fn main() {
    let v = vec![String::from("ab")];
    let r = &v;
    let w = *r;
    println!("{:?}", w);
}
