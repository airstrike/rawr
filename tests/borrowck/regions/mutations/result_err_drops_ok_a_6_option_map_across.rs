fn main() {
    let mut a = 1;
    let e = String::from("e");
    let r: Result<&i32, &String> = Ok(&a);
    let o = r.err();
    let __k = Some(0).map(|_| &a);
    a += 1;
    __see(&__k);
    println!("{o:?} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
