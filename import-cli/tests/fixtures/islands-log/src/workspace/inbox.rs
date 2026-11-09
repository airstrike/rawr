// rawr
//! The inbox, marked, as inboard's `workspace/inbox.rs`: it compares
//! `tracing`'s and `log`'s levels as their macros do, and names std's
//! macros through the crates' re-exports of them.

/// `tracing`'s enabled check, a `Level` compared with a `LevelFilter`, and
/// `log`'s, each way round.
pub fn levels() -> String {
    use tracing::level_filters::{LevelFilter, STATIC_MAX_LEVEL};
    use tracing::Level;
    let level = Level::WARN;
    let compiled = level <= STATIC_MAX_LEVEL;
    let current = level <= LevelFilter::current();
    let off = Level::TRACE > LevelFilter::OFF;
    let filter = LevelFilter::INFO >= Level::DEBUG;
    let equal = LevelFilter::WARN == Level::WARN;
    let unequal = Level::ERROR != LevelFilter::WARN;
    let same = Level::ERROR < Level::WARN;
    let log = log::Level::Warn <= log::STATIC_MAX_LEVEL && log::Level::Debug > log::max_level();
    format!(
        "compiled {compiled} current {current} off {off} filter {filter} \
         equal {equal} unequal {unequal} same {same} log {log}"
    )
}

/// std's macros by their paths and through the crates' re-exports of
/// them, as `log`'s and `tracing`'s macros name them.
pub fn reexported() -> String {
    let joined = std::concat!("a", core::stringify!(b), 1);
    let module = log::__private_api::module_path!();
    let event = tracing::__macro_support::concat!(
        "event ",
        tracing::__macro_support::stringify!(x),
        tracing::__macro_support::line!()
    );
    let text = log::__private_api::format_args!("{joined} {}", 2).to_string();
    format!("{joined} {module} {event} {text}")
}

/// Where its calls are, through a crate's macros that name std's.
pub fn here() -> String {
    located::here!()
}

pub fn name() -> &'static str {
    located::name!()
}

/// A nested expansion is where its outermost call is, and the caller's own
/// `line!()` is where the caller wrote it.
pub fn traced() -> String {
    let first = located::traced!(line!());
    let second = located::traced!(
        // On a line of its own.
        line!()
    );
    format!("{first}; {second}")
}

const TARGET: &str = module_path!();
const NAMED: &str = located::name!();

mod folder {
    pub struct Folder;

    impl Folder {
        pub fn path(&self) -> String {
            let block = {
                const INNER: &str = module_path!();
                INNER
            };
            format!("{} {block} {}", module_path!(), located::here!())
        }
    }
}

/// A marked value, which marked code formats.
#[derive(Debug)]
pub struct Message {
    pub subject: String,
    pub unread: bool,
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.subject, if self.unread { "*" } else { "" })
    }
}

/// Marked values lent to native functions as `&dyn Debug`, `&dyn Display`
/// and `&dyn LowerHex`, and `format_args!` given as `fmt::Arguments`.
pub fn crossing() -> String {
    let message = Message {
        subject: "hello".to_owned(),
        unread: true,
    };
    let count = 2;
    let read = Message {
        subject: "old".to_owned(),
        unread: false,
    };
    [
        located::sink::debug(&message),
        located::sink::display(&message),
        located::sink::both("count", &count, &Some("two")),
        located::sink::debug(&[Some(Ok::<_, ()>(read)), None]),
        located::sink::hex(&255),
        located::sink::arguments(format_args!("{} has {count}", message.subject)),
        located::sink::unread(&message).to_owned(),
    ]
    .join("; ")
}

/// `module_path!()` in a constant, an inline module and its impl.
pub fn modules() -> String {
    format!("{TARGET}; {NAMED}; {}", folder::Folder.path())
}
