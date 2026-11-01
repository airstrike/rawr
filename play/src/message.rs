//! Pure `?play` message parsing and reply decisions for the Discord bot.
//!
//! A message plays when its first line starts with `?play` or a code fence's
//! info string contains `?play`. `?play` takes no options.
//!
//! ````text
//! ?play
//! ```rust
//! println!("{}", 6 * 7);
//! ```
//! ````

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::Output;

/// Marker that turns a message into a run request.
pub const MARKER: &str = "?play";

/// Discord's message length limit.
pub const MAX_REPLY: usize = 2000;

/// One parsed run request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    /// The snippet inside the code block.
    pub code: String,
}

/// Parse a message. `None` means it is not a `?play` message at all.
pub fn parse(content: &str) -> Option<Result<Request, String>> {
    let first = content.lines().find(|line| !line.trim().is_empty())?.trim();
    let options_line = if let Some(rest) = first.strip_prefix(MARKER) {
        rest
    } else {
        let fence = content
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("```") && line.contains(MARKER))?;
        fence.split_once(MARKER).map_or("", |(_, rest)| rest)
    };
    Some(request(content, options_line))
}

fn request(content: &str, options_line: &str) -> Result<Request, String> {
    let options_line = options_line.split("```").next().unwrap_or_default();
    if let Some(token) = options_line.split_whitespace().next() {
        return Err(format!("?play takes no options: `{token}`"));
    }
    let code = code_block(content).ok_or_else(|| {
        "put the snippet in a code block:\n?play\n```rust\nprintln!(\"hi\");\n```".to_owned()
    })?;
    Ok(Request { code })
}

/// Arguments for one `rawr-play` process that reads a request's code on
/// stdin and writes its result to stdout.
#[must_use]
pub fn args(time: Duration, memory_mb: u64) -> Vec<String> {
    vec![
        "-".to_owned(),
        "--time-ms".to_owned(),
        time.as_millis().to_string(),
        "--memory-mb".to_owned(),
        memory_mb.to_string(),
        "-o".to_owned(),
        "-".to_owned(),
    ]
}

/// The body of the first fenced code block (multi-line or inline).
fn code_block(content: &str) -> Option<String> {
    let after = &content[content.find("```")? + 3..];
    let line_end = after.find('\n').unwrap_or(after.len());
    let code = if let Some(end) = after[..line_end].find("```") {
        &after[..end]
    } else {
        let body = after.get(line_end + 1..)?;
        &body[..body.find("```")?]
    };
    let code = code.trim();
    (!code.is_empty()).then(|| code.to_owned())
}

/// Discord's attachment limit for a bot, in bytes.
pub const MAX_ATTACHMENT: usize = 8 * 1024 * 1024;

/// What a finished `rawr-play` process gave.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Done {
    /// The program's output.
    Text(Output),
    /// The PNG of an iced program's first settled frame.
    Image(Vec<u8>),
}

/// What the bot sends back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    /// This text, already fenced and within [`MAX_REPLY`].
    Text(String),
    /// This PNG, attached.
    Image(Vec<u8>),
}

/// The reply to a finished `rawr-play` process: the output fenced or the
/// frame attached, its error text fenced on a clean failure, and a short
/// notice when it was killed (out of memory under `--memory-mb`, or a
/// crash).
#[must_use]
pub fn reply(success: bool, stdout: &[u8], stderr: &str) -> Reply {
    match outcome(success, stdout, stderr) {
        Ok(Done::Text(output)) => Reply::Text(output_text(&output)),
        Ok(Done::Image(png)) => Reply::Image(png),
        Err(text) => Reply::Text(error_text(&text)),
    }
}

/// Whether `bytes` are a PNG.
#[must_use]
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// The result of a finished `rawr-play` process, or its error text
/// unfenced (as [`reply`] reads it; the playground shows it as is). A
/// result starting with `{` is a program's output, and a PNG is an iced
/// program's frame.
pub fn outcome(success: bool, stdout: &[u8], stderr: &str) -> Result<Done, String> {
    if success && stdout.first() == Some(&b'{') {
        return Output::from_json(stdout).map(Done::Text);
    }
    if success && is_png(stdout) {
        if stdout.len() > MAX_ATTACHMENT {
            return Err(format!(
                "the window's frame is {} MiB as a PNG, over Discord's {} MiB",
                stdout.len().div_ceil(1024 * 1024),
                MAX_ATTACHMENT / (1024 * 1024)
            ));
        }
        return Ok(Done::Image(stdout.to_vec()));
    }
    let stderr = stderr.trim();
    if success || stderr.is_empty() || stderr.contains("memory allocation") {
        return Err("?play stopped: out of memory or crashed".to_owned());
    }
    Err(sanitize(stderr))
}

/// `text` without the server's file paths: a crash's `thread 'main'
/// panicked at /path/file.rs:1:2:` line becomes a plain one, and Rust's
/// note on backtraces goes.
#[must_use]
pub fn sanitize(text: &str) -> String {
    if !text.contains("panicked at ") {
        return text.to_owned();
    }
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("thread '") && line.contains(" panicked at ") {
            out.push("?play crashed. This is a bug in ?play, not in your code.".to_owned());
        } else if !line.starts_with("note: run with `RUST_BACKTRACE") && !line.trim().is_empty() {
            let words: Vec<String> = line
                .split(' ')
                .map(|word| match word.strip_prefix('/') {
                    Some(rest) if rest.contains('/') => {
                        word.rsplit('/').next().unwrap_or_default().to_owned()
                    }
                    _ => word.to_owned(),
                })
                .collect();
            out.push(words.join(" "));
        }
    }
    out.join("\n")
}

/// At most `max` requests per key in each `window` (fixed windows).
#[derive(Debug)]
pub struct RateLimit {
    max: u32,
    window: Duration,
    seen: HashMap<u64, (Instant, u32)>,
}

impl RateLimit {
    /// A limit of `max` requests per `window` for each key.
    #[must_use]
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            seen: HashMap::new(),
        }
    }

    /// Count a request from `key` at `now`; `false` if it is over the limit.
    pub fn allow(&mut self, key: u64, now: Instant) -> bool {
        self.wait(key, now).is_none()
    }

    /// Count a request from `key` at `now`, as [`Self::allow`] does; over
    /// the limit, how long until its window ends instead.
    pub fn wait(&mut self, key: u64, now: Instant) -> Option<Duration> {
        let window = self.window;
        if self.seen.len() >= 4096 {
            self.seen
                .retain(|_, (start, _)| now.duration_since(*start) < window);
        }
        let (start, count) = self.seen.entry(key).or_insert((now, 0));
        if now.duration_since(*start) >= window {
            (*start, *count) = (now, 0);
        }
        if *count >= self.max {
            return Some(window.saturating_sub(now.duration_since(*start)));
        }
        *count += 1;
        None
    }
}

/// Channel IDs from a comma-separated list such as `PLAY_CHANNELS`; empty
/// means every channel. An unreadable ID is an error, never a silent "all".
pub fn channels(list: &str) -> Result<HashSet<u64>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| id.parse().map_err(|_| format!("not a channel ID: {id:?}")))
        .collect()
}

/// A plain program's output as a reply: fenced as text and cut to fit
/// Discord with a note, then its exit status when that is not 0.
#[must_use]
pub fn output_text(output: &Output) -> String {
    const OPEN: &str = "```text\n";
    const CLOSE: &str = "\n```";
    const CUT: &str = "\n… cut to fit Discord";
    let status = if output.status == 0 {
        String::new()
    } else {
        format!("\nexit status {}", output.status)
    };
    let text = output.text().replace("```", "`\u{200b}``");
    let text = text.trim_end();
    let text = if text.is_empty() { "(no output)" } else { text };
    let budget = MAX_REPLY - OPEN.len() - CLOSE.len() - CUT.len() - status.len() - 128;
    let body = if text.len() > budget {
        let mut end = budget;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{CUT}", text[..end].trim_end())
    } else {
        text.to_owned()
    };
    let note = output.note.as_ref().map_or_else(String::new, |note| {
        note.lines().map(|line| format!("\n-# {line}")).collect()
    });
    format!("{OPEN}{body}{CLOSE}{status}{note}")
}

/// Fence error text and keep the whole reply within Discord's limit.
#[must_use]
pub fn error_text(text: &str) -> String {
    const OPEN: &str = "```text\n";
    const CLOSE: &str = "\n```";
    const MORE: &str = "\n…";
    let text = text.replace("```", "`\u{200b}``");
    let budget = MAX_REPLY - OPEN.len() - CLOSE.len() - MORE.len();
    let body = if text.len() > budget {
        let mut end = budget;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{MORE}", text[..end].trim_end())
    } else {
        text.trim_end().to_owned()
    };
    format!("{OPEN}{body}{CLOSE}")
}

/// The bot's own earlier reply to `request`, among `messages` given as
/// (id, author, the message it replies to). The bot keeps its replies in
/// memory, so after a restart an edit finds its reply this way. The oldest
/// match wins.
pub fn own_reply(
    messages: impl IntoIterator<Item = (u64, u64, Option<u64>)>,
    bot: u64,
    request: u64,
) -> Option<u64> {
    messages
        .into_iter()
        .filter(|&(_, author, to)| author == bot && to == Some(request))
        .map(|(id, ..)| id)
        .min()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        channels, error_text, output_text, parse, reply, RateLimit, Reply, Request, MAX_REPLY,
    };

    fn parsed(content: &str) -> Request {
        parse(content).unwrap().unwrap()
    }

    /// The text of a reply that is text.
    fn reply_text(reply: Reply) -> String {
        match reply {
            Reply::Text(text) => text,
            Reply::Image(_) => panic!("an image reply"),
        }
    }

    #[test]
    fn message_reply_for_a_frame_attaches_it() {
        let png = b"\x89PNG\r\n\x1a\n rest".to_vec();
        assert_eq!(reply(true, &png, ""), Reply::Image(png));
        let big = [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &vec![0; super::MAX_ATTACHMENT],
        ]
        .concat();
        assert!(reply_text(reply(true, &big, "")).contains("over Discord's 8 MiB"));
    }

    #[test]
    fn message_without_marker_is_ignored() {
        assert!(parse("hello\n```rust\ntext(\"x\")\n```").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn message_first_line_or_fence_marker() {
        let request = parsed("?play\n```rust\nprintln!(\"hi\");\n```\nthanks");
        assert_eq!(request.code, "println!(\"hi\");");
        let request = parsed("look:\n```rust ?play\nfn main() {\n    println!(\"x\");\n}\n```");
        assert!(request.code.starts_with("fn main()"));
        assert!(request.code.ends_with('}'));
    }

    #[test]
    fn message_inline_block_and_errors() {
        assert_eq!(parsed("?play ```1 + 2```").code, "1 + 2");
        let missing = parse("?play").unwrap();
        assert!(missing.unwrap_err().contains("code block"));
        for content in ["?play light\n```\nx\n```", "```rust ?play 320x200\nx\n```"] {
            let error = parse(content).unwrap().unwrap_err();
            assert!(error.starts_with("?play takes no options"), "{error}");
        }
    }

    #[test]
    fn message_fence_languages() {
        for fence in ["```rust", "```rawr", "```", "```rs"] {
            let request = parsed(&format!("?play\n{fence}\ntext(\"hi\")\n```"));
            assert_eq!(request.code, "text(\"hi\")", "{fence}");
        }
    }

    #[test]
    fn message_request_args() {
        assert_eq!(
            super::args(std::time::Duration::from_millis(2000), 512).join(" "),
            "- --time-ms 2000 --memory-mb 512 -o -"
        );
    }

    #[test]
    fn message_reply_decision() {
        let text = reply_text(reply(false, &[], "error: ```oops```\n"));
        assert!(text.starts_with("```text\n") && text.ends_with("\n```"));
        assert_eq!(text.matches("```").count(), 2, "{text}");
        for stderr in ["memory allocation of 9 bytes failed", ""] {
            let text = reply_text(reply(false, &[], stderr));
            assert!(text.contains("out of memory"), "{text}");
        }
        let long = error_text(&"é".repeat(5000));
        assert!(long.len() <= MAX_REPLY && long.ends_with("…\n```"));
    }

    #[test]
    fn message_reply_for_a_plain_program() {
        use crate::plain::Stream::{Stderr, Stdout};
        let output = crate::Output {
            chunks: vec![(Stdout, "total: 27\n".into())],
            ..crate::Output::default()
        };
        assert_eq!(
            reply_text(reply(true, output.to_json().as_bytes(), "")),
            "```text\ntotal: 27\n```"
        );
        let failed = crate::Output {
            chunks: vec![(Stdout, "a\n".into()), (Stderr, "Error: \"no\"\n".into())],
            status: 1,
            ..crate::Output::default()
        };
        assert_eq!(
            output_text(&failed),
            "```text\na\nError: \"no\"\n```\nexit status 1"
        );
        let noted = crate::Output {
            note: Some(crate::plain::UNUSED_ITERATOR.into()),
            ..output.clone()
        };
        assert_eq!(
            output_text(&noted),
            format!(
                "```text\ntotal: 27\n```\n-# {}",
                crate::plain::UNUSED_ITERATOR
            )
        );
        let silent = output_text(&crate::Output::default());
        assert_eq!(silent, "```text\n(no output)\n```");
        let long = crate::Output {
            chunks: vec![(Stdout, "line ```\n".repeat(500))],
            status: 1,
            ..crate::Output::default()
        };
        let text = output_text(&long);
        assert!(text.len() <= MAX_REPLY, "{}", text.len());
        assert!(
            text.ends_with("… cut to fit Discord\n```\nexit status 1"),
            "{text}"
        );
        assert_eq!(text.matches("```").count(), 2, "{text}");
    }

    #[test]
    fn message_rate_limit() {
        let start = Instant::now();
        let mut limit = RateLimit::new(2, Duration::from_secs(10));
        assert!(limit.allow(1, start));
        assert!(limit.allow(1, start));
        assert!(!limit.allow(1, start + Duration::from_secs(9)));
        assert_eq!(
            limit.wait(1, start + Duration::from_secs(9)),
            Some(Duration::from_secs(1)),
            "over the limit until the window ends"
        );
        assert!(limit.allow(2, start), "keys are independent");
        assert!(
            limit.allow(1, start + Duration::from_secs(10)),
            "a new window"
        );
    }

    #[test]
    fn message_channel_list() {
        assert!(channels("").unwrap().is_empty());
        let list = channels(" 1, 22 ,,333").unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.contains(&22));
        assert!(channels("1,general").is_err());
    }

    #[test]
    fn crash_text_has_no_server_paths() {
        let stderr = "\nthread 'main' (17) panicked at /usr/local/cargo/git/checkouts/iced-3e930cb38a75fe30/be88873/tiny_skia/src/raster.rs:138:14:\n\
                      Image should be allocated\n\
                      note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n";
        let text = reply_text(reply(false, &[], stderr));
        assert_eq!(
            text,
            "```text\n?play crashed. This is a bug in ?play, not in your code.\nImage should be allocated\n```"
        );
        let other =
            super::sanitize("thread 'main' panicked at src/x.rs:1:2:\nopened /srv/app/data.png\n");
        assert!(!other.contains("/srv"), "{other}");
        let plain = "error: `/a/b` is not a path\n";
        assert_eq!(super::sanitize(plain), plain, "only a crash is rewritten");
    }

    #[test]
    fn output_note_lines_are_each_small() {
        let output = crate::Output {
            note: Some("one\ntwo".into()),
            ..crate::Output::default()
        };
        assert!(output_text(&output).ends_with("\n-# one\n-# two"));
    }

    #[test]
    fn own_reply_is_the_bots_reply_to_that_request() {
        const BOT: u64 = 1;
        const REQUEST: u64 = 100;
        let messages = [
            (105, 7, Some(REQUEST)),   // someone else replying to it
            (104, BOT, Some(99)),      // our reply to another request
            (103, BOT, None),          // ours, no reference
            (102, BOT, Some(REQUEST)), // ours: the one
            (101, 7, None),
        ];
        assert_eq!(super::own_reply(messages, BOT, REQUEST), Some(102));
        assert_eq!(super::own_reply(messages, BOT, 98), None);
        assert_eq!(super::own_reply([], BOT, REQUEST), None);
        let twice = [(110, BOT, Some(REQUEST)), (102, BOT, Some(REQUEST))];
        assert_eq!(
            super::own_reply(twice, BOT, REQUEST),
            Some(102),
            "oldest wins"
        );
    }
}
