/// A type this crate exports itself.
pub struct Own(pub i64);

pub fn own() -> Own {
    Own(1)
}

/// A function whose output only `leaf-api` exports, not this crate.
pub fn leaf() -> leaf_api::Leaf {
    leaf_api::Leaf
}
