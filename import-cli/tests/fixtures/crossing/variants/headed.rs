// rawr
// A native generic call that takes a program value stages its argument
// before the call. In the head of a `for`, an `if`, a `match` and a
// `while`, that staging is part of the head, as a headless run of iced's
// download_progress loops over a task's outputs.

mod download;

#[derive(Debug)]
enum Message {
    Add,
    Download(usize),
}

fn main() {
    for message in download::wrapped(Message::Download(3)) {
        println!("for {message:?}");
    }
    if download::wrapped(Message::Add).len() == 1 {
        println!("if one");
    }
    match download::wrapped(Message::Add).len() {
        1 => println!("match one"),
        _ => println!("match other"),
    }
    let mut rounds = 0;
    while download::wrapped(Message::Add).len() > rounds {
        rounds += 1;
    }
    println!("while {rounds}");
}
