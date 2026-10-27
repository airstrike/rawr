fn main() {
    let a = 1;
    let mut v = vec![1, 2];
    let w: Vec<&i32> = v.iter().map(|_| &a).collect();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.push(3);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
