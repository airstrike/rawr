//! Static application checking without native window creation.

use std::path::PathBuf;

use crate::compile;

#[cfg(all(test, feature = "native-runner"))]
pub(crate) fn execute(path: Option<PathBuf>) -> Result<PathBuf, compile::Error> {
    execute_loaded(crate::base::load(path.as_ref()).map_err(compile::Error::from_report)?)
}

/// Check an application from the command's one load.
pub(crate) fn execute_loaded(loaded: crate::base::Loaded) -> Result<PathBuf, compile::Error> {
    #[cfg(feature = "native-runner")]
    if loaded.graph.declares_rust_import() {
        return crate::import::owner::check(&loaded.target, Some(loaded.graph))
            .map_err(compile::Error::from_report);
    }
    let validated = compile::validate_loaded(loaded)?;
    Ok(validated.source().to_path_buf())
}
