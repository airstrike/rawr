fn main() {
    let mut v = vec![1, 2];
    let r = v.last_mut().unwrap();
    *r = 9;
    let t = (String::from("a"), String::from("b"));
    let b = t.1;
    let s = Some(String::from("c"));
    let u = s.unwrap();
    let w = {
        let k = 1;
        u
    };
    for __i in 0..2 {
        if __c() {
            __take(v);
        }
    }
    println!("{:?} {} {} {}", v, t.0, b, w);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
