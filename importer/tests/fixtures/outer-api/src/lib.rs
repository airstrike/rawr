pub use hop_api;
pub use inner_api;

pub fn wrap(value: u8) -> inner_api::Deep {
    inner_api::Deep(value)
}

pub fn leaf() -> hop_api::Leaf {
    hop_api::Leaf
}
