fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    v.push(doubled.len() as i64);
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    println!("{:?} {:?}", v, doubled);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
