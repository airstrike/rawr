//! Pure `?play` message parsing and reply decisions for the Discord bot.
//!
//! A message plays when its first line starts with `?play` or a code fence's
//! info string contains `?play`. Options are whitespace-separated tokens on
//! that line: `size=WxH` (or a bare `WxH`), `theme=dark|light` (or a bare
//! `dark`/`light`), repeated `msg=Message::Increment` for a full app, and
//! a bare `mine`, which lets only the request's author click its reply.
//!
//! ````text
//! ?play 320x200 light msg=Message::Increment
//! ```rust
//! column![text("hi"), button("go")]
//! ```
//! ````

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::{Format, Options, Output};

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
    /// Only the request's author may click its reply's buttons (`mine`).
    /// Anyone may click otherwise.
    pub mine: bool,
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
    let mut mine = false;
    let options_line = options_line.split("```").next().unwrap_or_default();
    for token in options_line.split_whitespace() {
        match token.split_once('=') {
            Some(("size", value)) => options.size = Some(crate::parse_size(value)?),
            Some(("theme", value)) => options.theme = value.parse()?,
            Some(("msg" | "message", value)) => options.messages.push(value.to_owned()),
            Some((key, _)) => {
                return Err(format!(
                    "unknown option `{key}`; expected size=WxH, theme=dark|light, msg=EXPR, mine"
                ));
            }
            None if token.contains(['x', 'X'])
                && token.starts_with(|c: char| c.is_ascii_digit()) =>
            {
                options.size = Some(crate::parse_size(token)?);
            }
            None if token.eq_ignore_ascii_case("mine") => mine = true,
            None => options.theme = token.parse()?,
        }
    }
    let code = code_block(content).ok_or_else(|| {
        "put the snippet in a code block:\n?play\n```rust\ncolumn![text(\"hi\")]\n```".to_owned()
    })?;
    Ok(Request {
        code,
        options,
        mine,
    })
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
            "--animation-ms".to_owned(),
            options.limits.animation_budget().as_millis().to_string(),
            "--memory-mb".to_owned(),
            memory_mb.to_string(),
            "-o".to_owned(),
            "-".to_owned(),
        ]);
        for message in &options.messages {
            args.extend(["--message".to_owned(), message.clone()]);
        }
        for step in &options.steps {
            args.extend(["--step".to_owned(), step_arg(step)]);
        }
        if options.still {
            args.push("--still".to_owned());
        }
        args
    }
}

/// `step` as `rawr-play --step` takes it: the target's index, then for a
/// typed step `=` and the text.
#[must_use]
pub fn step_arg(step: &crate::Step) -> String {
    match &step.text {
        Some(text) => format!("{}={text}", step.target),
        None => step.target.to_string(),
    }
}

/// A step as [`step_arg`] writes it: `N`, or `N=TEXT` to type `TEXT`
/// into text input `N`.
pub fn parse_step(arg: &str) -> Result<crate::Step, String> {
    let (target, text) = match arg.split_once('=') {
        Some((target, text)) => (target, Some(text.to_owned())),
        None => (arg, None),
    };
    let target = target
        .parse::<usize>()
        .map_err(|_| format!("step `{arg}` must be a target's number, or N=TEXT"))?;
    Ok(crate::Step { target, text })
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
    /// Attach this image, with its note and its targets' buttons.
    Image(Shot),
    /// Send this text (already fenced and within [`MAX_REPLY`]).
    Text(String),
}

/// An encoded image `rawr-play` wrote, and what it said about it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Shot {
    /// The PNG, JPEG, WebP or GIF.
    pub bytes: Vec<u8>,
    /// The note to show beside it.
    pub note: Option<String>,
    /// A full app's targets in its last frame, in reading order.
    pub actions: Vec<crate::Action>,
    /// The label of the target each step clicked.
    pub clicked: Vec<String>,
}

/// What a finished `rawr-play` process gave: an iced snippet's image, or a
/// plain program's output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Done {
    /// The image, with what `rawr-play` said about it.
    Image(Shot),
    /// The program's output.
    Text(Output),
}

/// Decide the reply from a finished `rawr-play` process: the image or the
/// output on success, its error text on a clean failure, and a short
/// notice when it was killed (out of memory under `--memory-mb`, or a
/// crash).
#[must_use]
pub fn reply(success: bool, stdout: Vec<u8>, stderr: &str) -> Reply {
    match outcome(success, stdout, stderr) {
        Ok(Done::Image(shot)) => Reply::Image(shot),
        Ok(Done::Text(output)) => Reply::Text(output_text(&output)),
        Err(text) => Reply::Text(error_text(&text)),
    }
}

/// How `rawr-play` starts a line of stderr that notes something about the
/// image it wrote.
pub const NOTE: &str = "note: ";

/// How `rawr-play` starts a line naming a target of the image's last frame.
pub const ACTION: &str = "action: ";

/// How `rawr-play` starts a line naming a target of the image's last
/// frame that is a text input, which a step types into.
pub const INPUT: &str = "input: ";

/// How `rawr-play` starts a line naming the target a step clicked.
pub const CLICK: &str = "click: ";

/// The lines `rawr-play` writes beside an image: its note, then an
/// [`ACTION`] line for each target ([`INPUT`] for a text input) and a
/// [`CLICK`] line for each step.
#[must_use]
pub fn lines(note: Option<&str>, actions: &[crate::Action], clicked: &[String]) -> Vec<String> {
    let action = |action: &crate::Action| {
        let prefix = if action.input { INPUT } else { ACTION };
        format!("{prefix}{}", action.label)
    };
    note.map(|note| format!("{NOTE}{note}"))
        .into_iter()
        .chain(actions.iter().map(action))
        .chain(clicked.iter().map(|click| format!("{CLICK}{click}")))
        .collect()
}

/// The targets `rawr-play`'s [`ACTION`] and [`INPUT`] lines name, in
/// order.
fn actions(stderr: &str) -> Vec<crate::Action> {
    stderr
        .lines()
        .filter_map(|line| {
            let (label, input) = match line.strip_prefix(INPUT) {
                Some(label) => (label, true),
                None => (line.strip_prefix(ACTION)?, false),
            };
            let label = label.trim();
            (!label.is_empty()).then(|| crate::Action {
                label: label.to_owned(),
                input,
            })
        })
        .collect()
}

/// The text after `prefix` on each of `stderr`'s lines that start with it.
fn prefixed<'a>(stderr: &'a str, prefix: &'a str) -> impl Iterator<Item = &'a str> {
    stderr
        .lines()
        .filter_map(move |line| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// Whether `bytes` are a GIF.
#[must_use]
pub fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF8")
}

/// The note lines of a successful `rawr-play`'s stderr, joined.
fn note(stderr: &str) -> Option<String> {
    let notes: Vec<&str> = prefixed(stderr, NOTE).collect();
    (!notes.is_empty()).then(|| notes.join("\n"))
}

/// The result of a finished `rawr-play` process, or its error text
/// unfenced (as [`reply`] reads it; the playground shows it as is). A
/// result starting with `{` is a plain program's output.
pub fn outcome(success: bool, stdout: Vec<u8>, stderr: &str) -> Result<Done, String> {
    if success && stdout.first() == Some(&b'{') {
        return Output::from_json(&stdout).map(Done::Text);
    }
    if success && !stdout.is_empty() {
        if stdout.len() > MAX_ATTACHMENT {
            return Err("the image is too large to upload; try a smaller size".to_owned());
        }
        return Ok(Done::Image(Shot {
            bytes: stdout,
            note: note(stderr),
            actions: actions(stderr),
            clicked: prefixed(stderr, CLICK).map(str::to_owned).collect(),
        }));
    }
    let stderr = stderr.trim();
    if success || stderr.is_empty() || stderr.contains("memory allocation") {
        return Err("render stopped: out of memory or crashed".to_owned());
    }
    Err(sanitize(stderr))
}

/// The note on an image whose snippet loads `image` or `svg` from a path.
pub const NO_FILES: &str = "?play has no files. Paths given to image and svg do not exist here.";

/// [`NO_FILES`] when `code` loads an image or an svg from a path: it calls
/// `image("..")`, `svg("..")` or a `from_path`.
#[must_use]
pub fn image_note(code: &str) -> Option<&'static str> {
    let path = |name: &str| {
        code.match_indices(name).any(|(at, _)| {
            let before = code[..at].chars().next_back();
            let after = code[at + name.len()..].trim_start();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                && after
                    .strip_prefix('(')
                    .is_some_and(|rest| name == "from_path" || rest.trim_start().starts_with('"'))
        })
    };
    ["image", "svg", "from_path"]
        .into_iter()
        .any(path)
        .then_some(NO_FILES)
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
        channels, error_text, output_text, parse, reply, RateLimit, Reply, Request, Shot,
        MAX_ATTACHMENT, MAX_REPLY,
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
        assert!(!request.mine, "anyone may click by default");
        assert!(parsed("?play mine light\n```\nx\n```").mine);
        assert!(parsed("```rust ?play Mine\nx\n```").mine);
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
        let error = bad.unwrap_err();
        assert!(error.contains("unknown option"), "{error}");
        assert!(error.ends_with("msg=EXPR, mine"), "{error}");
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
        let mut request = parsed("?play 320x200 light msg=Message::Increment\n```\nx\n```");
        request.options.steps = vec![crate::Step::click(0), crate::Step::typed(2, "Ada = 1")];
        let args = request.args(Format::Webp, 512);
        assert_eq!(
            args.join(" "),
            "- --size 320x200 --theme light --format webp --time-ms 2000 --animation-ms 2000 \
             --memory-mb 512 -o - \
             --message Message::Increment --step 0 --step 2=Ada = 1"
        );
        assert_eq!(args.last().unwrap(), "2=Ada = 1", "one argument");
        for step in &request.options.steps {
            assert_eq!(super::parse_step(&super::step_arg(step)).as_ref(), Ok(step));
        }
        assert_eq!(super::parse_step("3="), Ok(crate::Step::typed(3, "")));
        assert!(super::parse_step("x=1").is_err());
        assert!(super::parse_step("-1").is_err());
    }

    #[test]
    fn message_reply_decision() {
        assert_eq!(
            reply(true, vec![1, 2, 3], ""),
            Reply::Image(Shot {
                bytes: vec![1, 2, 3],
                ..Shot::default()
            })
        );
        let noted = reply(
            true,
            b"GIF89a".to_vec(),
            "note: Animation shown for the first 5 s.\naction: Increment\ninput: Name\naction: Decrement\nclick: Increment\n",
        );
        let action = |label: &str, input| crate::Action {
            label: label.into(),
            input,
        };
        let actions = vec![
            action("Increment", false),
            action("Name", true),
            action("Decrement", false),
        ];
        assert_eq!(
            noted,
            Reply::Image(Shot {
                bytes: b"GIF89a".to_vec(),
                note: Some("Animation shown for the first 5 s.".into()),
                actions: actions.clone(),
                clicked: vec!["Increment".into()],
            })
        );
        assert_eq!(
            super::lines(None, &actions, &[]),
            ["action: Increment", "input: Name", "action: Decrement"],
            "the lines rawr-play writes read back"
        );
        assert!(super::is_gif(b"GIF89a") && !super::is_gif(b"\x89PNG"));
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
    fn message_reply_for_a_plain_program() {
        use crate::plain::Stream::{Stderr, Stdout};
        let output = crate::Output {
            chunks: vec![(Stdout, "total: 27\n".into())],
            ..crate::Output::default()
        };
        assert_eq!(
            reply(true, output.to_json().into_bytes(), ""),
            Reply::Text("```text\ntotal: 27\n```".into())
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
            note: Some(crate::plain::MESSAGES_IGNORED.into()),
            ..output.clone()
        };
        assert_eq!(
            output_text(&noted),
            "```text\ntotal: 27\n```\n-# messages are ignored for programs without iced"
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
    fn image_note_names_paths() {
        use super::{image_note, NO_FILES};
        assert_eq!(image_note(r#"image("67.png")"#), Some(NO_FILES));
        assert_eq!(
            image_note(r#"svg("Ivan the Terrible.svg")"#),
            Some(NO_FILES)
        );
        assert_eq!(
            image_note(r#"image(image::Handle::from_path(p))"#),
            Some(NO_FILES)
        );
        assert_eq!(
            image_note(r#"column![text("an image"), my_image("x")]"#),
            None
        );
        assert_eq!(image_note(r#"image(handle)"#), None);
    }

    #[test]
    fn crash_text_has_no_server_paths() {
        let stderr = "\nthread 'main' (17) panicked at /usr/local/cargo/git/checkouts/iced-3e930cb38a75fe30/be88873/tiny_skia/src/raster.rs:138:14:\n\
                      Image should be allocated\n\
                      note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n";
        let Reply::Text(text) = reply(false, Vec::new(), stderr) else {
            panic!("a crash is text");
        };
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
