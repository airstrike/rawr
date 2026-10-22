pub fn label() -> String {
    "generated".to_owned()
}

pub fn alternate() -> String {
    "alternate".to_owned()
}

pub fn window_title() -> String {
    use std::os::unix::fs::PermissionsExt as _;

    let Some(root) = std::env::var_os("RAWR_PROJECT_ROOT").map(std::path::PathBuf::from) else {
        return "phase9-baseline-invalid-title".to_owned();
    };
    let path = root.join("native/.phase9-window-title");
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return "phase9-baseline-invalid-title".to_owned();
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > 16 * 1024
    {
        return "phase9-baseline-invalid-title".to_owned();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return "phase9-baseline-invalid-title".to_owned();
    };
    if bytes.len() as u64 != metadata.len()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return "phase9-baseline-invalid-title".to_owned();
    }
    String::from_utf8(bytes).unwrap_or_else(|_| "phase9-baseline-invalid-title".to_owned())
}
