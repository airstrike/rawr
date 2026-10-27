fn main() {
    let mut v = vec![1, 2];
    let mut o: Option<usize> = None;
    let n = *o.get_or_insert_with(|| v.len());
    v.push(3);
    println!("{} {:?} {:?}", n, o, v);
}
