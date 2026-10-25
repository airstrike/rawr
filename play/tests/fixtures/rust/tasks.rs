// Two tasks talk over a channel. `?play` runs this on a simulated clock.
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant};

#[tokio::main]
async fn main() {
    let start = Instant::now();
    let (sender, mut receiver) = mpsc::channel(4);
    let producer = async move {
        for job in ["fetch", "parse", "render"] {
            sleep(Duration::from_millis(100)).await;
            sender.send(job).await.unwrap();
        }
    };
    let consumer = async move {
        let mut done = 0;
        while let Some(job) = receiver.recv().await {
            println!("{job} at {} ms", start.elapsed().as_millis());
            done += 1;
        }
        done
    };
    let (_, done) = tokio::join!(producer, consumer);
    println!("{done} jobs in {} ms", start.elapsed().as_millis());
}
