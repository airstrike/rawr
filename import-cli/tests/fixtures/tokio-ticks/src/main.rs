// rawr
// An async `main` that runs until it is stopped, on a runtime its attribute
// configures as written: a save to a function it calls is taken where its
// loop resumes, and an edit to `main` restarts it.
use std::time::Duration;

fn version() -> i64 {
    1
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut tick = 0;
    loop {
        tick += 1;
        println!("version {} tick {tick}", version());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
