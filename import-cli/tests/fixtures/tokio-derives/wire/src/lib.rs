//! A response body decoded on one of tokio's worker threads, as reqwest's
//! `Response::json` decodes it, and a local server with reqwest's shape
//! for iced's pokedex: `get`, `Response::json` and `Response::bytes`, an
//! image handle and a `Task::perform` that runs a future on a worker.

use std::sync::atomic::{AtomicBool, Ordering};

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

/// A GET of `url` from a local server, as `reqwest::get` makes one: the
/// species and sprite URLs iced's pokedex fetches, while no [`outage`]
/// has begun.
pub async fn get<U: IntoUrl>(url: U) -> Result<Response, Error> {
    let url = url.into_url();
    tokio::task::yield_now().await;
    if DOWN.load(Ordering::SeqCst) {
        return Err(Error(format!("error sending request for url ({url})")));
    }
    if url.contains("/pokemon-species/") {
        return Ok(Response(SPECIES.as_bytes().to_vec()));
    }
    if url.ends_with(".png") {
        return Ok(Response(b"\x89PNG".to_vec()));
    }
    Err(Error(format!("404 Not Found ({url})")))
}

/// Every later [`get`] fails to reach the server.
pub fn outage() {
    DOWN.store(true, Ordering::SeqCst);
}

static DOWN: AtomicBool = AtomicBool::new(false);

const SPECIES: &str = r#"{"name":"bulbasaur","id":1,"flavor_text_entries":[
    {"flavor_text":"Une graine","language":{"name":"fr"}},
    {"flavor_text":"A strange seed was\nplanted on its\u000cback at birth.","language":{"name":"en"}}]}"#;

/// What [`get`] takes, as reqwest's `IntoUrl`.
pub trait IntoUrl {
    fn into_url(self) -> String;
}

impl IntoUrl for &str {
    fn into_url(self) -> String {
        self.to_owned()
    }
}

impl IntoUrl for &String {
    fn into_url(self) -> String {
        self.clone()
    }
}

impl IntoUrl for String {
    fn into_url(self) -> String {
        self
    }
}

/// A response's body, as reqwest's `Response`.
pub struct Response(Vec<u8>);

impl Response {
    /// The body decoded as a `T`, as reqwest's `Response::json`.
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, Error> {
        tokio::task::yield_now().await;
        serde_json::from_slice(&self.0).map_err(|error| Error(error.to_string()))
    }

    /// The body's bytes, as reqwest's `Response::bytes`.
    pub async fn bytes(self) -> Result<Bytes, Error> {
        tokio::task::yield_now().await;
        Ok(Bytes(self.0))
    }
}

/// A body's bytes, as `bytes::Bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bytes(Vec<u8>);

/// A request that failed, as `reqwest::Error`.
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// An image's bytes, as iced's `image::Handle`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handle(Bytes);

impl Handle {
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        Self(bytes.into())
    }
}

/// `future` run on one of tokio's worker threads and its output given to
/// `f`, as iced's `Task::perform` runs one on its executor.
pub fn perform<A, M>(
    future: impl Future<Output = A> + Send + 'static,
    f: impl FnOnce(A) -> M + Send + 'static,
) -> tokio::task::JoinHandle<M>
where
    A: Send + 'static,
    M: Send + 'static,
{
    tokio::spawn(async move { f(future.await) })
}
