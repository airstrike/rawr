// rawr
use own_library::Floor;

struct Elevator {
    floor: Floor,
    trips: i64,
}

enum Message {
    Up,
    Down,
}

fn update(elevator: &mut Elevator, message: Message) {
    elevator.floor = match message {
        Message::Up => elevator.floor.above(),
        Message::Down => elevator.floor.below(),
    };
    elevator.trips += 1;
}

fn view(elevator: &Elevator) -> String {
    format!("{} after {} trips", elevator.floor.label(), elevator.trips)
}

fn main() {
    let start = Elevator {
        floor: Floor::new(0),
        trips: 0,
    };
    let messages = vec![Message::Up, Message::Up, Message::Down, Message::Up];
    println!("{}", own_library::run(start, messages, update, view));
    println!(
        "{}",
        own_library::Floor::new(3) == Floor::new(3).above().below()
    );
}
