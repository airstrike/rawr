//! Native: the app's workspace, as inboard's: it keeps the marked home
//! screen's `State` between calls, gives it messages, polls the tasks its
//! `update` gives and feeds their messages back.
pub mod home;
pub mod inbox;

pub struct Workspace {
    pub home: home::State,
    pub inbox: inbox::State,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            home: home::State::new(),
            inbox: inbox::State::default(),
        }
    }

    /// Give `message` to the home screen, and every message its tasks give
    /// after it, as iced's runtime does. How many messages it took.
    pub fn update(&mut self, message: home::Message) -> usize {
        let mut pending = vec![message];
        let mut taken = 0;
        while let Some(message) = pending.pop() {
            taken += 1;
            let task = home::update(&mut self.home, message, &mut self.inbox);
            pending.extend(task.poll());
        }
        taken
    }

    pub fn press(&mut self) -> usize {
        let element = home::view(&self.home);
        println!("view {}", element.label);
        match element.on_press {
            Some(message) => self.update(message),
            None => 0,
        }
    }
}
