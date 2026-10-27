fn main() {
    let mut a = 1;
    let v = vec![1, 2];
    let w: Vec<&i32> = v.iter().map(|_| &a).collect();
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
