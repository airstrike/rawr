#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

const COMPILE_ENVIRONMENT: &str = env!("FRESH_COMPILE_ENV");
const OPTIONAL_COMPILE_ENVIRONMENT: Option<&str> = option_env!("FRESH_OPTIONAL_COMPILE_ENV");
const RAW_RUSTC_WRAPPER: Option<&str> = option_env!("RUSTC_WRAPPER");
const RAW_IMPORT_PLAN: Option<&str> = option_env!("RAWR_IMPORT_PLAN");
const RAW_IMPORT_PROTOCOL: Option<&str> = option_env!("RAWR_IMPORT_PROTOCOL");

pub fn freshness(value: &str) -> String {
    let _ = (
        COMPILE_ENVIRONMENT,
        OPTIONAL_COMPILE_ENVIRONMENT,
        RAW_RUSTC_WRAPPER,
        RAW_IMPORT_PLAN,
        RAW_IMPORT_PROTOCOL,
    );
    format!("{GENERATED}:{value}")
}
