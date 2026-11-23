// rawr
use own_library::{Floor, Ride};

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
    // A tuple struct's constructor by its path (#1168), a module's function
    // bound to a local (#874), and a std type the library re-exports under
    // another name, in types and in calls' paths, by its `use` and by its
    // path (#344).
    let wing = own_library::Wing('b');
    let sign = own_library::signs::door;
    let ride: Ride = Ride::from_millis(1500);
    let wait: own_library::Ride = own_library::Ride::from_secs(2);
    println!(
        "{:?} {} {} {:?} {:?}",
        wing,
        wing.0,
        sign(&Floor::new(4)),
        ride,
        wait
    );
}
