// rawr
cfg_if::cfg_if! {
    if #[cfg(unix)] {
        fn kind() -> &'static str {
            "unix"
        }
    } else if #[cfg(windows)] {
        fn kind() -> &'static str {
            "windows"
        }
    } else {
        fn kind() -> &'static str {
            "other"
        }
    }
}

fn main() {
    println!("{}", kind());
}
