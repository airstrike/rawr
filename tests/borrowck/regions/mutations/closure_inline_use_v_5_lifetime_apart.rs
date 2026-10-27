fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.push(doubled.len() as i64);
    __see(&__k);
    println!("{:?} {:?}", v, doubled);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
