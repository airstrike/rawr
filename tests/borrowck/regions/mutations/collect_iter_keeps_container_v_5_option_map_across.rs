fn main() {
    let a = 3;
    let mut v = vec![&a];
    let w: Vec<&&i32> = v.iter().collect();
    let __k = Some(0).map(|_| &v);
    v.clear();
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
