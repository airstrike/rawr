// rawr
use events::matched::{self, Outcome};

fn by_value(outcome: Outcome) -> String {
    match outcome {
        Outcome::Opened(door) if door.name() == "gate" => format!("gate {}", door.close()),
        Outcome::Opened(door) => door.close(),
        Outcome::Failed { error, attempts } => format!("{attempts} {}", matched::consume(error)),
        Outcome::Idle => "idle".to_string(),
    }
}

fn by_reference(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Opened(door) => format!(
            "{} {} {}",
            door.name(),
            matched::describe(door),
            matched::state(outcome)
        ),
        Outcome::Failed { error, attempts } => format!("{attempts} {}", matched::reason(error)),
        Outcome::Idle => "idle".to_string(),
    }
}

fn main() {
    let opened = matched::open("door");
    println!("{}", by_reference(&opened));
    println!("{}", by_value(opened));
    let failed = matched::fail(3);
    println!("{}", by_reference(&failed));
    println!("{}", by_value(failed));
    println!("{}", by_value(matched::open("gate")));
    if let Outcome::Opened(door) = matched::open("hatch") {
        println!("{}", door.close());
    }
    if let Outcome::Failed { error, .. } = &matched::fail(1) {
        println!("{}", matched::reason(error));
    }
    println!("{}", by_value(Outcome::Idle));
}
