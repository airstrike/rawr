struct Inner {
    list: Vec<i64>,
}

struct Outer {
    inner: Inner,
    rows: Vec<Inner>,
}

impl Outer {
    fn bump(&mut self) {
        if let Some(x) = self.inner.list.get_mut(0) {
            *x = 5;
        }
        match self.inner.list.last_mut() {
            Some(x) => *x += 1,
            None => {}
        }
        if let Some(row) = self.rows.iter_mut().find(|row| row.list.len() > 1) {
            row.list.push(9);
        }
        if let Some(x) = self.rows[0].list.first_mut() {
            *x *= 3;
        }
    }
}

fn main() {
    let mut o = Outer {
        inner: Inner { list: vec![1, 2] },
        rows: vec![Inner { list: vec![4] }, Inner { list: vec![6, 7] }],
    };
    o.bump();
    o.bump();
    println!("{:?}", o.inner.list);
    println!("{:?} {:?}", o.rows[0].list, o.rows[1].list);
}
