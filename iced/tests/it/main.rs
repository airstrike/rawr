//! rawr-iced's integration tests, as one binary rather than one per file, each
//! linking its own copy of Rawr and iced.

mod application_contract;
mod async_body;
mod application_entry;
mod builtin_styles;
mod canvas;
mod canonical_values;
mod components;
mod control;
mod counter_runtime;
mod dependencies;
mod diagnostics;
mod effect_harness;
mod facade;
mod generated_activation;
mod generated_application;
mod generated_builders;
mod generated_custom_widget;
mod generated_preview;
mod generated_profile;
mod generated_recovery;
mod generated_runner_host;
mod generated_styles;
mod generated_view_compile_fail;
mod generated_view_region;
mod idioms;
mod intrinsic_column;
mod intrinsic_controls;
mod master_events;
mod master_helpers;
mod master_style;
mod native_adapter;
mod native_extension;
mod nested_generic;
mod package_application;
mod preview_catalog;
mod preview_metadata;
mod reload_generation;
mod reload_installation;
mod subscription_plan;
mod task_plan;
mod themes;
mod widget_descriptor;
mod widget_protocol;

/// Writes `relative` in the Cargo crate at `root`, marked `// rawr` unless
/// `source` already is, and the crate's `Cargo.toml` when it has none.
pub fn write_marked(root: &std::path::Path, relative: &str, source: impl AsRef<str>) {
    let manifest = root.join("Cargo.toml");
    if !manifest.exists() {
        std::fs::write(
            manifest,
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
    }
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = source.as_ref();
    if source.starts_with("// rawr\n") {
        std::fs::write(path, source).unwrap();
    } else {
        std::fs::write(path, format!("// rawr\n{source}")).unwrap();
    }
}
