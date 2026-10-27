struct Inner {
    state: Option<Vec<i64>>,
    hits: i64,
}

struct Outer {
    inner: Inner,
    grid: Vec<Inner>,
}

impl Outer {
    fn touch(&mut self, n: i64) {
        if let Some(list) = &mut self.inner.state {
            list.push(n);
        }
        self.inner.hits += 1;
        match &mut self.grid[1].state {
            Some(list) => list.push(n * 10),
            None => self.grid[1].state = Some(vec![n]),
        }
    }
}

fn main() {
    let mut o = Outer {
        inner: Inner { state: Some(Vec::new()), hits: 0 },
        grid: vec![Inner { state: None, hits: 0 }, Inner { state: None, hits: 0 }],
    };
    for n in 1..4 {
        o.touch(n);
    }
    println!("{:?} {}", o.inner.state, o.inner.hits);
    println!("{:?} {:?}", o.grid[0].state, o.grid[1].state);
}
