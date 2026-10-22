//! Bounded synchronous observation of pinned locator callsites.
//! This collector grants no file identity, content or invocation authority.

use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex};

use rustc_log::tracing;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

use super::Budget;

const FILE: &str =
    "/rustc-dev/375b1431b7d89d1c2e2bc168c011848ae12b7d14/compiler/rustc_metadata/src/locator.rs";
const LINE: u32 = 598;
const QUERY_LINE: u32 = 442;
const QUERY_PREFIX: &str = "lib candidate: ";
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVENTS: usize = 65_536;
const PREFIX: &str = "rlib reading metadata from: ";

pub(super) struct Collector {
    state: Arc<Mutex<State>>,
}

struct Observer {
    state: Arc<Mutex<State>>,
}

struct State {
    bytes: Vec<u8>,
    ranges: Vec<(bool, usize, usize)>,
    byte_limit: usize,
    event_limit: usize,
    events: usize,
    seen: bool,
    query_seen: bool,
    conservative: bool,
    failure: Option<&'static str>,
    closed: bool,
}

/// Completed observation data. Paths remain display strings until an exact
/// candidate-namespace join, and this value deliberately has no Serialize impl.
pub(super) struct Collected {
    bytes: Vec<u8>,
    ranges: Vec<(bool, usize, usize)>,
    attempts: usize,
    pub(super) conservative: bool,
}

impl Collected {
    pub(super) fn displays(&self) -> impl ExactSizeIterator<Item = &str> + Clone {
        self.ranges[..self.attempts].iter().map(|&(_, start, end)| {
            // Only fmt::Write's UTF-8 input reaches this buffer; all boundaries
            // are taken at completed writes. A failed write cannot complete.
            std::str::from_utf8(&self.bytes[start..end]).expect("formatted UTF-8 invariant")
        })
    }
}

impl Collected {
    pub(super) fn queries(&self) -> impl ExactSizeIterator<Item = &str> + Clone {
        self.ranges[self.attempts..].iter().map(|&(_, start, end)| {
            std::str::from_utf8(&self.bytes[start..end]).expect("formatted UTF-8 invariant")
        })
    }
}

impl Collector {
    pub(super) fn install(budget: &mut Budget) -> Result<Self, String> {
        if tracing::level_filters::STATIC_MAX_LEVEL < tracing::level_filters::LevelFilter::INFO {
            return Err("compiler attempt INFO events are unavailable".to_owned());
        }
        let state = Arc::new(Mutex::new(State::admit(budget, MAX_BYTES, MAX_EVENTS)?));
        tracing::subscriber::set_global_default(Observer {
            state: Arc::clone(&state),
        })
        .map_err(|_| "compiler attempt dispatcher installation failed".to_owned())?;
        Ok(Self { state })
    }

    /// Call only after the compiler has returned successfully with its analysis
    /// callback complete. This closes collection and does not synthesize success
    /// for an unobserved, unsupported, or refused callsite.
    pub(super) fn complete(self) -> Result<Collected, String> {
        self.state
            .lock()
            .map_err(|_| "compiler attempt collector was poisoned".to_owned())?
            .complete()
            .map_err(str::to_owned)
    }
}

impl State {
    fn admit(budget: &mut Budget, byte_limit: usize, event_limit: usize) -> Result<Self, String> {
        budget.bytes(byte_limit)?;
        budget.items(event_limit.saturating_add(2))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(byte_limit)
            .map_err(|_| "compiler attempt byte allocation failed".to_owned())?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(event_limit)
            .map_err(|_| "compiler attempt event allocation failed".to_owned())?;
        Ok(Self {
            bytes,
            ranges,
            byte_limit,
            event_limit,
            events: 0,
            seen: false,
            query_seen: false,
            conservative: false,
            failure: None,
            closed: false,
        })
    }

    fn fail(&mut self, reason: &'static str) {
        self.failure.get_or_insert(reason);
    }

    #[cfg(test)]
    fn record(&mut self, value: &dyn fmt::Debug) {
        self.record_kind(value, false);
    }

    fn record_kind(&mut self, value: &dyn fmt::Debug, query: bool) {
        if self.closed {
            self.fail("compiler attempt arrived after completion");
        }
        if self.failure.is_some() {
            return;
        }
        self.seen |= !query;
        self.query_seen |= query;
        if self.events >= self.event_limit {
            self.fail("compiler attempt event limit exceeded");
            return;
        }
        self.events += 1;
        let start = self.bytes.len();
        if write!(self, "{value:?}").is_err() {
            self.fail("compiler attempt formatting refused");
            return;
        }
        let message = &self.bytes[start..];
        // Path::display can replace invalid native bytes. Even a literal U+FFFD
        // takes the conservative branch: display data cannot disambiguate it.
        self.conservative |= message.windows(3).any(|bytes| bytes == b"\xef\xbf\xbd");
        if query {
            if message.starts_with(QUERY_PREFIX.as_bytes()) && message.len() > QUERY_PREFIX.len() {
                self.ranges
                    .push((true, start + QUERY_PREFIX.len(), self.bytes.len()));
            } else {
                self.fail("compiler query format differs from the pinned callsite");
            }
            return;
        }
        if message.starts_with(PREFIX.as_bytes()) && message.len() > PREFIX.len() {
            self.ranges
                .push((false, start + PREFIX.len(), self.bytes.len()));
        } else {
            if message.starts_with(b"sdylib reading metadata from: ")
                && message.len() > b"sdylib reading metadata from: ".len()
            {
                self.conservative = true;
            } else if ![
                b"rmeta reading metadata from: ",
                b"dylib reading metadata from: ",
            ]
            .iter()
            .any(|prefix| message.starts_with(*prefix) && message.len() > prefix.len())
            {
                self.fail("compiler attempt format differs from the pinned callsite");
            }
            self.bytes.truncate(start);
        }
    }

    fn complete(&mut self) -> Result<Collected, &'static str> {
        if self.closed {
            self.fail("compiler attempt collector completed twice");
        }
        self.closed = true;
        if !self.seen {
            self.fail("compiler attempt callsite was not observed");
        }
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        // In-place ordering separates the two populations without another
        // allocation; byte offsets preserve event order within each population.
        self.ranges.sort_unstable();
        let attempts = self.ranges.partition_point(|row| !row.0);
        Ok(Collected {
            bytes: std::mem::take(&mut self.bytes),
            ranges: std::mem::take(&mut self.ranges),
            attempts,
            // An unobserved query callsite cannot grant narrower query coverage.
            conservative: self.conservative || !self.query_seen,
        })
    }
}

impl fmt::Write for State {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if value.len() > self.byte_limit.saturating_sub(self.bytes.len()) {
            self.fail("compiler attempt byte limit exceeded");
            return Err(fmt::Error);
        }
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
}

struct Fields<'a> {
    state: &'a mut State,
    messages: usize,
    query: bool,
}

impl tracing::field::Visit for Fields<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() != "message" || self.messages != 0 {
            self.state.fail("compiler attempt event fields changed");
            return;
        }
        self.messages += 1;
        self.state.record_kind(value, self.query);
    }
}

impl Subscriber for Observer {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event()
            && metadata.target() == "rustc_metadata::locator"
            && metadata.file() == Some(FILE)
            && matches!(metadata.line(), Some(LINE | QUERY_LINE))
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let Ok(mut state) = self.state.lock() else {
            return; // Poison is terminal at completion.
        };
        if !self.enabled(event.metadata()) || event.metadata().level() != &tracing::Level::INFO {
            state.fail("compiler attempt callsite changed");
            return;
        }
        let mut fields = Fields {
            state: &mut state,
            messages: 0,
            query: event.metadata().line() == Some(QUERY_LINE),
        };
        event.record(&mut fields);
        if fields.messages != 1 {
            fields.state.fail("compiler attempt message was absent");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_uses_the_remaining_original_account() {
        let mut exact = Budget::with_limits(1031, 0);
        exact.bytes(7).unwrap();
        exact.items(3).unwrap();
        State::admit(&mut exact, 1024, 4).unwrap();
        assert_eq!(exact.usage(), (1031, 9, 0));

        let mut short_bytes = Budget::with_limits(1030, 0);
        short_bytes.bytes(7).unwrap();
        short_bytes.items(3).unwrap();
        assert!(State::admit(&mut short_bytes, 1024, 4).is_err());
        assert_eq!(short_bytes.usage(), (1031, 3, 0));
    }

    #[test]
    fn extra_or_duplicate_event_fields_refuse_the_collection() {
        use tracing::field::Visit as _;
        let callsite = tracing::callsite! {
            name: "attempt field fixture",
            kind: tracing::metadata::Kind::EVENT,
            target: "fixture",
            level: tracing::Level::INFO,
            fields: message, extra
        };
        let metadata = tracing::callsite::Callsite::metadata(callsite);
        for name in ["message", "extra"] {
            let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
            let mut visitor = Fields {
                state: &mut state,
                messages: 0,
                query: false,
            };
            visitor.record_debug(
                &metadata.fields().field("message").unwrap(),
                &format_args!("rlib reading metadata from: /a.rlib"),
            );
            visitor.record_debug(
                &metadata.fields().field(name).unwrap(),
                &format_args!("rlib reading metadata from: /b.rlib"),
            );
            assert!(state.complete().is_err());
        }
    }

    #[test]
    fn pinned_compiler_dispatch_completes_in_an_isolated_process() {
        const CHILD: &str = "RAWR_ATTEMPT_COLLECTOR_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "compiler::attempt::tests::pinned_compiler_dispatch_completes_in_an_isolated_process",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "rawr-attempt-collector-test-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("input.rs");
        std::fs::write(&source, "pub fn answer() -> u32 { 42 }\n").unwrap();
        let collector = Collector::install(&mut Budget::standard()).unwrap();
        assert!(Collector::install(&mut Budget::standard()).is_err());
        struct Callback(bool);
        impl rustc_driver::Callbacks for Callback {
            fn after_analysis<'tcx>(
                &mut self,
                _: &rustc_interface::interface::Compiler,
                _: rustc_middle::ty::TyCtxt<'tcx>,
            ) -> rustc_driver::Compilation {
                self.0 = true;
                rustc_driver::Compilation::Stop
            }
        }
        let mut callback = Callback(false);
        let exit = rustc_driver::catch_with_exit_code(|| {
            rustc_driver::run_compiler(
                &[
                    "rustc".to_owned(),
                    "--crate-name=attempt_fixture".to_owned(),
                    "--crate-type=rlib".to_owned(),
                    "--emit=metadata".to_owned(),
                    "-Zthreads=2".to_owned(),
                    source.to_str().unwrap().to_owned(),
                ],
                &mut callback,
            );
        });
        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        assert!(callback.0);
        let collected = collector.complete().unwrap();
        assert!(!collected.conservative);
        assert!(collected.queries().len() > 0);
        for path in collected.displays() {
            assert!(std::path::Path::new(path).is_absolute());
            assert!(path.ends_with(".rlib"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn query_and_attempt_populations_share_one_bounded_account() {
        let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        state.record_kind(&format_args!("lib candidate: /a.rmeta"), true);
        state.record(&format_args!("rlib reading metadata from: /b.rlib"));
        state.record_kind(&format_args!("lib candidate: /a.rmeta"), true);
        let collected = state.complete().unwrap();
        assert!(!collected.conservative);
        assert_eq!(
            collected.queries().collect::<Vec<_>>(),
            ["/a.rmeta", "/a.rmeta"]
        );
        assert_eq!(collected.displays().collect::<Vec<_>>(), ["/b.rlib"]);

        let mut state = State::admit(&mut Budget::standard(), 1024, 1).unwrap();
        state.record_kind(&format_args!("lib candidate: /a.rmeta"), true);
        state.record(&format_args!("rlib reading metadata from: /b.rlib"));
        assert!(state.complete().is_err());
        let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        state.record_kind(&format_args!("rmeta reading metadata from: /a.rmeta"), true);
        assert!(state.complete().is_err());
        let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        state.record(&format_args!("rlib reading metadata from: /b.rlib"));
        assert!(state.complete().unwrap().conservative);
    }

    #[test]
    fn zero_attempts_requires_observed_callsite_and_single_completion() {
        let mut missing = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        assert!(missing.complete().is_err());
        let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        state.record(&format_args!("rmeta reading metadata from: /a.rmeta"));
        assert_eq!(state.complete().unwrap().displays().len(), 0);
        assert!(state.complete().is_err());
    }

    #[test]
    fn retains_repeated_attempts_and_falls_back_for_interface_children() {
        let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        for _ in 0..2 {
            state.record(&format_args!("rlib reading metadata from: /a\nλ.rlib"));
        }
        state.record(&format_args!("sdylib reading metadata from: /a.rs"));
        let observed = state.complete().unwrap();
        assert!(observed.conservative);
        assert_eq!(
            observed.displays().collect::<Vec<_>>(),
            ["/a\nλ.rlib", "/a\nλ.rlib"]
        );
    }

    #[test]
    fn lossy_native_display_cannot_grant_exact_attempt_coverage() {
        use std::os::unix::ffi::OsStrExt as _;
        let native = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/a\xff.rlib"));
        let mut state = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        state.record(&format_args!(
            "rlib reading metadata from: {}",
            native.display()
        ));
        assert!(state.complete().unwrap().conservative);

        let mut empty = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        empty.record(&format_args!("rmeta reading metadata from: "));
        assert!(empty.complete().is_err());
    }

    #[test]
    fn byte_event_and_format_failures_latch_without_partial_completion() {
        let mut bytes = State::admit(&mut Budget::standard(), 4, 4).unwrap();
        bytes.record(&format_args!("rlib reading metadata from: /a.rlib"));
        assert!(bytes.complete().is_err());
        let mut events = State::admit(&mut Budget::standard(), 1024, 1).unwrap();
        events.record(&format_args!("rmeta reading metadata from: /a.rmeta"));
        events.record(&format_args!("rmeta reading metadata from: /b.rmeta"));
        assert!(events.complete().is_err());
        let mut malformed = State::admit(&mut Budget::standard(), 1024, 4).unwrap();
        malformed.record(&format_args!("unknown reading metadata from: /a"));
        malformed.record(&format_args!("rlib reading metadata from: /a.rlib"));
        assert!(malformed.complete().is_err());
    }
}
