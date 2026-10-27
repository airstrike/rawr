fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().collect();
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{w:?} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
