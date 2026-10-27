fn main() {
    let n = 3;
    let m = {
        let k = 1;
        n
    };
    println!("{} {}", m, n + k_of());
}

fn k_of() -> i32 {
    1
}
