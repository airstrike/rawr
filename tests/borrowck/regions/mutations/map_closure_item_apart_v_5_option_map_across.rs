fn main() {
    let a = 1;
    let mut v = vec![1, 2];
    let w: Vec<&i32> = v.iter().map(|_| &a).collect();
    let __k = Some(0).map(|_| &v);
    v.push(3);
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
