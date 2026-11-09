//! Native `main`, as inboard's: the native workspace keeps the marked home
//! screen's state, gives it messages with native payloads, polls the
//! tasks it gives back and feeds their messages in again. With
//! `ISLANDS_STOP` set, it then presses the home screen's button each tick
//! until the file it names exists.
mod workspace;

use ui::Action;
use workspace::{home, inbox};

/// The app's theme, as inboard's `crate::Theme`: the marked home screen is
/// lent it and lends it on to its open wizard's element.
#[derive(Debug)]
pub struct Theme {
    name: String,
}

impl Theme {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

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
    let opened = workspace
        .home
        .open(home::wizard::View::new(&workspace.inbox));
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
    let home = &mut workspace.home;
    println!("{}", home.layout());
    home.open_wizard();
    println!("advanced {}", home.advance_wizard());
    println!("{}", home.layout());
    for title in ["inbox", "notes", "drafts"] {
        home.push_tab(title);
    }
    home.rename_tab(1, "ideas");
    println!("{}", home.layout());
    home.close_tab(0);
    println!("{}", home.layout());
    home.open_wizard();
    println!("{}", home.layout());
    home.close_wizard();
    println!("advanced {}", home.advance_wizard());
    println!("{}", home.layout());
    home.close_tab(1);
    home.close_tab(0);
    println!("{}", home.layout());
    for title in ["left", "middle", "right"] {
        home.split(title);
    }
    home.pin(2);
    println!("panes [{}]", home.panes());
    home.close_pane(1);
    println!("panes [{}]", home.panes());
    home.close_pane(0);
    home.close_pane(0);
    println!("panes [{}]", home.panes());
    // A grid element given by value, the open wizard's element over the
    // catalog, and the inbox's top bar over its updates or its greeting.
    let shown = workspace.show_over(Some(ui::button("grid", home::wizard::Message::Step(7))));
    println!("grid shows {} {:?}", shown.label, shown.on_press);
    workspace.home.open_wizard();
    let shown = workspace.show();
    println!("wizard shows {} {:?}", shown.label, shown.on_press);
    workspace.home.close_wizard();
    workspace.inbox.unread = 0;
    let shown = workspace.show();
    println!("no updates show {} {:?}", shown.label, shown.on_press);
    workspace.updates.push(inbox::Update::new("tiles 1.2"));
    let shown = workspace.show();
    println!("updates show {} {:?}", shown.label, shown.on_press);
    // The open wizard past step 5 shows the theme's name. Its steps run
    // through the screen's `update`: it commits past step 9, and closes
    // on a catalog with nothing unread.
    workspace.home.open_wizard();
    workspace.home.advance_wizard();
    let shown = workspace.show();
    println!("themed wizard shows {} {:?}", shown.label, shown.on_press);
    workspace.sources.unread = 2;
    let taken = workspace.update(home::Message::Built(home::wizard::Message::Step(1)));
    println!("built {taken} drew {}", workspace.thumbs.drawn);
    workspace.home.open_wizard();
    workspace.sources.unread = 0;
    let taken = workspace.update(home::Message::Built(home::wizard::Message::Step(1)));
    println!(
        "built {taken} drew {} {}",
        workspace.thumbs.drawn,
        workspace.home.layout()
    );
    println!("redrew {}", workspace.home.redraw(&mut workspace.thumbs));
    workspace.inbox.unread = 3;
    // Elements over native inputs alone, two held at once, and what their
    // crate calls make read where it is made.
    let banner = home::inbox_banner(&workspace.inbox);
    let top = home::top_bar(&workspace.inbox, &workspace.updates);
    println!(
        "banner {} {:?} top {}",
        banner.label, banner.on_press, top.label
    );
    println!(
        "pending {} top label {}",
        home::pending(&workspace.inbox, &workspace.updates),
        home::top_label(&workspace.inbox, &workspace.updates)
    );
    let titled = workspace.home.titled(&workspace.inbox);
    println!(
        "titled {} label {}",
        titled.label,
        home::titled_label(&workspace.inbox, "mail")
    );
    workspace.home.open_wizard();
    workspace.home.advance_wizard();
    let catalog = home::wizard::View::new(&workspace.sources);
    println!(
        "wizard label {}",
        home::wizard_label(&workspace.home, catalog, &workspace.theme)
    );
    workspace.home.close_wizard();
    // Ids marked read from a `Vec` and from an iterator over a local's
    // names, each made from text by value and by reference.
    let mut read = home::Read::default();
    read.mark_all_read(vec![
        home::Id::new("one"),
        home::Id::new(format!("t{}", "wo")),
    ]);
    let names = ["three".to_owned(), "four".to_owned()];
    read.mark_all_read(names.iter().map(home::Id::new));
    println!("read {}", read.label());
    // An id's text, borrowed where native code holds the id: twice while
    // the first borrow is held, the same text, and never the text of
    // another id that reads the same.
    let server = home::Id::new("server-7");
    let twin = home::Id::new("server-7");
    let text = server.as_str();
    println!(
        "id {} {:?} {} {}",
        text,
        server.as_str().strip_prefix("server-"),
        std::ptr::eq(text, server.as_str()),
        std::ptr::eq(text, twin.as_str())
    );
    // Screens whose elements native code held, then moved and dropped.
    for round in 0..2 {
        let screen = home::State::new();
        println!("screen {round} shows {}", home::view(&screen).label);
        let moved = Box::new(screen);
        println!("moved shows {}", home::view(&moved).label);
    }
    let Some(stop) = std::env::var_os("ISLANDS_STOP") else {
        return;
    };
    workspace.home.push_tab("kept");
    // A banner built once and kept, as iced keeps a view's elements, beside
    // one built each tick: a save that edits its literal shows in the fresh
    // one, and the kept one still reads the text it borrowed.
    let banner = home::State::new();
    let kept = home::banner(&banner);
    for tick in 1..3000 {
        if std::path::Path::new(&stop).exists() {
            break;
        }
        let fresh = home::banner(&banner);
        // Text of every small size, made and dropped, so memory the swap
        // let go is written over before the kept banner is read.
        let noise = (1..256)
            .flat_map(|size| (0..8).map(move |_| "#".repeat(size)))
            .collect::<Vec<_>>();
        drop(noise);
        println!("banner {} kept {}", fresh.label, kept.label);
        let before = workspace.home.count();
        workspace.update(home::Message::Count);
        let after = workspace.home.count();
        let shown = workspace.show();
        println!(
            "tick {tick} from {before} by {} over [{}] shows {}",
            after - before,
            workspace.home.tabs(),
            shown.label
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
