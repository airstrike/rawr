//! `std::thread::spawn`, `Mutex` and `std::sync::mpsc` under `rawr run`
//! (#901): each spawned closure runs on a thread of its own, in parallel,
//! and the program prints what its rustc build prints.

use std::fs;

use super::{assert_success, stderr, stdout, Sandbox};

/// Threads that share a counter through `Arc<Mutex<_>>`, give values back
/// through `join`, and talk through channels, whose receiving loops end
/// when every sender is gone. Each line is what rustc's build prints.
#[test]
fn threads_lock_join_and_send_as_rustc_builds_them() {
    let sandbox = Sandbox::new("threads");
    fs::write(
        sandbox.work().join("main.rs"),
        r#"use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let counter = Arc::new(Mutex::new(0));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..500 {
                *counter.lock().unwrap() += 1;
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    println!("{}", *counter.lock().unwrap());
    {
        let mut guard = counter.lock().unwrap();
        *guard += 5;
    }
    println!("{:?}", counter);

    let squares: Vec<_> = (1..=4).map(|n| thread::spawn(move || n * n)).collect();
    let total: i32 = squares.into_iter().map(|h| h.join().unwrap()).sum();
    println!("{total}");

    let (tx, rx) = mpsc::channel();
    for id in 0..3 {
        let tx = tx.clone();
        thread::spawn(move || {
            for step in 0..2 {
                tx.send(id * 10 + step).unwrap();
            }
        });
    }
    drop(tx);
    let mut received: Vec<i32> = rx.iter().collect();
    received.sort();
    println!("{received:?} {:?}", rx.recv());

    let (tx, rx) = mpsc::channel::<String>();
    drop(rx);
    println!("{:?}", tx.send(String::from("lost")));

    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut sum = 0;
        for value in rx {
            sum += value;
        }
        sum
    });
    for value in [3, 4, 5] {
        tx.send(value).unwrap();
    }
    drop(tx);
    println!("{}", worker.join().unwrap());

    let failed = thread::spawn(move || {
        if total > 0 {
            panic!("boom");
        }
    })
    .join();
    println!("{:?}", failed);
}
"#,
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "main.rs"]);

    assert_success(&run);
    assert_eq!(
        stdout(&run),
        "4000\nMutex { data: 4005, poisoned: false, .. }\n30\n\
         [0, 1, 10, 11, 20, 21] Err(RecvError)\nErr(SendError { .. })\n12\nErr(Any { .. })\n",
        "{}",
        stderr(&run)
    );
    assert!(stderr(&run).contains("boom"), "{}", stderr(&run));
}

/// A program exits when `main` returns, ending the threads it left
/// running, as a Rust program does.
#[test]
fn main_returning_ends_the_threads_left_running() {
    let sandbox = Sandbox::new("threads-exit");
    fs::write(
        sandbox.work().join("main.rs"),
        "use std::thread;\nuse std::time::Duration;\n\nfn main() {\n    thread::spawn(|| loop {\n        \
         thread::sleep(Duration::from_millis(10));\n    });\n    println!(\"done\");\n}\n",
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "main.rs"]);

    assert_success(&run);
    assert_eq!(stdout(&run), "done\n");
}

/// A closure that borrows a local may outlive it: `spawn`'s `'static`
/// bound is rustc's E0373.
#[test]
fn spawning_a_closure_that_borrows_a_local_is_rustcs_e0373() {
    let sandbox = Sandbox::new("threads-static");
    fs::write(
        sandbox.work().join("main.rs"),
        "use std::thread;\n\nfn main() {\n    let total = 5;\n    \
         let handle = thread::spawn(|| total + 1);\n    println!(\"{}\", handle.join().unwrap());\n}\n",
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "main.rs"]);

    assert!(!run.status.success());
    assert!(
        stderr(&run).starts_with(
            "error[E0373]: closure may outlive the current function, but it borrows `total`, \
             which is owned by the current function"
        ),
        "{}",
        stderr(&run)
    );
}

/// A closure that captures an `Rc` isn't `Send`: rustc's E0277 at the
/// closure, which `spawn`'s bound requires.
#[test]
fn spawning_a_closure_that_captures_an_rc_is_rustcs_e0277() {
    let sandbox = Sandbox::new("threads-send");
    fs::write(
        sandbox.work().join("main.rs"),
        "use std::rc::Rc;\nuse std::thread;\n\nfn main() {\n    let shared = Rc::new(5);\n    \
         let handle = thread::spawn(move || *shared + 1);\n    println!(\"{}\", handle.join().unwrap());\n}\n",
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "main.rs"]);

    assert!(!run.status.success());
    assert!(
        stderr(&run).starts_with("error[E0277]: `Rc<")
            && stderr(&run).contains("cannot be sent between threads safely"),
        "{}",
        stderr(&run)
    );
}
