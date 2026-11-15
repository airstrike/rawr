// rawr
// A program's async functions that take `&mut` parameters (#176): each
// future holds its borrow of the caller's place across its awaits, as a
// `Vec`, a program struct and a crate's connection, whose own `&mut self`
// async method the future calls. `cargo run` prints the same.

struct Counter {
    n: u32,
}

impl Counter {
    async fn add(&mut self, by: u32) {
        tokio::task::yield_now().await;
        self.n += by;
    }
}

async fn push(items: &mut Vec<i32>) {
    items.push(4);
}

async fn bump(counter: &mut Counter) {
    counter.n += 1;
    tokio::task::yield_now().await;
    counter.n += 10;
}

async fn run(conn: &mut shelf::db::Connection, names: &mut Vec<String>) -> usize {
    conn.execute("create").await;
    names.push(String::from("create"));
    let count = conn.execute("insert").await;
    names.push(String::from("insert"));
    count
}

async fn rename(font: &mut shelf::Font) {
    shelf::rename(font, String::from("Inter")).await;
    font.family.push('!');
}

#[tokio::main]
async fn main() {
    let mut v = vec![1];
    push(&mut v).await;
    push(&mut v).await;
    println!("{v:?}");

    let mut counter = Counter { n: 0 };
    bump(&mut counter).await;
    counter.add(2).await;
    counter.add(3).await;
    println!("{}", counter.n);

    let mut conn = shelf::db::open();
    let mut names = Vec::new();
    let ran = run(&mut conn, &mut names).await;
    let again = run(&mut conn, &mut names).await;
    println!("{ran} {again} {} {names:?}", conn.count());

    let mut font = shelf::font(String::from("Georgia"), 1);
    rename(&mut font).await;
    println!("{}", font.family);
}
