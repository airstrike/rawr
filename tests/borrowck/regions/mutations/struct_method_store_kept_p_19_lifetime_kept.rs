struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

impl<'a, 'b> Pair<'a, 'b> {
    fn set_y(&mut self, y: &'b i32) {
        self.y = y;
    }
}

fn main() {
    let a = 1;
    let b = 2;
    let mut c = 3;
    let mut p = Pair { x: &a, y: &b };
    p.set_y(&c);
    c += 1;
    let __u = 0u8;
    let __p = __Two(&p, &__u);
    let __k = __p.0;
    println!("{}", p.y);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
