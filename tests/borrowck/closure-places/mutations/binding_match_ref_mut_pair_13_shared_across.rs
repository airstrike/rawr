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
    let __s = &pair;
    println!("{pair:?}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
