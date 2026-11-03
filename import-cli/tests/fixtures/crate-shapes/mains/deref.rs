// rawr
use natives::deref::{shout, Labeled, Name, Scores};

fn main() {
    let mut labeled = Labeled::new("clicks", 2);
    println!("{} {} {}", labeled.label, labeled.step, labeled.count());
    labeled.bump();
    labeled.bump();
    println!("{}", labeled.count());
    let name = Name::new("ferris");
    println!("{} {} {}", name.len(), name.to_uppercase(), shout(&name));
    let scores = Scores::new(vec![3, 1, 2]);
    let total: i64 = scores.iter().sum();
    println!("{} {:?} {}", scores.len(), scores.first(), total);
}
