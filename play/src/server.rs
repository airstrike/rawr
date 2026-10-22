//! Server (guild) IDs as opaque tokens, so reports can count per server
//! without revealing which server.
//!
//! A token is one AES-256 block: the guild ID as a big-endian `u64` in the
//! first eight bytes, zeros in the last eight, encrypted under the secret
//! log key and written as 32 hex digits. The same server always gives the
//! same token (IDs are unique, so one deterministic block is enough), and
//! only the key's holder can turn a token back into an ID (`rawr-play
//! decrypt-server`). Decryption checks the zero padding, so a token made
//! with another key is refused rather than misread.
//!
//! The key is 32 random bytes as 64 hex digits: `PLAY_LOG_KEY` for the bot,
//! `~/.config/rawr-play/log-key` locally. Without a key the bot reports no
//! per-server data at all.

use std::path::PathBuf;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes256;

/// The secret key that makes and reads server tokens.
#[derive(Clone)]
pub struct ServerKey(Aes256);

impl std::fmt::Debug for ServerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServerKey(..)")
    }
}

impl ServerKey {
    /// A key from 64 hex digits (surrounding whitespace ignored).
    pub fn from_hex(text: &str) -> Result<Self, String> {
        let bytes = unhex(text.trim())
            .filter(|bytes| bytes.len() == 32)
            .ok_or("the log key must be 64 hex digits (32 bytes)")?;
        Ok(Self(Aes256::new(GenericArray::from_slice(&bytes))))
    }

    /// `PLAY_LOG_KEY`, if set and valid.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let text = std::env::var("PLAY_LOG_KEY").ok()?;
        match Self::from_hex(&text) {
            Ok(key) => Some(key),
            Err(error) => {
                eprintln!("rawr-play-bot: PLAY_LOG_KEY: {error}; reporting no per-server data");
                None
            }
        }
    }

    /// `~/.config/rawr-play/log-key`.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/rawr-play/log-key"))
    }

    /// The key in a file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Self::from_hex(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// The token for a guild ID: 32 hex digits.
    #[must_use]
    pub fn encrypt(&self, guild: u64) -> String {
        let mut block = [0u8; 16];
        block[..8].copy_from_slice(&guild.to_be_bytes());
        let mut block = GenericArray::from(block);
        self.0.encrypt_block(&mut block);
        hex(&block)
    }

    /// The guild ID in a token made with this key.
    pub fn decrypt(&self, token: &str) -> Result<u64, String> {
        let bytes = unhex(token.trim())
            .filter(|bytes| bytes.len() == 16)
            .ok_or_else(|| format!("not a server token (32 hex digits): {token}"))?;
        let mut block = GenericArray::clone_from_slice(&bytes);
        self.0.decrypt_block(&mut block);
        if block[8..].iter().any(|&byte| byte != 0) {
            return Err(format!("{token} was not made with this key"));
        }
        let mut id = [0u8; 8];
        id.copy_from_slice(&block[..8]);
        Ok(u64::from_be_bytes(id))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const OTHER: &str = "ff0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn play_server_token_round_trip() {
        let key = ServerKey::from_hex(KEY).unwrap();
        for id in [0, 1, 1_562_248_110_887_932_025, u64::MAX] {
            let token = key.encrypt(id);
            assert_eq!(token.len(), 32);
            assert!(token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            assert_eq!(key.decrypt(&token), Ok(id));
            assert_eq!(key.decrypt(&token.to_uppercase()), Ok(id));
        }
    }

    #[test]
    fn play_server_token_is_deterministic() {
        let key = ServerKey::from_hex(KEY).unwrap();
        let again = ServerKey::from_hex(&format!("  {KEY}\n")).unwrap();
        assert_eq!(key.encrypt(42), again.encrypt(42));
        assert_ne!(key.encrypt(42), key.encrypt(43));
        // Not the plaintext in any visible form.
        assert_ne!(key.encrypt(42), format!("{:016x}{:016x}", 42, 0));
    }

    #[test]
    fn play_server_token_wrong_key_is_refused() {
        let key = ServerKey::from_hex(KEY).unwrap();
        let other = ServerKey::from_hex(OTHER).unwrap();
        let token = key.encrypt(1_562_248_110_887_932_025);
        assert_ne!(other.encrypt(1_562_248_110_887_932_025), token);
        let read = other.decrypt(&token);
        assert!(read.is_err(), "{read:?}");
        assert!(key.decrypt("xyz").is_err());
        assert!(key.decrypt(&"0".repeat(30)).is_err());
    }

    #[test]
    fn play_server_key_shapes() {
        assert!(ServerKey::from_hex("").is_err());
        assert!(
            ServerKey::from_hex(&KEY[..32]).is_err(),
            "a 16-byte key is refused"
        );
        assert!(ServerKey::from_hex(&format!("{}zz", &KEY[..62])).is_err());
        assert_eq!(
            format!("{:?}", ServerKey::from_hex(KEY).unwrap()),
            "ServerKey(..)"
        );
    }
}
