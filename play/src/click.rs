//! Clicks on a `?play` reply's buttons (D516), as pure decisions for the
//! Discord bot.
//!
//! A full app's reply has a button for each target of its last frame, up to
//! [`MAX_BUTTONS`], and a select menu ([`MORE`]) holding the next
//! [`MAX_OPTIONS`]. A button carries the whole history in its `custom_id`,
//! and an option in its value, so a click needs no store and survives a
//! restart: [`PREFIX`] (the format's version), a short [`hash`] of the
//! request, then one base-62 digit per step, the button's own target last.
//! A click, or a pick from the menu, replays the request with its messages
//! and those steps.
//!
//! ```text
//! p1Xb3K001
//! ^^         version
//!   ^^^^     hash of the request's code and options
//!       ^^^  steps: target 0, target 0, target 1
//! ```
//!
//! A text input's button ends in [`OPENS`]: a click on it opens a modal
//! instead of rendering. The modal's `custom_id` is the button's, and its
//! submitted text joins the history as a typed step: the target's digit,
//! [`TYPED`], one base-62 digit for the text's length in characters, then
//! the text itself. The text rides in every later button's `custom_id`, so
//! it is still stateless, and [`room`] keeps every ID within [`MAX_ID`].
//!
//! ```text
//! p1Xb3K02?        target 2 is a text input: open the modal
//! p1Xb3K02~3Ada1   typed "Ada" into target 2, then clicked target 1
//! ```

use std::time::Duration;

use crate::message::Request;
use crate::{Action, Step};

/// How every button's `custom_id` starts: `p` for `?play`, then the
/// format's version.
pub const PREFIX: &str = "p1";

/// Most buttons on a reply: four rows of five. The fifth row holds the
/// select menu.
pub const MAX_BUTTONS: usize = 20;

/// Most options in the select menu: Discord's limit for one menu.
pub const MAX_OPTIONS: usize = 25;

/// Most targets a reply offers: the buttons, then the menu's options.
pub const MAX_TARGETS: usize = MAX_BUTTONS + MAX_OPTIONS;

/// The select menu's `custom_id`. Each option's value is the `custom_id`
/// a button for its target would have, so a pick is read as that click
/// (see [`chosen`]).
pub const MORE: &str = "p1more";

/// Most steps a history holds. A reply at the cap offers no buttons.
pub const MAX_STEPS: usize = 24;

/// Longest label Discord shows on a button.
pub const MAX_LABEL: usize = 80;

/// Longest `custom_id` Discord takes, and longest option value, counted
/// in UTF-16 code units as Discord's clients count them.
pub const MAX_ID: usize = 100;

/// Longest text one typed step holds: its length is one base-62 digit.
pub const MAX_TEXT: usize = 61;

/// Longest title of a modal, and longest label of its text input.
pub const MAX_TITLE: usize = 45;

/// Marks a typed step's text in a `custom_id`; see the module's
/// documentation.
pub const TYPED: char = '~';

/// Ends a text input's button's `custom_id`: a click opens the modal.
pub const OPENS: char = '?';

/// The `custom_id` of the modal's text input.
pub const TEXT: &str = "text";

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

/// The base-62 digit for `value`, below 62.
fn digit(value: usize) -> char {
    char::from(DIGITS[value % 62])
}

/// The value of base-62 digit `digit`.
fn value(digit: char) -> Option<usize> {
    u8::try_from(digit)
        .ok()
        .and_then(|byte| DIGITS.iter().position(|&d| d == byte))
}

/// A click's `custom_id`: [`PREFIX`], `hash`, then `steps`: each one
/// base-62 digit (each target is below [`MAX_TARGETS`]), and a typed
/// step's text after [`TYPED`] and its length. A text is cut to
/// [`MAX_TEXT`] characters.
#[must_use]
pub fn encode(hash: &str, steps: &[Step]) -> String {
    let mut id: String = PREFIX.chars().chain(hash.chars()).collect();
    for step in steps {
        id.push(digit(step.target));
        if let Some(text) = &step.text {
            let text: String = text.chars().take(MAX_TEXT).collect();
            id.push(TYPED);
            id.push(digit(text.chars().count()));
            id.push_str(&text);
        }
    }
    id
}

/// A decoded `custom_id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Click {
    /// The [`hash`] of the request its buttons were made for.
    pub hash: String,
    /// The history, the clicked button's target last.
    pub steps: Vec<Step>,
    /// Whether it ends in [`OPENS`]: its last step is a text input whose
    /// modal it opens, not a step to render.
    pub opens: bool,
}

/// `id` read as [`encode`] writes it, with [`OPENS`] when it ends in one;
/// `None` for another format or version, or a history past [`MAX_STEPS`].
#[must_use]
pub fn decode(id: &str) -> Option<Click> {
    let rest = id.strip_prefix(PREFIX)?;
    let hash = rest.get(..HASH)?;
    if !hash.chars().all(|c| value(c).is_some()) {
        return None;
    }
    let mut chars = rest[HASH..].chars().peekable();
    let mut steps = Vec::new();
    let mut opens = false;
    while let Some(next) = chars.next() {
        if next == OPENS && chars.peek().is_none() {
            opens = steps.last().is_some_and(|step: &Step| step.text.is_none());
            if !opens {
                return None;
            }
            break;
        }
        let mut step = Step::click(value(next)?);
        if chars.next_if_eq(&TYPED).is_some() {
            let length = value(chars.next()?)?;
            let text = (0..length)
                .map(|_| chars.next())
                .collect::<Option<String>>()?;
            step.text = Some(text);
        }
        steps.push(step);
    }
    (!steps.is_empty() && steps.len() <= MAX_STEPS).then(|| Click {
        hash: hash.to_owned(),
        steps,
        opens,
    })
}

/// The length Discord counts for a `custom_id`: UTF-16 code units.
fn length(id: &str) -> usize {
    id.encode_utf16().count()
}

/// The most characters the modal opened by `opens` (a text input's
/// button's `custom_id`) takes, so the typed step and one more step after
/// it, a text input's button too, stay within [`MAX_ID`]; at most
/// [`MAX_TEXT`]. Zero when there is no room.
#[must_use]
pub fn room(opens: &str) -> usize {
    // The typed step trades `OPENS` for `TYPED` and a length digit, and a
    // later button adds a target's digit and maybe `OPENS`.
    MAX_ID.saturating_sub(length(opens) + 3).min(MAX_TEXT)
}

/// The `custom_id` of the typed step a modal opened by `opens` stands for
/// once `text` is submitted: `None` when `opens` doesn't end in [`OPENS`].
/// `text` is cut to [`room`] characters.
#[must_use]
pub fn typed(opens: &str, text: &str) -> Option<String> {
    let base = opens.strip_suffix(OPENS)?;
    let text: String = text.chars().take(room(opens)).collect();
    Some(format!(
        "{base}{TYPED}{}{text}",
        digit(text.chars().count())
    ))
}

/// The `custom_id` a component interaction stands for: a button's own,
/// or for a pick from the [`MORE`] menu, the picked option's value.
#[must_use]
pub fn chosen<'a>(custom_id: &'a str, values: &'a [String]) -> &'a str {
    match values {
        [value, ..] if custom_id == MORE => value,
        _ => custom_id,
    }
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
    /// A text input's button: open the modal, whose text takes this many
    /// characters at most (see [`room`]). Its submission is decided again
    /// as the [`typed`] step.
    Ask(usize),
    /// Over the per-user or per-channel limit: tell them [`wait_text`].
    Wait(Duration),
    /// A click on this reply is rendering: acknowledge and drop this one.
    Busy,
    /// Acknowledge, render the request with these steps, and edit the reply.
    Render(Vec<Step>),
}

/// Decide a click on button `id` by `clicker`, in order: the hash check
/// against `request` (its [`hash`], and its author when it says `mine`;
/// `None` when it is gone or no longer a `?play`), the author check, a
/// text input's modal, then `allow` (the clicker's rate limits:
/// `Some(wait)` when over) and `claim` (`false` while a click on this
/// reply renders). Anyone may click a request without `mine`. `allow`
/// runs only for current buttons the clicker may press, and `claim` only
/// once `allow` passed, so a refused click costs nothing. Opening a modal
/// renders nothing, so it counts against neither.
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
    if click.opens {
        return Decision::Ask(room(id));
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
    /// [`encode`]d: the history so far, then this button's target, and
    /// [`OPENS`] for a text input.
    pub id: String,
    /// The target's label, cut to [`MAX_LABEL`] characters.
    pub label: String,
    /// Whether it is a text input's, which opens a modal.
    pub input: bool,
}

/// The buttons for a reply to the request hashed `hash`, rendered after
/// `steps`, whose last frame has `actions`: one per action, up to
/// [`MAX_TARGETS`]. The first [`MAX_BUTTONS`] are shown as buttons and the
/// rest as the [`MORE`] menu's options. None once `steps` reaches
/// [`MAX_STEPS`]. A button whose ID would pass [`MAX_ID`], or a text
/// input's with no [`room`] left for text, is left out; see [`quiet`].
#[must_use]
pub fn buttons(hash: &str, steps: &[Step], actions: &[Action]) -> Vec<Button> {
    if steps.len() >= MAX_STEPS {
        return Vec::new();
    }
    let history = encode(hash, steps);
    actions
        .iter()
        .take(MAX_TARGETS)
        .enumerate()
        .map(|(target, action)| {
            let mut id = history.clone();
            id.push(digit(target));
            if action.input {
                id.push(OPENS);
            }
            Button {
                id,
                label: cut(&action.label, MAX_LABEL),
                input: action.input,
            }
        })
        .filter(|button| length(&button.id) <= MAX_ID && (!button.input || room(&button.id) > 0))
        .collect()
}

/// The title of the modal a text input's button labelled `label` opens.
#[must_use]
pub fn title(label: &str) -> String {
    cut(&format!("Type into {label}"), MAX_TITLE)
}

/// The quiet lines under a clicked reply: the clicks so far, and when
/// the history is `full` (no button fits while targets are left),
/// [`FULL`].
#[must_use]
pub fn quiet(clicked: &[String], full: bool) -> Vec<String> {
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
    if full {
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
        buttons, chosen, decide, decode, encode, hash, quiet, room, title, typed, wait_text, Click,
        Decision, FULL, MAX_BUTTONS, MAX_ID, MAX_STEPS, MAX_TARGETS, MAX_TEXT, MORE, PREFIX,
    };
    use crate::message::parse;
    use crate::{Action, Options, Step};

    fn request(content: &str) -> crate::message::Request {
        parse(content, &Options::default()).unwrap().unwrap()
    }

    /// Clicks on targets `targets`, in order.
    fn clicks(targets: &[usize]) -> Vec<Step> {
        targets.iter().copied().map(Step::click).collect()
    }

    /// Targets labelled `labels`, none of them a text input.
    fn actions(labels: impl IntoIterator<Item = String>) -> Vec<Action> {
        labels
            .into_iter()
            .map(|label| Action {
                label,
                input: false,
            })
            .collect()
    }

    #[test]
    fn click_ids_round_trip_within_discords_limit() {
        let steps = clicks(&[0, 0, 1, 19, 7]);
        let id = encode("Xb3K", &steps);
        assert_eq!(id, "p1Xb3K001j7");
        assert_eq!(
            decode(&id),
            Some(Click {
                hash: "Xb3K".into(),
                steps,
                opens: false,
            })
        );
        let longest = encode("Xb3K", &clicks(&[MAX_TARGETS - 1; MAX_STEPS]));
        assert!(longest.len() <= MAX_ID, "{}", longest.len());
        assert_eq!(decode(&longest).unwrap().steps.len(), MAX_STEPS);
    }

    #[test]
    fn click_ids_of_another_version_or_shape_do_not_decode() {
        assert_eq!(decode("p0Xb3K0"), None, "another version");
        assert_eq!(decode("p1Xb3K"), None, "no step");
        assert_eq!(decode("p1Xb"), None, "a short hash");
        assert_eq!(decode("p1Xb3K0-"), None, "not base 62");
        assert_eq!(decode("p1Xb3K\u{e9}"), None, "not ASCII");
        assert_eq!(
            decode("p1Xb3K0~5ab"),
            None,
            "a text shorter than its length"
        );
        assert_eq!(decode("p1Xb3K0~"), None, "a text with no length");
        assert_eq!(decode("p1Xb3K?"), None, "nothing to open");
        assert_eq!(decode("p1Xb3K0~1a?"), None, "a typed step opens nothing");
        let over = encode("Xb3K", &clicks(&[0; MAX_STEPS + 1]));
        assert_eq!(decode(&over), None, "past the cap");
        assert!(encode("abcd", &clicks(&[1])).starts_with(PREFIX));
    }

    #[test]
    fn typed_text_rides_in_the_id() {
        let steps = vec![
            Step::click(0),
            Step::typed(2, "Ada ~? é"),
            Step::click(1),
            Step::typed(3, ""),
        ];
        let id = encode("Xb3K", &steps);
        assert_eq!(id, "p1Xb3K02~8Ada ~? é13~0");
        assert_eq!(decode(&id).unwrap().steps, steps, "markers inside a text");
        // A text input's button opens the modal; the submission is the
        // typed step.
        let opens = "p1Xb3K02?";
        let click = decode(opens).unwrap();
        assert!(click.opens);
        assert_eq!(click.steps, clicks(&[0, 2]));
        let id = typed(opens, "Ada").unwrap();
        assert_eq!(id, "p1Xb3K02~3Ada");
        let click = decode(&id).unwrap();
        assert!(!click.opens);
        assert_eq!(click.steps, [Step::click(0), Step::typed(2, "Ada")]);
        assert_eq!(typed("p1Xb3K02", "Ada"), None, "not a text input's button");
        // The modal takes what still fits, and a text past it is cut.
        assert_eq!(room(opens), MAX_TEXT);
        let long = format!("p1Xb3K0~Z{}2?", "x".repeat(61));
        assert_eq!(room(&long), MAX_ID - long.len() - 3);
        let cut = typed(&long, &"y".repeat(MAX_TEXT)).unwrap();
        assert_eq!(cut.len() + 2, MAX_ID, "room for one more button");
        assert_eq!(
            decode(&cut).unwrap().steps[1]
                .text
                .as_deref()
                .unwrap()
                .len(),
            room(&long)
        );
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
        let id = encode("Xb3K", &clicks(&[0, 1]));
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
        assert_eq!(
            decide("p1Xb3K02?", current, OTHER, never, unclaimed),
            Decision::NotYours,
            "a mine request's text input"
        );
        assert_eq!(
            decide("p1Xb3K02?", current, AUTHOR, never, unclaimed),
            Decision::Ask(MAX_TEXT),
            "a modal costs nothing"
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
            Decision::Render(clicks(&[0, 1]))
        );
        let submitted = typed("p1Xb3K02?", "Ada").unwrap();
        assert_eq!(
            decide(&submitted, current, AUTHOR, || Some(wait), unclaimed),
            Decision::Wait(wait),
            "a submitted text is a run"
        );
        assert_eq!(
            decide(&submitted, current, AUTHOR, || None, || true),
            Decision::Render(vec![Step::click(0), Step::typed(2, "Ada")])
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
            Decision::Render(clicks(&[0, 1]))
        );
        assert_eq!(wait_text(wait), "Too many renders. Try again in 42 s.");
        assert_eq!(
            wait_text(Duration::ZERO),
            "Too many renders. Try again in 1 s."
        );
    }

    #[test]
    fn buttons_carry_the_history_and_stop_at_the_cap() {
        let actions = actions((0..25).map(|n| format!("item {n}")));
        let made = buttons("Xb3K", &clicks(&[3]), &actions);
        assert_eq!(made.len(), 25);
        assert_eq!(made[0].id, "p1Xb3K30");
        assert_eq!(made[0].label, "item 0");
        assert_eq!(decode(&made[19].id).unwrap().steps, clicks(&[3, 19]));
        assert_eq!(decode(&made[24].id).unwrap().steps, clicks(&[3, 24]));
        let long = self::actions(["x".repeat(100)]);
        let label = &buttons("Xb3K", &[], &long)[0].label;
        assert_eq!(label.chars().count(), 80);
        assert!(label.ends_with('…'));
        let full = clicks(&[0; MAX_STEPS]);
        assert!(buttons("Xb3K", &full, &actions).is_empty());
    }

    #[test]
    fn a_text_inputs_button_opens_a_modal_while_text_fits() {
        let mut targets = actions(["Go".to_owned()]);
        targets.push(Action {
            label: "Name".into(),
            input: true,
        });
        let made = buttons("Xb3K", &[], &targets);
        assert_eq!(made[1].id, "p1Xb3K1?");
        assert!(made[1].input && !made[0].input);
        assert_eq!(decode(&made[1].id).unwrap().steps, clicks(&[1]));
        // After a long text, only buttons that fit are left, and a text
        // input's only while some text still fits.
        let long = vec![Step::typed(1, "x".repeat(MAX_TEXT)), Step::click(0)];
        let made = buttons("Xb3K", &long, &targets);
        assert!(made.iter().all(|button| button.id.len() <= MAX_ID));
        assert_eq!(made.len(), 2, "{made:?}");
        let longer = vec![
            Step::typed(1, "x".repeat(MAX_TEXT)),
            Step::typed(1, "y".repeat(22)),
        ];
        let made = buttons("Xb3K", &longer, &targets);
        assert_eq!(made.len(), 1, "no room left to type: {made:?}");
        assert!(!made[0].input);
        assert_eq!(made[0].id.len(), MAX_ID - 4);
        assert_eq!(title("Name"), "Type into Name");
        assert_eq!(title(&"n".repeat(60)).chars().count(), 45);
    }

    #[test]
    fn past_the_buttons_a_menu_holds_the_rest() {
        let actions = actions((0..60).map(|n| format!("item {n}")));
        let made = buttons("Xb3K", &[], &actions);
        assert_eq!(made.len(), MAX_TARGETS, "twenty buttons and a menu of 25");
        let options = &made[MAX_BUTTONS..];
        assert_eq!(options[0].label, "item 20");
        assert_eq!(decode(&options[24].id).unwrap().steps, clicks(&[44]));
        assert!(made.iter().all(|button| button.id.len() <= MAX_ID));
        // A pick is read as the click its option's value names.
        let picked = vec![options[3].id.clone()];
        assert_eq!(chosen(MORE, &picked), "p1Xb3Kn");
        assert_eq!(decode(chosen(MORE, &picked)).unwrap().steps, clicks(&[23]));
        assert_eq!(chosen("p1Xb3K0", &[]), "p1Xb3K0", "a button");
        assert_eq!(chosen(MORE, &[]), MORE, "a menu with nothing picked");
        assert_eq!(decode(MORE), None, "the menu's own ID is no click");
    }

    #[test]
    fn the_quiet_lines_list_the_clicks_and_the_cap() {
        assert!(quiet(&[], false).is_empty());
        let clicked = vec!["Increment".to_owned(), "Name: \"Ada\"".to_owned()];
        assert_eq!(
            quiet(&clicked, false),
            ["Clicked: Increment, Name: \"Ada\""]
        );
        let many: Vec<String> = ["Previous", "Shout", "Next", "Next", "Next", "Next"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            quiet(&many, false),
            ["Clicked: …, Shout, Next, Next, Next, Next"]
        );
        let full = vec!["Increment".to_owned(); MAX_STEPS];
        let lines = quiet(&full, true);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], FULL);
    }
}
