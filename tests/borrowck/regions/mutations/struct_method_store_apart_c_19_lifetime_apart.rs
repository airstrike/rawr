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
    let x = p.x;
    let __u = 0u8;
    let __p = __Two(&__u, &c);
    let __k = __p.0;
    c += 1;
    __see(&__k);
    println!("{x} {c}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
