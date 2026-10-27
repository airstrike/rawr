use std::marker::PhantomData;

#[derive(Clone, Copy)]
struct Addr<T> {
    n: u64,
    p: PhantomData<T>,
}

fn take<T>(a: Addr<T>) -> u64 {
    a.n
}

fn main() {
    let a: Addr<String> = Addr { n: 3, p: PhantomData };
    println!("{}", take(a) + take(a));
}
