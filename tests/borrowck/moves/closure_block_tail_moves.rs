fn main() {
    let x = String::from("ab");
    let c = || {
        let n = 1;
        x
    };
    println!("{}", x);
    println!("{}", c());
}
