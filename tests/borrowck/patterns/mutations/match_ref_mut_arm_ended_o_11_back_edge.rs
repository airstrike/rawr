fn main() {
    let mut o = Some(vec![1]);
    match o {
        Some(ref mut x) => {
            x.push(2);
            let n = o.is_some();
            println!("{}", n);
        }
        None => {}
    }
    let __b = &mut o;
    for __i in 0..2 {
        __touch(__b);
        println!("{:?}", o);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
