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
    println!("{:?}", o);
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
