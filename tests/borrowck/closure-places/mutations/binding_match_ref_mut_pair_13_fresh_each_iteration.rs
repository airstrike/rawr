fn main() {
    let mut pair = (vec![1], 0);
    match pair {
        (ref mut items, ref mut count) => {
            let mut add = |x| {
                items.push(x);
                *count += 1;
            };
            add(5);
            add(6);
        }
    }
    for __i in 0..2 {
        let __b = &mut pair;
        __touch(__b);
        println!("{pair:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
