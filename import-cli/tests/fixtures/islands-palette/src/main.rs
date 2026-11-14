//! Native `main`, as inboard's: it makes the marked themes and draws the
//! marked home screen's elements. With `ISLANDS_STOP` set, it then draws
//! the subtitle at each tick until that file is there, as a running
//! program does.
mod home;
mod theme;

pub type Element<'a, Message> = ui::Element<'a, Message, theme::Theme>;

fn main() {
    let alarmed = theme::Theme::alarmed();
    println!("alarmed {alarmed:?}");
    let (accent, base) = theme::Theme::split();
    println!("split {accent:?} {base:?}");
    let updated = theme::Theme::updated();
    println!("updated {updated:?}");
    let theme = theme::Theme::default();
    println!("title {}", home::title("Inbox").describe(&theme));
    println!("subtitle {}", home::subtitle("Mail").describe(&theme));
    let _ = home::Message::Opened;
    if let Some(stop) = std::env::var_os("ISLANDS_STOP") {
        let mut tick = 0;
        while !std::path::Path::new(&stop).exists() {
            tick += 1;
            println!("tick {tick} {}", home::subtitle("Mail").describe(&theme));
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
