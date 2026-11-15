//! _(serde)_ Serialization and deserialization support for [`serde`](https://crates.io/crates/serde).
//! Exported under the `serde` feature only.

//! Without the feature, `rust-import` compiles only the impls of rawr's
//! values, which a runner's kept lowerings hold.

#[cfg(feature = "serde")]
mod de;
mod deserialize;
#[cfg(feature = "serde")]
mod metadata;
#[cfg(feature = "serde")]
mod ser;
mod serialize;

#[cfg(feature = "serde")]
pub use de::{from_dynamic, DynamicDeserializer};
#[cfg(feature = "serde")]
pub use ser::{to_dynamic, DynamicSerializer};
