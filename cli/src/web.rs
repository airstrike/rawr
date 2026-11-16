//! `rawr run --web`'s options: the program runs in a browser, on a page
//! `rawr-import` serves on this machine, rather than in a process here.

use std::ffi::OsString;
use std::path::PathBuf;

/// What `rawr run --web` was asked for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// `--out <dir>`: write the page's files to `dir` and serve nothing.
    pub out: Option<PathBuf>,
    /// `--port <port>`: the port the page is served on.
    pub port: Option<u16>,
}

impl Options {
    /// Take `--web`, `--out <dir>` and `--port <port>` out of a command's
    /// `arguments`, which hold no program arguments: the options when
    /// `--web` is among them, and the arguments left.
    ///
    /// # Errors
    ///
    /// An option without its value, or `--out` or `--port` without `--web`.
    pub fn take(arguments: &[OsString]) -> Result<(Option<Self>, Vec<OsString>), String> {
        let mut web = false;
        let mut options = Self::default();
        let mut left = Vec::with_capacity(arguments.len());
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.to_string_lossy().as_ref() {
                "--web" => web = true,
                "--out" => {
                    let directory = arguments.next().ok_or("`--out` needs a directory")?;
                    options.out = Some(PathBuf::from(directory));
                }
                "--port" => {
                    let port = arguments.next().ok_or("`--port` needs a port")?;
                    let port = port.to_string_lossy();
                    options.port = Some(
                        port.parse()
                            .map_err(|_| format!("`--port` needs a port, not `{port}`"))?,
                    );
                }
                _ => left.push(argument.clone()),
            }
        }
        if web {
            return Ok((Some(options), left));
        }
        match (&options.out, options.port) {
            (Some(_), _) => Err("`--out` runs with `--web`".to_owned()),
            (_, Some(_)) => Err("`--port` runs with `--web`".to_owned()),
            (None, None) => Ok((None, left)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::Options;

    fn words(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// `--web` and its options leave the command and its path, and need
    /// each other.
    #[test]
    fn web_options_are_taken_with_their_values() {
        let (options, left) = Options::take(&words(&["run", "--web", "app", "--port", "8001"]))
            .expect("the options read");
        assert_eq!(
            options,
            Some(Options {
                out: None,
                port: Some(8001)
            })
        );
        assert_eq!(left, words(&["run", "app"]));
        let (options, _) =
            Options::take(&words(&["run", "--out", "site", "--web"])).expect("the options read");
        assert_eq!(
            options.and_then(|options| options.out),
            Some(PathBuf::from("site"))
        );
        assert_eq!(
            Options::take(&words(&["run", "app"])),
            Ok((None, words(&["run", "app"])))
        );
        assert!(Options::take(&words(&["run", "--port", "80"])).is_err());
        assert!(Options::take(&words(&["run", "--web", "--port", "web"])).is_err());
        assert!(Options::take(&words(&["run", "--web", "--out"])).is_err());
    }
}
