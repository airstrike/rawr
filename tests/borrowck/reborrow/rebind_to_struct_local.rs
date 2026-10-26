#[derive(Debug)]
struct Counter {
    hits: u32,
}

impl Counter {
    fn hit(&mut self) {
        self.hits += 1;
    }
}

fn main() {
    let mut left = Counter { hits: 0 };
    let mut right = Counter { hits: 0 };
    let mut r = &mut left;
    for n in 0..5 {
        r.hit();
        r.hits += n;
        if n >= 2 {
            r = &mut right;
        }
    }
    println!("{left:?} {right:?}");
}
