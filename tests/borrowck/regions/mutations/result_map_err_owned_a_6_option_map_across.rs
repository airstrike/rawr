fn main() {
    let mut a = 1;
    let e = String::from("e");
    let r: Result<&i32, &String> = Ok(&a);
    let n = r.map_err(|s| s.len());
    let __k = Some(0).map(|_| &a);
    a += 1;
    __see(&__k);
    println!("{n:?} {a} {e}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
