struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

impl<'a, 'b> Pair<'a, 'b> {
    fn second(&self) -> &i32 {
        self.y
    }
}

fn main() {
    let mut a = 1;
    let b = 2;
    let p = Pair { x: &a, y: &b };
    let y = p.second();
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{y}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
