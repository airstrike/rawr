//! One Cargo build's JSON messages (`--message-format=json`), each line
//! decoded once into the fields rawr reads, borrowed from the build's
//! output where its text needs no unescaping.

use std::borrow::Cow;

use serde::Deserialize;

/// The messages of one build, in Cargo's order.
pub(crate) struct Messages<'a> {
    messages: Vec<Message<'a>>,
    /// Whether a line was not a message of the shape these fields take.
    malformed: bool,
}

/// One message's fields that rawr reads. Cargo leaves out what a reason
/// doesn't carry, so each is optional.
#[derive(Deserialize)]
pub(crate) struct Message<'a> {
    #[serde(borrow, default)]
    reason: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    package_id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    manifest_path: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    target: Option<Target<'a>>,
    #[serde(default)]
    fresh: Option<bool>,
    #[serde(borrow, default)]
    filenames: Vec<Cow<'a, str>>,
    #[serde(borrow, default)]
    executable: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    out_dir: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    linked_libs: Vec<Cow<'a, str>>,
    #[serde(borrow, default)]
    cfgs: Option<Vec<Cow<'a, str>>>,
    #[serde(borrow, default)]
    env: Option<Vec<(Cow<'a, str>, Cow<'a, str>)>>,
}

/// The target a message is for.
#[derive(Deserialize)]
struct Target<'a> {
    #[serde(borrow, default)]
    kind: Vec<Cow<'a, str>>,
}

impl<'a> Messages<'a> {
    /// The messages of `output`, one to a line. A line that is not one is
    /// left out, and [`Messages::is_complete`] says so.
    #[must_use]
    pub(crate) fn parse(output: &'a [u8]) -> Self {
        let mut messages = Vec::new();
        let mut malformed = false;
        for line in output.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<Message<'a>>(line) {
                Ok(message) => messages.push(message),
                Err(_) => malformed = true,
            }
        }
        Self {
            messages,
            malformed,
        }
    }

    /// Whether every line was a message.
    pub(crate) const fn is_complete(&self) -> bool {
        !self.malformed
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Message<'a>> {
        self.messages.iter()
    }
}

impl Message<'_> {
    pub(crate) fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub(crate) fn package_id(&self) -> Option<&str> {
        self.package_id.as_deref()
    }

    pub(crate) fn manifest_path(&self) -> Option<&str> {
        self.manifest_path.as_deref()
    }

    /// The first kind of the message's target, as `lib` or `custom-build`.
    pub(crate) fn target_kind(&self) -> Option<&str> {
        self.target.as_ref()?.kind.first().map(Cow::as_ref)
    }

    pub(crate) const fn fresh(&self) -> Option<bool> {
        self.fresh
    }

    pub(crate) fn executable(&self) -> Option<&str> {
        self.executable.as_deref()
    }

    pub(crate) fn out_dir(&self) -> Option<&str> {
        self.out_dir.as_deref()
    }

    /// Every path the message names: its files, its executable and its
    /// build script's output directory.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.filenames
            .iter()
            .map(Cow::as_ref)
            .chain(self.executable())
            .chain(self.out_dir())
    }

    /// The libraries a build script links, as `static=name`.
    pub(crate) fn linked_libs(&self) -> impl Iterator<Item = &str> {
        self.linked_libs.iter().map(Cow::as_ref)
    }

    /// A build script's `cfg`s, when the message carries them.
    pub(crate) fn cfgs(&self) -> Option<impl Iterator<Item = &str>> {
        Some(self.cfgs.as_ref()?.iter().map(Cow::as_ref))
    }

    /// A build script's environment, when the message carries it.
    pub(crate) fn env(&self) -> Option<impl Iterator<Item = (&str, &str)>> {
        Some(
            self.env
                .as_ref()?
                .iter()
                .map(|(name, value)| (name.as_ref(), value.as_ref())),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Messages;

    /// Each line is one message, its escaped text owned and the rest
    /// borrowed; a line of another shape is left out and counted.
    #[test]
    fn each_line_is_one_message_and_a_misshapen_line_is_counted() {
        let output = br#"{"reason":"compiler-artifact","package_id":"a 0.1.0","target":{"kind":["lib"],"name":"a"},"filenames":["/t/liba-0123.rlib"],"executable":null,"fresh":false,"message":{"spans":[{"x":1}]}}

{"reason":"build-script-executed","package_id":"b","out_dir":"C:\\out","linked_libs":["static=z"],"cfgs":["x"],"env":[["K","V"]]}
{"reason":"build-script-executed","env":[["K"]]}
not json
"#;
        let messages = Messages::parse(output);
        assert!(!messages.is_complete());
        let parsed = messages.iter().collect::<Vec<_>>();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].reason(), Some("compiler-artifact"));
        assert_eq!(parsed[0].target_kind(), Some("lib"));
        assert_eq!(parsed[0].fresh(), Some(false));
        assert_eq!(parsed[0].paths().collect::<Vec<_>>(), ["/t/liba-0123.rlib"]);
        assert_eq!(parsed[1].out_dir(), Some("C:\\out"));
        assert_eq!(parsed[1].paths().collect::<Vec<_>>(), ["C:\\out"]);
        assert_eq!(parsed[1].linked_libs().collect::<Vec<_>>(), ["static=z"]);
        assert_eq!(parsed[1].env().unwrap().collect::<Vec<_>>(), [("K", "V")]);
        assert!(parsed[0].cfgs().is_none());
        assert!(Messages::parse(b"{}\n").is_complete());
    }
}
