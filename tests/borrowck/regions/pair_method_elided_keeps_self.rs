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
    a += 1;
    println!("{y}");
}
