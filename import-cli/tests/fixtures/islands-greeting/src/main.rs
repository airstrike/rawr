//! Native `main`, as inboard's workspace: it makes the marked home screen,
//! prints its greeting and its recent list, gives it messages, and polls
//! the tasks its wizard gives back.
mod home;
mod registry;

use home::Message;

fn main() {
    let mut state = home::State::new();
    println!("{}", state.greeting("Ada"));
    let mut registry = registry::Registry::default();
    println!("{}", state.sidebar(&registry).describe());
    registry.add("notes", 40, false, false);
    registry.add("old", 90, true, false);
    registry.add("team", 70, false, true);
    registry.add("drafts", 10, false, false);
    println!("{}", state.sidebar(&registry).describe());
    for message in [
        Message::Submit,
        Message::InputChanged(ui::Edit::Insert(' ')),
        Message::InputChanged(ui::Edit::Insert('h')),
        Message::InputResized(4),
        Message::FocusInput(None),
        Message::Submit,
        Message::Submit,
        Message::OpenProject(registry::Id(2)),
        Message::LeaveProject(registry::Id(3)),
        Message::NoOp,
        Message::Wizard(home::wizard::Message::Step(1)),
    ] {
        let action = state.update(message);
        println!("{:?} and {} tasks", action.instruction, action.task.len());
    }
    let sources = vec!["sales".to_owned(), "costs".to_owned()];
    let mut pending = state
        .edit_tile(
            home::wizard::Infolet::new("revenue"),
            home::wizard::View::new(&sources),
        )
        .poll();
    while let Some(message) = pending.pop() {
        let action = state.update(message);
        println!("{:?}", action.instruction);
        pending.extend(action.task.poll());
    }
    state.open_wizard();
    let action = state.update(Message::Wizard(home::wizard::Message::Step(3)));
    println!("{:?} and {} tasks", action.instruction, action.task.len());
}
