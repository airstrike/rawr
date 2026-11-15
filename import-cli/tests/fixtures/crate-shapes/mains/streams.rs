// rawr
use natives::streams::Names;

fn main() {
    let names = Names(vec!["a".to_string(), "b".to_string()]);
    println!("{}", names.len());
}
