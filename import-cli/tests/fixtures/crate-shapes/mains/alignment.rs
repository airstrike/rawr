// rawr
use natives::alignment;
use natives::Alignment::End;
use natives::{Alignment, Center};

fn name(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::Start => "start",
        Center => "center",
        End => "end",
    }
}

fn main() {
    println!("{}", alignment::describe(Center));
    println!("{} {} {}", name(Center), name(End), name(Alignment::Start));
    println!("{}", Center == Alignment::Center);
    let middle = Center;
    println!("{}", matches!(middle, Center));
}
