struct W<'a> {
    r: &'a i32,
}

fn get(w: W) -> &i32 {
    w.r
}

fn word(s: &str, n: usize) -> Option<&str> {
    s.split(' ').nth(n)
}

fn named<'a>(x: &'a str, _y: &str) -> &'a str {
    x
}

struct C {
    v: Vec<i32>,
}

impl C {
    fn push_get(&mut self, x: &i32) -> &i32 {
        self.v.push(*x);
        &self.v[0]
    }
}

fn main() {
    let x = 4;
    let mut c = C { v: Vec::new() };
    println!("{} {:?} {} {}", get(W { r: &x }), word("a b", 1), named("n", "m"), c.push_get(&x));
}
