//! How late a save reaches FSEvents, beside a kqueue vnode watch on the
//! same files.
//!
//! The probe makes a package under `--dir` and registers it the way the
//! watcher does on macOS since D566.2: `src/` as a recursive tree and
//! `Cargo.toml` by name. A kqueue `EVFILT_VNODE` watch holds each saved file
//! open. Each save rewrites one file in place, alternating `src/main.rs`
//! and `Cargo.toml`, and waits until both watchers have reported it (or
//! `--cap` passes, which counts as a miss) before the next.
//!
//! The first phase runs on an idle machine. When a command follows `--`,
//! the probe then starts it in its own process group, waits `--warmup`
//! seconds, saves again while it runs, and stops its group. The probe
//! deletes `--dir` when it ends.
//!
//! ```text
//! cargo build -p rawr-watch --example save_delay
//! target/debug/examples/save_delay --saves 40 \
//!     -- cargo build -p rawr --target-dir target/save-delay/load
//! ```

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    match probe::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("save_delay: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("save_delay measures FSEvents, which only macOS has");
}

#[cfg(target_os = "macos")]
mod probe {
    use std::fs;
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use notify::Watcher;

    /// Which watcher reported a path.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Source {
        Fsevents,
        Kqueue,
    }

    /// One report of one path, stamped when the watcher's thread received it.
    struct Arrival {
        source: Source,
        path: PathBuf,
        at: Instant,
    }

    /// When each watcher first reported one save, from just before its write.
    struct Save {
        file: &'static str,
        fsevents: Option<Duration>,
        kqueue: Option<Duration>,
    }

    struct Options {
        saves: usize,
        gap: Duration,
        warmup: Duration,
        cap: Duration,
        dir: PathBuf,
        load: Vec<String>,
    }

    impl Options {
        fn parse() -> io::Result<Self> {
            let mut options = Self {
                saves: 40,
                gap: Duration::from_millis(250),
                warmup: Duration::from_secs(15),
                cap: Duration::from_secs(30),
                dir: PathBuf::from("target/save-delay"),
                load: Vec::new(),
            };
            let mut args = std::env::args().skip(1);
            while let Some(arg) = args.next() {
                if arg == "--" {
                    options.load = args.collect();
                    break;
                }
                let value = args
                    .next()
                    .ok_or_else(|| invalid(format!("{arg} needs a value")))?;
                let number = || {
                    value
                        .parse::<u64>()
                        .map_err(|_| invalid(format!("{arg} takes a number, not {value}")))
                };
                match arg.as_str() {
                    "--saves" => options.saves = usize::try_from(number()?).map_err(invalid)?,
                    "--gap" => options.gap = Duration::from_millis(number()?),
                    "--warmup" => options.warmup = Duration::from_secs(number()?),
                    "--cap" => options.cap = Duration::from_secs(number()?),
                    "--dir" => options.dir = PathBuf::from(&value),
                    _ => return Err(invalid(format!("unknown option {arg}"))),
                }
            }
            Ok(options)
        }
    }

    pub fn run() -> io::Result<()> {
        let options = Options::parse()?;
        let package = options.dir.join("package");
        fs::create_dir_all(package.join("src"))?;
        let package = fs::canonicalize(&package)?;
        let files = [package.join("src/main.rs"), package.join("Cargo.toml")];
        fs::write(&files[0], "fn main() {}\n")?;
        fs::write(&files[1], "[package]\nname = \"probe\"\n")?;

        let (sender, arrivals) = mpsc::channel();
        let fsevents = sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let at = Instant::now();
                match event {
                    Ok(event) => {
                        for path in event.paths {
                            let _ = fsevents.send(Arrival {
                                source: Source::Fsevents,
                                path,
                                at,
                            });
                        }
                    }
                    Err(error) => eprintln!("fsevents: {error}"),
                }
            })
            .map_err(io::Error::other)?;
        let mut registration = watcher.paths_mut();
        registration
            .add(&package.join("src"), notify::RecursiveMode::Recursive)
            .map_err(io::Error::other)?;
        registration
            .add(&files[1], notify::RecursiveMode::NonRecursive)
            .map_err(io::Error::other)?;
        registration.commit().map_err(io::Error::other)?;
        vnode::watch(&files, sender)?;

        let idle = phase(&options, &files, &arrivals)?;
        report("idle", &idle);

        if let Some((program, args)) = options.load.split_first() {
            let mut load = process::Command::new(program)
                .args(args)
                .process_group(0)
                .stdout(process::Stdio::null())
                .stderr(process::Stdio::null())
                .spawn()?;
            println!(
                "load: started `{}`; saving after {} s",
                options.load.join(" "),
                options.warmup.as_secs()
            );
            thread::sleep(options.warmup);
            let loaded = phase(&options, &files, &arrivals)?;
            let ended = load.try_wait()?;
            vnode::stop(load.id());
            load.wait()?;
            if let Some(status) = ended {
                println!("load: ended before the last save ({status})");
            }
            report("loaded", &loaded);
        }

        drop(watcher);
        fs::remove_dir_all(&options.dir)
    }

    /// Save `options.saves` times and record when each watcher first
    /// reported each save.
    fn phase(
        options: &Options,
        files: &[PathBuf; 2],
        arrivals: &mpsc::Receiver<Arrival>,
    ) -> io::Result<Vec<Save>> {
        let names = ["src/main.rs", "Cargo.toml"];
        let mut saves = Vec::with_capacity(options.saves);
        for index in 0..options.saves {
            let which = index % 2;
            let path = &files[which];
            // Late reports of the previous save are not this save's.
            while arrivals.try_recv().is_ok() {}
            let written = Instant::now();
            fs::write(path, format!("// save {index}\nfn main() {{}}\n"))?;
            let mut save = Save {
                file: names[which],
                fsevents: None,
                kqueue: None,
            };
            let cap = written + options.cap;
            while save.fsevents.is_none() || save.kqueue.is_none() {
                let Ok(arrival) =
                    arrivals.recv_timeout(cap.saturating_duration_since(Instant::now()))
                else {
                    break;
                };
                if arrival.path != *path {
                    continue;
                }
                let delay = arrival.at.saturating_duration_since(written);
                match arrival.source {
                    Source::Fsevents => save.fsevents = save.fsevents.or(Some(delay)),
                    Source::Kqueue => save.kqueue = save.kqueue.or(Some(delay)),
                }
            }
            println!(
                "save {index:>3} {:<11} fsevents {:>10}  kqueue {:>10}",
                save.file,
                shown(save.fsevents),
                shown(save.kqueue)
            );
            saves.push(save);
            thread::sleep(options.gap);
        }
        Ok(saves)
    }

    fn report(phase: &str, saves: &[Save]) {
        for (name, source) in [("fsevents", Source::Fsevents), ("kqueue", Source::Kqueue)] {
            let mut delays = saves
                .iter()
                .filter_map(|save| match source {
                    Source::Fsevents => save.fsevents,
                    Source::Kqueue => save.kqueue,
                })
                .collect::<Vec<_>>();
            delays.sort();
            let missed = saves.len() - delays.len();
            let at = |fraction: f64| {
                // The rank of the `fraction` quantile among the sorted delays.
                let rank = ((delays.len() - 1) as f64 * fraction).round() as usize;
                shown(delays.get(rank).copied())
            };
            if delays.is_empty() {
                println!("{phase:<6} {name:<8} no reports, {missed} missed");
                continue;
            }
            let over = |limit: Duration| delays.iter().filter(|delay| **delay > limit).count();
            println!(
                "{phase:<6} {name:<8} min {} median {} p90 {} max {}; over 100 ms {}, over 1 s {}, missed {missed}",
                at(0.0),
                at(0.5),
                at(0.9),
                at(1.0),
                over(Duration::from_millis(100)),
                over(Duration::from_secs(1)),
            );
        }
    }

    fn shown(delay: Option<Duration>) -> String {
        match delay {
            Some(delay) => format!("{:.1} ms", delay.as_secs_f64() * 1000.0),
            None => "missed".to_owned(),
        }
    }

    fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }

    /// A kqueue vnode watch on open files, and stopping the load's group.
    mod vnode {
        use std::ffi::CString;
        use std::io;
        use std::os::unix::ffi::OsStrExt;
        use std::path::PathBuf;
        use std::ptr;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Instant;

        use super::{Arrival, Source};

        /// Watch each of `files` for writes, attribute changes, deletes and
        /// renames, and send each report on `sender` from a thread blocked in
        /// `kevent`.
        pub(super) fn watch(files: &[PathBuf; 2], sender: mpsc::Sender<Arrival>) -> io::Result<()> {
            // SAFETY: `kqueue` takes no arguments and returns a descriptor or -1.
            let queue = unsafe { libc::kqueue() };
            if queue < 0 {
                return Err(io::Error::last_os_error());
            }
            for (index, path) in files.iter().enumerate() {
                let name = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
                // SAFETY: `name` is a NUL-terminated path that outlives the call.
                let file = unsafe { libc::open(name.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
                if file < 0 {
                    return Err(io::Error::last_os_error());
                }
                let change = libc::kevent {
                    ident: file as usize,
                    filter: libc::EVFILT_VNODE,
                    flags: libc::EV_ADD | libc::EV_CLEAR,
                    fflags: libc::NOTE_WRITE
                        | libc::NOTE_EXTEND
                        | libc::NOTE_ATTRIB
                        | libc::NOTE_DELETE
                        | libc::NOTE_RENAME,
                    data: 0,
                    udata: index as *mut libc::c_void,
                };
                // SAFETY: one valid change in, no events out, no timeout.
                let added =
                    unsafe { libc::kevent(queue, &change, 1, ptr::null_mut(), 0, ptr::null()) };
                if added < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            let files = files.clone();
            thread::spawn(move || {
                loop {
                    // SAFETY: `kevent` is plain data, so zeroed is a valid value.
                    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                    // SAFETY: no changes in, room for one event out, and a null
                    // timeout blocks until an event arrives.
                    let count =
                        unsafe { libc::kevent(queue, ptr::null(), 0, &mut event, 1, ptr::null()) };
                    let at = Instant::now();
                    if count < 0 {
                        eprintln!("kqueue: {}", io::Error::last_os_error());
                        return;
                    }
                    let path = files[event.udata as usize].clone();
                    if sender
                        .send(Arrival {
                            source: Source::Kqueue,
                            path,
                            at,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
            Ok(())
        }

        /// Stop the load's process group, its compilers among them.
        pub(super) fn stop(leader: u32) {
            let Ok(group) = libc::pid_t::try_from(leader) else {
                return;
            };
            // SAFETY: a negative pid names the group the load leads.
            unsafe { libc::kill(-group, libc::SIGTERM) };
        }
    }
}
