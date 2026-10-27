fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().filter(|x| **x > 3).collect();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.clear();
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
