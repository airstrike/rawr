fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().collect();
    a += 1;
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    println!("{w:?} {a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
