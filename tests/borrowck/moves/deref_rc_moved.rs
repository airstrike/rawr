use std::rc::Rc;

fn main() {
    let r = Rc::new(String::from("ab"));
    let s = *r;
    println!("{}", s);
}
