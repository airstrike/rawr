// rawr
use events::matched::{self, Outcome, Wrapped};

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
        Outcome::Failed { .. } => matched::state(outcome).to_string(),
        Outcome::Idle => "idle".to_string(),
    }
}

fn checked(result: Result<u32, Outcome>) -> String {
    match result {
        Ok(count) => format!("ok {count}"),
        Err(Outcome::Opened(door)) => door.close(),
        Err(other) => matched::state(&other).to_string(),
    }
}

fn nested(wrapped: Wrapped) -> String {
    match wrapped {
        Wrapped::Held(Outcome::Opened(door), label) => format!("{label} {}", door.close()),
        Wrapped::Held(Outcome::Failed { attempts, .. }, label) => {
            format!("{label} {attempts} attempts")
        }
        Wrapped::Held(Outcome::Idle, label) => label,
        Wrapped::Counted { count, .. } => format!("{count} counted"),
    }
}

fn peeked(wrapped: &Wrapped) -> String {
    match wrapped {
        Wrapped::Held(Outcome::Opened(door), label) => {
            format!("{label} {}", matched::describe(door))
        }
        Wrapped::Held(other, label) => format!("{label} {}", matched::state(other)),
        Wrapped::Counted { door, count } => format!("{} {count}", door.name()),
    }
}

fn attempts(outcome: &Outcome) -> u32 {
    match outcome {
        Outcome::Failed { attempts, .. } => *attempts,
        _ => 0,
    }
}

fn renamed(outcome: &mut Outcome, name: &str) {
    if let Outcome::Opened(door) = outcome {
        door.rename(name);
    }
}

fn main() {
    println!("{}", checked(Ok(4)));
    println!("{}", checked(Err(matched::open("vent"))));
    println!("{}", checked(Err(matched::fail(2))));
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
    println!("{}", by_value(Outcome::Idle));
    println!(
        "{}",
        nested(matched::held(matched::open("cellar"), "below"))
    );
    println!("{}", nested(matched::held(matched::fail(4), "tried")));
    println!("{}", nested(matched::held(Outcome::Idle, "quiet")));
    println!("{}", nested(matched::counted("shed", 2)));
    println!(
        "{}",
        peeked(&matched::held(matched::open("attic"), "above"))
    );
    println!("{}", peeked(&matched::held(matched::fail(1), "again")));
    println!("{}", peeked(&matched::counted("barn", 5)));
    println!("{}", attempts(&matched::fail(6)));
    if let Outcome::Failed { attempts, .. } = matched::fail(7) {
        println!("{attempts} by value");
    }
    let mut porch = matched::open("porch");
    renamed(&mut porch, "deck");
    println!("{}", by_reference(&porch));
    match porch {
        Outcome::Opened(ref mut door) => door.rename("stoop"),
        _ => {}
    }
    println!("{}", by_reference(&porch));
    match &mut porch {
        Outcome::Opened(door) => door.rename("terrace"),
        _ => {}
    }
    println!("{}", by_value(porch));
    let mut shed = matched::counted("shed", 3);
    if let Wrapped::Counted { door, .. } = &mut shed {
        door.rename("barn");
    }
    println!("{}", peeked(&shed));
}
