fn main() {
    let mut a = 1;
    let e = String::from("e");
    let r: Result<&i32, &String> = Ok(&a);
    let n = r.map_err(|s| s.len());
    a += 1;
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    println!("{n:?} {a} {e}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
