struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

struct Both<'a, 'b> {
    g: Guard<'a>,
    r: &'b i32,
}

fn main() {
    let v = vec![];
    let mut n = 1;
    let b = Both { g: Guard(&v), r: &n };
    n += 1;
    println!("{}", n);
}
