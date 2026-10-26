fn main() {
    let mut items = vec![1, 2, 3, 4];
    let mut seen = Vec::new();
    'w: while let Some(x) = loop {
        let next = items.pop();
        if next == Some(2) {
            break 'w;
        }
        break next;
    } {
        seen.push(x);
    }
    println!("{:?} {:?}", seen, items);
}
