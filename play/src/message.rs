//! Pure `?play` message parsing and reply decisions for the Discord bot.
//!
//! A message plays when its first line starts with `?play` or a code fence's
//! info string contains `?play`. Options are whitespace-separated tokens on
//! that line: `size=WxH` (or a bare `WxH`), `theme=dark|light` (or a bare
//! `dark`/`light`), and repeated `msg=Message::Increment` for a full app.
//!
//! ````text
//! ?play 320x200 light msg=Message::Increment
//! ```rust
//! column![text("hi"), button("go")]
//! ```
//! ````

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::{Format, Options};

/// Marker that turns a message into a render request.
pub const MARKER: &str = "?play";

/// Discord's message length limit.
pub const MAX_REPLY: usize = 2000;

/// Largest attachment a bot may upload to any server.
pub const MAX_ATTACHMENT: usize = 10 * 1024 * 1024;

/// One parsed render request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    /// The snippet inside the code block.
    pub code: String,
    /// Options from the marker line, over `defaults`.
    pub options: Options,
}

/// Parse a message. `None` means it is not a `?play` message at all.
pub fn parse(content: &str, defaults: &Options) -> Option<Result<Request, String>> {
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
    Some(request(content, options_line, defaults))
}

fn request(content: &str, options_line: &str, defaults: &Options) -> Result<Request, String> {
    let mut options = defaults.clone();
    let options_line = options_line.split("```").next().unwrap_or_default();
    for token in options_line.split_whitespace() {
        match token.split_once('=') {
            Some(("size", value)) => options.size = Some(crate::parse_size(value)?),
            Some(("theme", value)) => options.theme = value.parse()?,
            Some(("msg" | "message", value)) => options.messages.push(value.to_owned()),
            Some((key, _)) => {
                return Err(format!(
                    "unknown option `{key}`; expected size=WxH, theme=dark|light, msg=EXPR"
                ));
            }
            None if token.contains(['x', 'X'])
                && token.starts_with(|c: char| c.is_ascii_digit()) =>
            {
                options.size = Some(crate::parse_size(token)?);
            }
            None => options.theme = token.parse()?,
        }
    }
    let code = code_block(content).ok_or_else(|| {
        "put the snippet in a code block:\n?play\n```rust\ncolumn![text(\"hi\")]\n```".to_owned()
    })?;
    Ok(Request { code, options })
}

impl Request {
    /// Arguments for one `rawr-play` process that reads this request's code
    /// on stdin and writes the image to stdout.
    #[must_use]
    pub fn args(&self, format: Format, memory_mb: u64) -> Vec<String> {
        let options = &self.options;
        let mut args = vec!["-".to_owned()];
        if let Some((width, height)) = options.size {
            args.extend(["--size".to_owned(), format!("{width}x{height}")]);
        }
        args.extend([
            "--theme".to_owned(),
            options.theme.variant().to_ascii_lowercase(),
            "--format".to_owned(),
            format.extension().to_owned(),
            "--time-ms".to_owned(),
            options.limits.time.as_millis().to_string(),
            "--memory-mb".to_owned(),
            memory_mb.to_string(),
            "-o".to_owned(),
            "-".to_owned(),
        ]);
        for message in &options.messages {
            args.extend(["--message".to_owned(), message.clone()]);
        }
        args
    }
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

/// What the bot sends back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    /// Attach this PNG.
    Image(Vec<u8>),
    /// Send this text (already fenced and within [`MAX_REPLY`]).
    Text(String),
}

/// Decide the reply from a finished `rawr-play` process: the image on
/// success, its error text on a clean failure, and a short notice when it
/// was killed (out of memory under `--memory-mb`, or a crash).
#[must_use]
pub fn reply(success: bool, stdout: Vec<u8>, stderr: &str) -> Reply {
    match outcome(success, stdout, stderr) {
        Ok(image) => Reply::Image(image),
        Err(text) => Reply::Text(error_text(&text)),
    }
}

/// The image of a finished `rawr-play` process, or its error text unfenced
/// (as [`reply`] reads it; the playground shows it as is).
pub fn outcome(success: bool, stdout: Vec<u8>, stderr: &str) -> Result<Vec<u8>, String> {
    if success && !stdout.is_empty() {
        if stdout.len() > MAX_ATTACHMENT {
            return Err("the image is too large to upload; try a smaller size".to_owned());
        }
        return Ok(stdout);
    }
    let stderr = stderr.trim();
    if success || stderr.is_empty() || stderr.contains("memory allocation") {
        return Err("render stopped: out of memory or crashed".to_owned());
    }
    Err(stderr.to_owned())
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
            return false;
        }
        *count += 1;
        true
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        channels, error_text, parse, reply, RateLimit, Reply, Request, MAX_ATTACHMENT, MAX_REPLY,
    };
    use crate::{Format, Options, Theme};

    fn parsed(content: &str) -> Request {
        parse(content, &Options::default()).unwrap().unwrap()
    }

    #[test]
    fn message_without_marker_is_ignored() {
        assert!(parse("hello\n```rust\ntext(\"x\")\n```", &Options::default()).is_none());
        assert!(parse("", &Options::default()).is_none());
    }

    #[test]
    fn message_first_line_marker_with_options() {
        let request = parsed(
            "?play 320x200 light msg=Message::Increment msg=Message::Increment\n\
             ```rust\ncolumn![text(\"hi\"), button(\"go\")]\n```\nthanks",
        );
        assert_eq!(request.code, "column![text(\"hi\"), button(\"go\")]");
        assert_eq!(request.options.size, Some((320, 200)));
        assert_eq!(request.options.theme, Theme::Light);
        assert_eq!(request.options.messages, ["Message::Increment"; 2]);
    }

    #[test]
    fn message_fence_marker_and_keyed_options() {
        let request = parsed("look:\n```rust ?play size=100x50 theme=dark\nfn view() -> impl Widget<()> {\n    text(\"x\")\n}\n```");
        assert!(request.code.starts_with("fn view()"));
        assert!(request.code.ends_with('}'));
        assert_eq!(request.options.size, Some((100, 50)));
        assert_eq!(request.options.theme, Theme::Dark);
    }

    #[test]
    fn message_inline_block_and_errors() {
        assert_eq!(parsed("?play ```text(\"hi\")```").code, "text(\"hi\")");
        let missing = parse("?play", &Options::default()).unwrap();
        assert!(missing.unwrap_err().contains("code block"));
        let bad = parse("?play size=big\n```\nx\n```", &Options::default()).unwrap();
        assert!(bad.unwrap_err().contains("400x300"));
        let bad = parse("?play colour=red\n```\nx\n```", &Options::default()).unwrap();
        assert!(bad.unwrap_err().contains("unknown option"));
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
        let request = parsed("?play 320x200 light msg=Message::Increment\n```\nx\n```");
        let args = request.args(Format::Webp, 512);
        assert_eq!(
            args.join(" "),
            "- --size 320x200 --theme light --format webp --time-ms 2000 --memory-mb 512 -o - \
             --message Message::Increment"
        );
    }

    #[test]
    fn message_reply_decision() {
        assert_eq!(reply(true, vec![1, 2, 3], ""), Reply::Image(vec![1, 2, 3]));
        let Reply::Text(text) = reply(false, Vec::new(), "error: ```oops```\n") else {
            panic!("errors reply with text");
        };
        assert!(text.starts_with("```text\n") && text.ends_with("\n```"));
        assert_eq!(text.matches("```").count(), 2, "{text}");
        for (stdout, stderr) in [
            (vec![], "memory allocation of 9 bytes failed"),
            (vec![], ""),
        ] {
            let Reply::Text(text) = reply(false, stdout, stderr) else {
                panic!("a killed render replies with text");
            };
            assert!(text.contains("out of memory"), "{text}");
        }
        let Reply::Text(text) = reply(true, vec![0; MAX_ATTACHMENT + 1], "") else {
            panic!("an oversized image replies with text");
        };
        assert!(text.contains("too large"), "{text}");
        let long = error_text(&"é".repeat(5000));
        assert!(long.len() <= MAX_REPLY && long.ends_with("…\n```"));
    }

    #[test]
    fn message_rate_limit() {
        let start = Instant::now();
        let mut limit = RateLimit::new(2, Duration::from_secs(10));
        assert!(limit.allow(1, start));
        assert!(limit.allow(1, start));
        assert!(!limit.allow(1, start + Duration::from_secs(9)));
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
}
