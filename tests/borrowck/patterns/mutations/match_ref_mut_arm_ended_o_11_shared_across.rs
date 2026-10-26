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
    let __s = &o;
    println!("{:?}", o);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
