fn main() {
    let t = ((String::from("ab"), String::from("cde")), String::from("f"));
    let ((a, _), _) = t;
    println!("{} {} {}", a, t.0.1, t.1);
}
