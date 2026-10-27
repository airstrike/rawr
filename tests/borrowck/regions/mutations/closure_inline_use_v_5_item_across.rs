fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.push(doubled.len() as i64);
    __see(&__k);
    println!("{:?} {:?}", v, doubled);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
