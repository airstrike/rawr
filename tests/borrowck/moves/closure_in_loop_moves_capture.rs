fn main() {
    for i in 0..2 {
        let s = format!("{}", i);
        let c = move || {
            let t = s;
            t.len()
        };
        println!("{}", c());
    }
}
