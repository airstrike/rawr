pub struct VersionOne {
    pub value: u8,
}

#[macro_export]
macro_rules! duplicate_macro {
    () => {
        1_u8
    };
}
