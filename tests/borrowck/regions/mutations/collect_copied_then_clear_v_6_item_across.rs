fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().collect();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.clear();
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
