fn main() {
    let a = 3;
    let mut v = vec![&a];
    let w: Vec<&&i32> = v.iter().collect();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.clear();
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
