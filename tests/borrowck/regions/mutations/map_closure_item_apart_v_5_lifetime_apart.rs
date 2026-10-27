fn main() {
    let a = 1;
    let mut v = vec![1, 2];
    let w: Vec<&i32> = v.iter().map(|_| &a).collect();
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.push(3);
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
