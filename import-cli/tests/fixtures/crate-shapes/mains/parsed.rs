// rawr
use natives::form::settings;

fn main() {
    let text = String::from("1, 2, 3");
    let parsed: Vec<_> = settings(&text).collect();
    println!("{parsed:?}");
    let mut each = settings("4, 5");
    println!("{:?}", each.next());
    for setting in settings(&format!("{}, 7", 6)) {
        println!("{setting:?}");
    }
}
