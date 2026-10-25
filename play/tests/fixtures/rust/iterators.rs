// Closures and iterator chains: lazy until something consumes them.
fn main() {
    let squares: Vec<u32> = (1..=10).map(|n| n * n).collect();
    println!("squares: {squares:?}");

    let even_sum: u32 = squares.iter().filter(|&&n| n % 2 == 0).sum();
    println!("even sum: {even_sum}");

    let limit = 30;
    let small = squares.iter().take_while(|&&n| n < limit).count(); // captures `limit`
    println!("under {limit}: {small}");

    let words = ["apple", "banana", "cherry"];
    let line = words
        .iter()
        .enumerate()
        .map(|(i, w)| format!("{}:{}", i + 1, w.len()))
        .collect::<Vec<_>>()
        .join(" ");
    println!("lengths: {line}");

    let mut counter = 0;
    let mut tick = || {
        counter += 3;
        counter
    };
    tick();
    tick();
    println!("ticked to {}", tick());
}
