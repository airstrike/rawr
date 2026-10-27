use std::rc::Rc;

struct S {
    v: Vec<i32>,
}

fn main() {
    let r = Rc::new(S { v: vec![1] });
    let v = r.v.clone();
    println!("{:?} {:?}", v, r.v);
}
