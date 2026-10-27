fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().collect();
    a += 1;
    let __k = Some(0).map(|_| &a);
    println!("{w:?} {a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
