// rawr
// An async `main` that runs until it is stopped, on a runtime its attribute
// configures as written: a save restarts it with the new code.
use std::time::Duration;

const VERSION: i64 = 1;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut tick = 0;
    loop {
        tick += 1;
        println!("version {} tick {tick}", VERSION);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
