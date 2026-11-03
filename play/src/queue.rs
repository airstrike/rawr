//! The snippets waiting for the bot's runs, and the deadline each running
//! one gets from them (see [`Budget`]): none while nothing waits, and the
//! normal budget from the moment something does.

use std::fmt;
use std::future::Future;
use std::time::{Duration, Instant, SystemTime};

use rawr::play::deadline;
use tokio::sync::watch;

use crate::budget::Budget;

/// The count of snippets waiting to run, and the budget runs get.
#[derive(Debug)]
pub struct Queue {
    budget: Budget,
    waiting: watch::Sender<usize>,
}

/// A snippet counted as waiting until dropped.
#[derive(Debug)]
pub struct Waiting<'a>(&'a watch::Sender<usize>);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

/// A run killed at its deadline, after this long.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Killed(Duration);

impl Killed {
    /// How long the run went on.
    #[must_use]
    pub fn after(self) -> Duration {
        self.0
    }
}

impl fmt::Display for Killed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the run timed out after {} ms",
            self.0.as_millis()
        )
    }
}

impl Queue {
    /// An empty queue whose runs get `budget`.
    #[must_use]
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            waiting: watch::Sender::new(0),
        }
    }

    /// The budget its runs get.
    #[must_use]
    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    /// Count a snippet as waiting until the result drops, which is when it
    /// starts or gives up.
    pub fn wait(&self) -> Waiting<'_> {
        self.waiting.send_modify(|count| *count += 1);
        Waiting(&self.waiting)
    }

    /// Drive `run`, a run whose processes read `deadline`. It has no
    /// deadline but the ceiling while no snippet waits. Once one does, its
    /// script deadline goes to `deadline` and it is killed at the budget's
    /// timeout from then.
    pub async fn run<F: Future>(
        &self,
        deadline: &deadline::File,
        run: F,
    ) -> Result<F::Output, Killed> {
        let started = Instant::now();
        let mut contended = None;
        let mut waiting = self.waiting.subscribe();
        let mut run = std::pin::pin!(run);
        loop {
            let kill = tokio::time::Instant::from_std(self.budget.kill(started, contended));
            tokio::select! {
                biased;
                output = &mut run => return Ok(output),
                () = tokio::time::sleep_until(kill) => return Err(Killed(started.elapsed())),
                _ = waiting.wait_for(|&count| count > 0), if contended.is_none() => {
                    let now = Instant::now();
                    contended = Some(now);
                    let left = self.budget.script(started, contended).saturating_duration_since(now);
                    if let Err(error) = deadline.set(SystemTime::now() + left) {
                        eprintln!("rawr-play: cannot set a run's deadline: {error}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Budget, Killed, Queue};
    use rawr::play::deadline;
    use std::time::{Duration, Instant};

    fn queue(time: u64, timeout: u64, max: u64) -> Queue {
        Queue::new(Budget::new(
            Duration::from_millis(time),
            Duration::from_millis(timeout),
            Duration::from_millis(max),
        ))
    }

    #[tokio::test]
    async fn an_idle_run_goes_past_the_budget() {
        let queue = queue(50, 100, 60_000);
        let file = deadline::File::new().unwrap();
        let ran = queue
            .run(&file, async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                7
            })
            .await;
        assert_eq!(ran, Ok(7));
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "");
    }

    #[tokio::test]
    async fn an_idle_run_is_killed_at_the_ceiling() {
        let queue = queue(50, 100, 200);
        let file = deadline::File::new().unwrap();
        let ran = queue.run(&file, std::future::pending::<()>()).await;
        let after = ran.map(drop).unwrap_err().after();
        assert!(after >= Duration::from_millis(250), "{after:?}");
    }

    #[tokio::test]
    async fn a_waiting_snippet_kills_the_run_its_timeout_later() {
        let queue = queue(50, 100, 60_000);
        let file = deadline::File::new().unwrap();
        let waiter = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            (queue.wait(), Instant::now())
        };
        let (ran, (_waiting, arrived)) =
            tokio::join!(queue.run(&file, std::future::pending::<()>()), waiter);
        let killed: Killed = ran.unwrap_err();
        assert!(killed.after() >= Duration::from_millis(300), "{killed:?}");
        assert!(arrived.elapsed() < Duration::from_secs(5));
        assert!(!std::fs::read_to_string(file.path()).unwrap().is_empty());
    }

    /// A plain program that loops runs on with nothing waiting, and stops
    /// with its text so far once a second snippet waits, at its script
    /// deadline and not at the kill.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_looping_plain_run_stops_only_once_another_snippet_waits() {
        let queue = queue(200, 60_000, 600_000);
        let file = deadline::File::new().unwrap();
        let limits = crate::Limits {
            operations: u64::MAX,
            time: queue.budget().max(),
            deadline: Some(file.path().to_owned()),
            ..crate::Limits::default()
        };
        // The second snippet arrives well after the loop began: after
        // three times what a program that does nothing takes to compile
        // and run, and 2 s more, ten times the budget.
        let warm = Instant::now();
        crate::run("fn main() {}\n", &limits).unwrap();
        let arrival = warm.elapsed() * 3 + Duration::from_secs(2);
        let source =
            "fn main() {\n    println!(\"start\");\n    let mut n = 0;\n    loop { n += 1; }\n}\n";
        let run = async {
            let ran = tokio::task::spawn_blocking(move || crate::run(source, &limits)).await;
            (ran, Instant::now())
        };
        let waiter = async {
            tokio::time::sleep(arrival).await;
            (queue.wait(), Instant::now())
        };
        let (ran, (_waiting, arrived)) = tokio::join!(queue.run(&file, run), waiter);
        let (output, ended) = ran.expect("stopped at the script deadline, not killed");
        let output = output.unwrap().unwrap();
        assert_eq!((output.status, output.limit), (1, true));
        assert_eq!(output.text(), "start\ntime limit exceeded\n");
        assert!(
            ended >= arrived + Duration::from_millis(150),
            "stopped early"
        );
    }
}
