fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = a.len() > 0;
    let r = if c { &a } else { &b };
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    b.push(3);
    __see(&__k);
    println!("{}", r.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
