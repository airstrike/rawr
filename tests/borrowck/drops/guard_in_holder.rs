struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

struct Holder<'a> {
    g: Guard<'a>,
}

fn main() {
    let mut v = vec![];
    let h = Holder { g: Guard(&v) };
    v.push(1);
}
