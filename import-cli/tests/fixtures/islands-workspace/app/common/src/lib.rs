//! A member that reads an asset at the workspace root.
const LOGO: &[u8] = include_bytes!("../../assets/logo.txt");

pub fn logo() -> String {
    String::from_utf8_lossy(LOGO).trim().to_string()
}
