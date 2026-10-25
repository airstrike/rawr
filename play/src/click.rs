//! Clicks on a `?play` reply's buttons (D516), as pure decisions for the
//! Discord bot.
//!
//! A full app's reply has a button for each target of its last frame, up to
//! [`MAX_BUTTONS`]. A button carries the whole history in its `custom_id`,
//! so a click needs no store and survives a restart: [`PREFIX`] (the
//! format's version), a short [`hash`] of the request, then one base-62
//! digit per step, the button's own target last. A click replays the
//! request with its messages and those steps.
//!
//! ```text
//! p1Xb3K001
//! ^^         version
//!   ^^^^     hash of the request's code and options
//!       ^^^  steps: target 0, target 0, target 1
//! ```

use std::time::Duration;

use crate::message::Request;

/// How every button's `custom_id` starts: `p` for `?play`, then the
/// format's version.
pub const PREFIX: &str = "p1";

/// Most buttons on a reply: four rows of five.
pub const MAX_BUTTONS: usize = 20;

/// Most steps a history holds. A reply at the cap offers no buttons.
pub const MAX_STEPS: usize = 24;

/// Longest label Discord shows on a button.
pub const MAX_LABEL: usize = 80;

/// Characters of [`hash`].
const HASH: usize = 4;

/// Base-62 digits, by value.
const DIGITS: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// What a stale button's click is told, privately.
pub const STALE: &str = "This render is out of date. Run it again.";

/// What a click from someone other than the author of a `mine` request is
/// told.
pub const NOT_YOURS: &str =
    "This render says mine, so only its author can click it. Post your own ?play to try it.";

/// The line that replaces the buttons once the history is full.
pub const FULL: &str = "Edit the message to start over.";

/// The text a click over the rate limits is told: when to try again.
#[must_use]
pub fn wait_text(wait: Duration) -> String {
    let seconds = wait.as_millis().div_ceil(1000).max(1);
    format!("Too many renders. Try again in {seconds} s.")
}

/// A short hash of what `request` renders: its code, size, theme and
/// messages. A button made for another request (its message was edited
/// since) has another hash. FNV-1a, so it is the same in every build.
/// `mine` changes who may click, not the image, so [`decide`] reads it
/// from the request as it is now instead.
#[must_use]
pub fn hash(request: &Request) -> String {
    let options = &request.options;
    let size = options
        .size
        .map_or_else(String::new, |(width, height)| format!("{width}x{height}"));
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let parts = [request.code.as_str(), &size, options.theme.variant()]
        .into_iter()
        .chain(options.messages.iter().map(String::as_str));
    for part in parts {
        for byte in part.bytes().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    (0..HASH)
        .map(|_| {
            let digit = DIGITS[(hash % 62) as usize];
            hash /= 62;
            char::from(digit)
        })
        .collect()
}

/// A click's `custom_id`: [`PREFIX`], `hash`, then `steps` in base 62.
/// Each step is below [`MAX_BUTTONS`], so one digit each.
#[must_use]
pub fn encode(hash: &str, steps: &[usize]) -> String {
    let steps = steps.iter().map(|&step| char::from(DIGITS[step % 62]));
    PREFIX.chars().chain(hash.chars()).chain(steps).collect()
}

/// A decoded `custom_id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Click {
    /// The [`hash`] of the request its buttons were made for.
    pub hash: String,
    /// The history, the clicked button's target last.
    pub steps: Vec<usize>,
}

/// `id` read as [`encode`] writes it; `None` for another format or
/// version, or a history past [`MAX_STEPS`].
#[must_use]
pub fn decode(id: &str) -> Option<Click> {
    let rest = id.strip_prefix(PREFIX)?;
    let hash = rest.get(..HASH)?;
    let digit = |byte: u8| DIGITS.iter().position(|&d| d == byte);
    if !hash.bytes().all(|byte| digit(byte).is_some()) {
        return None;
    }
    let steps = rest[HASH..]
        .bytes()
        .map(digit)
        .collect::<Option<Vec<usize>>>()?;
    (!steps.is_empty() && steps.len() <= MAX_STEPS).then(|| Click {
        hash: hash.to_owned(),
        steps,
    })
}

/// What the bot does with a click.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    /// The button is not for the request as it is now: tell the clicker
    /// [`STALE`], privately.
    Stale,
    /// Someone other than the author clicked a `mine` request's button:
    /// tell them [`NOT_YOURS`].
    NotYours,
    /// Over the per-user or per-channel limit: tell them [`wait_text`].
    Wait(Duration),
    /// A click on this reply is rendering: acknowledge and drop this one.
    Busy,
    /// Acknowledge, render the request with these steps, and edit the reply.
    Render(Vec<usize>),
}

/// Decide a click on button `id` by `clicker`, in order: the hash check
/// against `request` (its [`hash`], and its author when it says `mine`;
/// `None` when it is gone or no longer a `?play`), the author check, then
/// `allow` (the clicker's rate limits: `Some(wait)` when over) and `claim`
/// (`false` while a click on this reply renders). Anyone may click a
/// request without `mine`. `allow` runs only for current buttons the
/// clicker may press, and `claim` only once `allow` passed, so a refused
/// click costs nothing.
pub fn decide(
    id: &str,
    request: Option<(&str, Option<u64>)>,
    clicker: u64,
    allow: impl FnOnce() -> Option<Duration>,
    claim: impl FnOnce() -> bool,
) -> Decision {
    let Some((hash, author)) = request else {
        return Decision::Stale;
    };
    let Some(click) = decode(id).filter(|click| click.hash == hash) else {
        return Decision::Stale;
    };
    if author.is_some_and(|author| author != clicker) {
        return Decision::NotYours;
    }
    if let Some(wait) = allow() {
        return Decision::Wait(wait);
    }
    if !claim() {
        return Decision::Busy;
    }
    Decision::Render(click.steps)
}

/// One button: its `custom_id` and label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button {
    /// [`encode`]d: the history so far, then this button's target.
    pub id: String,
    /// The target's label, cut to [`MAX_LABEL`] characters.
    pub label: String,
}

/// The buttons for a reply to the request hashed `hash`, rendered after
/// `steps`, whose last frame has `actions`: one per action, up to
/// [`MAX_BUTTONS`]. None once `steps` reaches [`MAX_STEPS`]; see [`quiet`].
#[must_use]
pub fn buttons(hash: &str, steps: &[usize], actions: &[String]) -> Vec<Button> {
    if steps.len() >= MAX_STEPS {
        return Vec::new();
    }
    actions
        .iter()
        .take(MAX_BUTTONS)
        .enumerate()
        .map(|(target, label)| {
            let mut history = steps.to_vec();
            history.push(target);
            Button {
                id: encode(hash, &history),
                label: cut(label, MAX_LABEL),
            }
        })
        .collect()
}

/// The quiet lines under a clicked reply: the clicks so far, and once the
/// history is full with targets left, [`FULL`].
#[must_use]
pub fn quiet(clicked: &[String], actions: &[String]) -> Vec<String> {
    /// Characters of each label in the list of clicks.
    const LABEL: usize = 24;
    /// The most recent clicks shown; earlier ones become a leading `…`.
    const SHOWN: usize = 5;
    let mut lines = Vec::new();
    if !clicked.is_empty() {
        let recent = &clicked[clicked.len().saturating_sub(SHOWN)..];
        let mut list: Vec<String> = recent.iter().map(|label| cut(label, LABEL)).collect();
        if recent.len() < clicked.len() {
            list.insert(0, "…".to_owned());
        }
        lines.push(format!("Clicked: {}", list.join(", ")));
    }
    if clicked.len() >= MAX_STEPS && !actions.is_empty() {
        lines.push(FULL.to_owned());
    }
    lines
}

/// `text` within `max` characters, with `…` when cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max - 1).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        buttons, decide, decode, encode, hash, quiet, wait_text, Click, Decision, FULL,
        MAX_BUTTONS, MAX_STEPS, PREFIX,
    };
    use crate::message::parse;
    use crate::Options;

    fn request(content: &str) -> crate::message::Request {
        parse(content, &Options::default()).unwrap().unwrap()
    }

    #[test]
    fn click_ids_round_trip_within_discords_limit() {
        let steps = vec![0, 0, 1, 19, 7];
        let id = encode("Xb3K", &steps);
        assert_eq!(id, "p1Xb3K001j7");
        assert_eq!(
            decode(&id),
            Some(Click {
                hash: "Xb3K".into(),
                steps
            })
        );
        let longest = encode("Xb3K", &[MAX_BUTTONS - 1; MAX_STEPS]);
        assert!(longest.len() <= 100, "{}", longest.len());
        assert_eq!(decode(&longest).unwrap().steps.len(), MAX_STEPS);
    }

    #[test]
    fn click_ids_of_another_version_or_shape_do_not_decode() {
        assert_eq!(decode("p0Xb3K0"), None, "another version");
        assert_eq!(decode("p1Xb3K"), None, "no step");
        assert_eq!(decode("p1Xb"), None, "a short hash");
        assert_eq!(decode("p1Xb3K0-"), None, "not base 62");
        assert_eq!(decode("p1Xb3K\u{e9}"), None, "not ASCII");
        let over = encode("Xb3K", &[0; MAX_STEPS + 1]);
        assert_eq!(decode(&over), None, "past the cap");
        assert!(encode("abcd", &[1]).starts_with(PREFIX));
    }

    #[test]
    fn the_hash_follows_the_requests_code_and_options() {
        let code = "?play\n```rust\ncolumn![text(\"hi\")]\n```";
        let base = hash(&request(code));
        assert_eq!(base.len(), 4);
        assert!(base.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(base, hash(&request(code)), "the same request");
        assert_eq!(
            base,
            hash(&request(
                "?play\n```rust\ncolumn![text(\"hi\")]\n```\nlook at this"
            )),
            "text outside the code block"
        );
        for other in [
            "?play\n```rust\ncolumn![text(\"ho\")]\n```",
            "?play light\n```rust\ncolumn![text(\"hi\")]\n```",
            "?play 200x100\n```rust\ncolumn![text(\"hi\")]\n```",
            "?play msg=Message::Go\n```rust\ncolumn![text(\"hi\")]\n```",
        ] {
            assert_ne!(base, hash(&request(other)), "{other}");
        }
        assert_eq!(
            base,
            hash(&request("?play mine\n```rust\ncolumn![text(\"hi\")]\n```")),
            "mine changes who may click, not the image"
        );
    }

    #[test]
    fn a_click_is_checked_in_order() {
        const AUTHOR: u64 = 7;
        const OTHER: u64 = 8;
        let id = encode("Xb3K", &[0, 1]);
        let current = Some(("Xb3K", Some(AUTHOR)));
        let never = || -> Option<Duration> { panic!("not reached") };
        let unclaimed = || -> bool { panic!("not reached") };
        assert_eq!(
            decide(&id, None, AUTHOR, never, unclaimed),
            Decision::Stale,
            "the request is gone"
        );
        assert_eq!(
            decide(&id, Some(("Zzzz", None)), OTHER, never, unclaimed),
            Decision::Stale,
            "the request was edited"
        );
        assert_eq!(
            decide("p0Xb3K0", current, AUTHOR, never, unclaimed),
            Decision::Stale,
            "an older format"
        );
        assert_eq!(
            decide(&id, current, OTHER, never, unclaimed),
            Decision::NotYours,
            "a mine request"
        );
        let wait = Duration::from_millis(41_200);
        assert_eq!(
            decide(&id, current, AUTHOR, || Some(wait), unclaimed),
            Decision::Wait(wait)
        );
        assert_eq!(
            decide(&id, current, AUTHOR, || None, || false),
            Decision::Busy
        );
        assert_eq!(
            decide(&id, current, AUTHOR, || None, || true),
            Decision::Render(vec![0, 1])
        );
        let anyone = Some(("Xb3K", None));
        assert_eq!(
            decide(&id, anyone, OTHER, || Some(wait), unclaimed),
            Decision::Wait(wait),
            "anyone may click, within their own limits"
        );
        assert_eq!(
            decide(&id, anyone, OTHER, || None, || false),
            Decision::Busy,
            "one click at a time"
        );
        assert_eq!(
            decide(&id, anyone, OTHER, || None, || true),
            Decision::Render(vec![0, 1])
        );
        assert_eq!(wait_text(wait), "Too many renders. Try again in 42 s.");
        assert_eq!(
            wait_text(Duration::ZERO),
            "Too many renders. Try again in 1 s."
        );
    }

    #[test]
    fn buttons_carry_the_history_and_stop_at_the_cap() {
        let actions: Vec<String> = (0..25).map(|n| format!("item {n}")).collect();
        let made = buttons("Xb3K", &[3], &actions);
        assert_eq!(made.len(), MAX_BUTTONS);
        assert_eq!(made[0].id, "p1Xb3K30");
        assert_eq!(made[0].label, "item 0");
        assert_eq!(decode(&made[19].id).unwrap().steps, [3, 19]);
        let long = ["x".repeat(100)];
        let label = &buttons("Xb3K", &[], &long)[0].label;
        assert_eq!(label.chars().count(), 80);
        assert!(label.ends_with('…'));
        let full = vec![0; MAX_STEPS];
        assert!(buttons("Xb3K", &full, &actions).is_empty());
    }

    #[test]
    fn the_quiet_lines_list_the_clicks_and_the_cap() {
        let actions = vec!["Increment".to_owned(), "Decrement".to_owned()];
        assert!(quiet(&[], &actions).is_empty());
        let clicked = vec!["Increment".to_owned(), "Decrement".to_owned()];
        assert_eq!(quiet(&clicked, &actions), ["Clicked: Increment, Decrement"]);
        let many: Vec<String> = ["Previous", "Shout", "Next", "Next", "Next", "Next"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            quiet(&many, &actions),
            ["Clicked: …, Shout, Next, Next, Next, Next"]
        );
        let full = vec!["Increment".to_owned(); MAX_STEPS];
        let lines = quiet(&full, &actions);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], FULL);
    }
}
