// rawr
use natives::simulation::{self, Building, Picture, Simulation};

struct Lobby {
    calls: u32,
}

enum Message {
    Count,
    Report,
}

impl Lobby {
    fn update(&mut self, message: Message, building: &Building) {
        self.calls += 1;
        let line = match message {
            Message::Count => format!("{} floors in the {building}", building.floors()),
            Message::Report => simulation::summary(building),
        };
        let four = building == &Building::new(4);
        println!("{} {line} {building:?} {four}", self.calls);
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
}
