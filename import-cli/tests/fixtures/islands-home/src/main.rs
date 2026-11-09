//! Native `main`, as inboard's: the native workspace keeps the marked home
//! screen's state, gives it messages with native payloads, polls the
//! tasks it gives back and feeds their messages in again. With
//! `ISLANDS_STOP` set, it then presses the home screen's button each tick
//! until the file it names exists.
mod workspace;

use ui::Action;
use workspace::{home, inbox};

fn main() {
    let mut workspace = workspace::Workspace::new();
    workspace.inbox.unread = 5;
    for character in ['h', 'i'] {
        workspace.update(home::Message::Edited(Action::Insert(character)));
    }
    println!("{}", workspace.home.summary());
    // Two elements held at once, each borrowing the screen.
    let inbox = workspace.show();
    let wizard = home::view(&workspace.home);
    println!(
        "shown {} {:?} and {}",
        inbox.label, inbox.on_press, wizard.label
    );
    let taken = workspace.update(home::Message::Inbox(inbox::Message::Read(2)));
    println!("inbox {} after {taken}", workspace.inbox.unread);
    let opened = workspace.home.open();
    println!("opened {} task", opened.len());
    for message in opened.poll() {
        println!("message {message:?}");
        workspace.update(message);
    }
    println!(
        "count {} text {}",
        workspace.home.count(),
        workspace.home.text()
    );
    let shown = workspace.show();
    println!("shown {} {:?}", shown.label, shown.on_press);
    let taken = workspace.press();
    println!("pressed {taken}: {}", workspace.home.summary());
    let taken = workspace.press();
    println!("pressed {taken}: {}", workspace.home.summary());
    let Some(stop) = std::env::var_os("ISLANDS_STOP") else {
        return;
    };
    for tick in 1..3000 {
        if std::path::Path::new(&stop).exists() {
            break;
        }
        let before = workspace.home.count();
        workspace.update(home::Message::Count);
        let after = workspace.home.count();
        let shown = workspace.show();
        println!(
            "tick {tick} from {before} by {} shows {}",
            after - before,
            shown.label
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
