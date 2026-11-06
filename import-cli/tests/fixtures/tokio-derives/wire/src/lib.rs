//! A response body decoded on one of tokio's worker threads, as reqwest's
//! `Response::json` decodes it.

use serde::de::DeserializeOwned;

/// `body` decoded as a `T` on a worker thread, and whether that thread is
/// another than the caller's.
pub async fn json<T: DeserializeOwned + Send + 'static>(
    body: &'static str,
) -> Result<(T, bool), String> {
    let caller = std::thread::current().id();
    tokio::spawn(async move {
        let elsewhere = std::thread::current().id() != caller;
        serde_json::from_str::<T>(body)
            .map(|value| (value, elsewhere))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
