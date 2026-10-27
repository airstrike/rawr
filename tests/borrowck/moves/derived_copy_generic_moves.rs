use std::marker::PhantomData;

#[derive(Clone, Copy)]
struct Addr<T> {
    n: u64,
    p: PhantomData<T>,
}

fn take<T>(a: Addr<T>) -> u64 {
    a.n
}

fn twice<T>(a: Addr<T>) -> u64 {
    take(a) + take(a)
}

fn main() {
    println!("{}", twice::<String>(Addr { n: 3, p: PhantomData }));
}
