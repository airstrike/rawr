//! The playground's run request (`POST /render` on the bot's HTTP port,
//! reached only through the Worker): a JSON body read into the same
//! [`Request`] a `?play` message makes.
//!
//! `{"code": "..."}`. Other fields are ignored.

use crate::message::Request;

/// Longest snippet the playground runs, in bytes.
pub const MAX_CODE: usize = 16 * 1024;

/// The run request in a playground body.
pub fn parse_render(body: &[u8]) -> Result<Request, String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|error| format!("bad request: {error}"))?;
    let code = value["code"].as_str().ok_or("bad request: no `code`")?;
    if code.trim().is_empty() {
        return Err("write some code to run".to_owned());
    }
    if code.len() > MAX_CODE {
        return Err(format!("the code is over {} KiB", MAX_CODE / 1024));
    }
    Ok(Request {
        code: code.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_web_render_body() {
        assert_eq!(parse_render(br#"{"code":"1 + 2"}"#).unwrap().code, "1 + 2");
    }

    #[test]
    fn play_web_render_limits() {
        let big = serde_json::json!({ "code": "x".repeat(MAX_CODE + 1) }).to_string();
        assert!(parse_render(big.as_bytes()).unwrap_err().contains("KiB"));
        assert!(parse_render(b"{}").is_err());
        assert!(parse_render(b"not json").is_err());
        assert!(parse_render(br#"{"code":"  "}"#).is_err());
    }
}
