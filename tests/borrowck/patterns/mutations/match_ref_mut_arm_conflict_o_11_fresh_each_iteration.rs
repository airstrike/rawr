fn main() {
    let mut o = Some(vec![1]);
    match o {
        Some(ref mut x) => {
            let n = o.is_some();
            x.push(2);
            println!("{}", n);
        }
        None => {}
    }
    for __i in 0..2 {
        let __b = &mut o;
        __touch(__b);
        println!("{:?}", o);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
