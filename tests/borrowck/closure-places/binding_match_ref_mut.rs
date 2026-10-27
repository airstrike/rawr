fn main() {
    let mut pair = (vec![1], 0);
    match pair {
        (ref mut items, ref mut count) => {
            let mut add = |x| {
                items.push(x);
                *count += 1;
            };
            add(5);
            add(6);
        }
    }
    println!("{pair:?}");
}
