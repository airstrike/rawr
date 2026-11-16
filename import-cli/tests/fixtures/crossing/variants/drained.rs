// rawr
// A driver generic over a type, at the opaque type a function's
// `Wrapper<impl SomeTrait>` holds, drained in a loop: the pipe a generic
// function makes from nothing carries signals of that wrapper, which only
// the driver's call that takes it shows, and each signal is matched and
// its step handed back to the driver.

use events::piped::{self, Driver, Signal};

fn main() {
    let stepper = piped::wrapped(3);
    let (pipe, mut drain) = piped::pipe();
    let mut driver = Driver::with_start(pipe, &stepper, None);
    let mut words = ["go", "go", "stay"].iter();
    let mut told = Vec::new();
    loop {
        let Ok(signal) = drain.try_recv() else {
            break;
        };
        match signal {
            Signal::Moved(moved) => driver.perform(&stepper, moved),
            Signal::Failed(word) => told.push(format!("failed {word}")),
            Signal::Took(step) => told.push(format!("took {step}")),
            Signal::Ready => match words.next() {
                Some(word) => driver.run(&stepper, word),
                None => break,
            },
        }
    }
    println!("told {}", told.join(", "));
    println!("count {}", driver.count());
}
