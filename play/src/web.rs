//! The playground's render request (`POST /render` on the bot's HTTP port,
//! reached only through the Worker): a JSON body read into the same
//! [`Request`] a `?play` message makes.
//!
//! `{"code": "...", "theme": "dark" | "light", "size": "WxH" | "", "messages": ["Message::Increment"], "still": true}`
//!
//! `still` asks for the last frame as a PNG, never a GIF (a share's preview).
//!
//! The Worker names the caller in [`USER_HEADER`]: a hash of its IP. An
//! animation's longer budget is counted against it (see
//! [`crate::budget`]). A request without it gets the still budget.

use crate::message::Request;
use crate::Options;

/// Longest snippet the playground renders, in bytes.
pub const MAX_CODE: usize = 16 * 1024;
/// Most messages applied before the view.
pub const MAX_MESSAGES: usize = 32;
/// Longest message expression, in bytes.
pub const MAX_MESSAGE: usize = 256;

/// The header that names the playground's caller.
pub const USER_HEADER: &str = "x-play-user";

/// The budget key for the caller [`USER_HEADER`] names: `web:` and the
/// name, or `None` when it is missing, empty or longer than 64 bytes.
#[must_use]
pub fn user(header: Option<&str>) -> Option<String> {
    let name = header?.trim();
    (!name.is_empty() && name.len() <= 64).then(|| format!("web:{name}"))
}

/// The render request in a playground body, over `defaults`.
pub fn parse_render(body: &[u8], defaults: &Options) -> Result<Request, String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|error| format!("bad request: {error}"))?;
    let code = value["code"].as_str().ok_or("bad request: no `code`")?;
    if code.trim().is_empty() {
        return Err("write some code to render".to_owned());
    }
    if code.len() > MAX_CODE {
        return Err(format!("the code is over {} KiB", MAX_CODE / 1024));
    }
    let mut options = defaults.clone();
    match value.get("theme").and_then(serde_json::Value::as_str) {
        None | Some("") => {}
        Some(theme) => options.theme = theme.parse()?,
    }
    match value
        .get("size")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
    {
        None | Some("") => {}
        Some(size) => options.size = Some(crate::parse_size(size)?),
    }
    if let Some(messages) = value.get("messages") {
        let messages = messages
            .as_array()
            .ok_or("bad request: `messages` is a list")?;
        if messages.len() > MAX_MESSAGES {
            return Err(format!("at most {MAX_MESSAGES} messages"));
        }
        for message in messages {
            let message = message
                .as_str()
                .ok_or("bad request: a message is text")?
                .trim();
            if message.len() > MAX_MESSAGE {
                return Err(format!("a message is over {MAX_MESSAGE} bytes"));
            }
            if !message.is_empty() {
                options.messages.push(message.to_owned());
            }
        }
    }
    if let Some(still) = value.get("still").and_then(serde_json::Value::as_bool) {
        options.still = still;
    }
    Ok(Request {
        code: code.to_owned(),
        options,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_web_render_body() {
        let defaults = Options::default();
        let body = br#"{"code":"text(\"hi\")","theme":"light","size":"320x200","messages":["Message::A"," ",""]}"#;
        let request = parse_render(body, &defaults).unwrap();
        assert_eq!(request.code, "text(\"hi\")");
        assert_eq!(request.options.size, Some((320, 200)));
        assert_eq!(request.options.messages, ["Message::A"]);
        assert_ne!(request.options.theme, defaults.theme);
        let plain = parse_render(br#"{"code":"x","size":""}"#, &defaults).unwrap();
        assert_eq!(plain.options, defaults);
        let still = parse_render(br#"{"code":"x","still":true}"#, &defaults).unwrap();
        assert!(still.options.still);
        assert!(still.args(crate::Format::Png, 64).contains(&"--still".to_owned()));
    }

    #[test]
    fn play_web_user() {
        assert_eq!(user(Some("ab12")).as_deref(), Some("web:ab12"));
        assert_eq!(user(Some(" ")), None);
        assert_eq!(user(None), None);
        assert_eq!(user(Some(&"a".repeat(65))), None);
    }

    #[test]
    fn play_web_render_limits() {
        let defaults = Options::default();
        let big = serde_json::json!({ "code": "x".repeat(MAX_CODE + 1) }).to_string();
        assert!(parse_render(big.as_bytes(), &defaults)
            .unwrap_err()
            .contains("KiB"));
        assert!(parse_render(b"{}", &defaults).is_err());
        assert!(parse_render(b"not json", &defaults).is_err());
        assert!(parse_render(br#"{"code":"  "}"#, &defaults).is_err());
        assert!(parse_render(br#"{"code":"x","size":"wide"}"#, &defaults).is_err());
        assert!(parse_render(br#"{"code":"x","theme":"pink"}"#, &defaults).is_err());
        let many = serde_json::json!({ "code": "x", "messages": vec!["M"; MAX_MESSAGES + 1] });
        assert!(parse_render(many.to_string().as_bytes(), &defaults).is_err());
    }
}
