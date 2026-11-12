// rawr
// The crate's `plaque<'a, Message: 'a>(..) -> impl Widget<Message> + 'a`
// called where nothing is expected of it: rustc's E0282, since only the
// output names `Message`.

fn main() {
    let _unused = events::widgets::plaque("x");
}
