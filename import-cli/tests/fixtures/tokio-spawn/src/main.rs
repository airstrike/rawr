// rawr
// D527's falsifying program: a spawned script future runs though nobody
// awaits it, a spawned one gives its value to `.await`, and an unawaited
// `spawn_blocking` runs too, as under `cargo run`.
use std::time::Duration;

#[tokio::main]
async fn main() {
    tokio::spawn(async { println!("ran") });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let handle = tokio::spawn(async { 6 * 7 });
    let value = handle.await.unwrap();
    println!("joined {value}");
    let _blocking = tokio::task::spawn_blocking(|| println!("blocking ran"));
    tokio::time::sleep(Duration::from_millis(50)).await;
}
