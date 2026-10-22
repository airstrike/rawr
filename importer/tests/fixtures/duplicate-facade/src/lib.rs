pub use duplicate_one::VersionOne;
pub use duplicate_one::duplicate_macro;

mod private_glob {
    pub use duplicate_two::*;
}

pub use private_glob::*;

const _: () = {
    assert!(option_env!("RUSTC_WRAPPER").is_none());
    assert!(option_env!("RUSTC_WORKSPACE_WRAPPER").is_none());
    assert!(option_env!("RAWR_IMPORT_PLAN").is_none());
    assert!(option_env!("RAWR_IMPORT_PROTOCOL").is_none());
};
