struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

impl<'a, 'b> Pair<'a, 'b> {
    fn first(&self) -> &'a i32 {
        self.x
    }
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = Pair { x: &a, y: &b };
    let x = p.first();
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    b += 1;
    println!("{x} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
