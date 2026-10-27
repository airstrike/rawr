fn main() {
    let x = String::from("ab");
    let c = || {
        let n = 1;
        x.len() + n
    };
    println!("{}", x);
    println!("{}", c());
}
