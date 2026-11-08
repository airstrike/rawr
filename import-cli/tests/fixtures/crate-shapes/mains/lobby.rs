// rawr
use natives::simulation::{self, Building, Command, Picture, Simulation};

struct Lobby {
    calls: u32,
}

enum Message {
    Count,
    Report,
    Climb(usize),
}

impl Lobby {
    fn update(&mut self, message: Message, building: &Building) {
        self.calls += 1;
        let line = match message {
            Message::Count => format!("{} floors in the {building}", building.floors()),
            Message::Report | Message::Climb(_) => simulation::summary(building),
        };
        let four = building == &Building::new(4);
        println!("{} {line} {building:?} {four}", self.calls);
    }

    /// What it answers is a command made during the call, which borrows
    /// nothing of the building it was lent.
    fn advance(&mut self, message: Message, building: &Building) -> Command<Message> {
        self.calls += 1;
        match message {
            Message::Climb(left) if left > 0 => {
                println!(
                    "{} at {} floors, {left} to go",
                    self.calls,
                    building.floors()
                );
                Command::done(left - 1).map(Message::Climb)
            }
            Message::Climb(_) => Command::none(),
            Message::Count | Message::Report => Command::done(Message::Climb(1)),
        }
    }

    fn view(&self, building: &Building) -> Picture<'_, Message> {
        simulation::picture(building)
    }
}

fn main() {
    let mut lobby = Simulation::new(Lobby { calls: 0 }, 3);
    lobby.step(Message::Count, Lobby::update);
    lobby.step(Message::Report, Lobby::update);
    println!("{}", lobby.draw(Lobby::view));
    lobby.step(Message::Count, Lobby::update);
    println!("{}", lobby.draw(Lobby::view));
    println!("delivered {}", lobby.run(Message::Climb(3), Lobby::advance));
    let mut running = lobby.keep(Lobby::advance);
    println!("delivered {}", running.deliver(Message::Climb(2)));
    println!("delivered {}", running.deliver(Message::Report));
}
