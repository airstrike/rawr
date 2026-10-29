// rawr
use events::widgets::{column, Framed, Widget};

#[derive(Default)]
struct State;

#[derive(Debug, Clone, Copy)]
enum Message {}

struct Label;

impl<M, T, R> Widget<M, T, R> for Label {
    fn labels(&self, _labels: &mut Vec<String>) {
        println!("script widget");
    }

    fn press(&self, _label: &str) -> Option<M> {
        None
    }
}

impl State {
    fn update(&mut self, message: Message) {
        match message {}
    }

    fn view(&self) -> impl Widget<Message> {
        column![events::framed!(Label)]
    }
}

fn main() {
    events::widgets::run(State::update, State::view);
}
