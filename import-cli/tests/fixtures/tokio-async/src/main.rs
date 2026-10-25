// rawr
// An async `main` on the project's own tokio (D513): tokio's real sleep, an
// mpsc channel of a script struct, `join!` and `spawn_blocking` of a script
// closure. `cargo run` prints the same.
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

#[derive(Debug, Clone)]
struct Job {
    id: u32,
    name: String,
}

fn count_primes(limit: u32) -> u32 {
    let mut count = 0;
    for n in 2..limit {
        let mut prime = true;
        let mut d = 2;
        while d * d <= n {
            if n % d == 0 {
                prime = false;
                break;
            }
            d += 1;
        }
        if prime {
            count += 1;
        }
    }
    count
}

fn millis(start: Instant) -> u64 {
    (start.elapsed().as_secs_f64() * 1000.0) as u64
}

#[tokio::main]
async fn main() {
    let start = Instant::now();
    let (sender, mut receiver) = mpsc::channel::<Job>(1);
    let producer = async move {
        for id in 1..=3 {
            sleep(Duration::from_millis(100)).await;
            let name = format!("job {id}");
            sender.send(Job { id, name }).await.unwrap();
        }
    };
    let consumer = async move {
        let mut total = 0;
        while let Some(job) = receiver.recv().await {
            println!("{} at {} ms: {:?}", job.name, millis(start), job);
            total += job.id;
        }
        total
    };
    let (_, total) = tokio::join!(producer, consumer);
    let primes = tokio::task::spawn_blocking(move || count_primes(2_000))
        .await
        .unwrap();
    println!("total {total}, {primes} primes");
}
