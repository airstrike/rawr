//! Read-only facts about the native iced build.

/// Exact iced source revision linked into the native adapter.
pub const ICED_REVISION: &str = "be88873d38544455e26c7a104eb757da2cbbac6b";

/// A renderer compiled into the native adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Renderer {
    /// iced's GPU-backed wgpu renderer, preferred for the live window.
    Wgpu,
    /// iced's CPU-backed tiny-skia renderer: the live window's fallback and the
    /// pinned renderer for previews and snapshots.
    TinySkia,
}

/// A window system compiled into the native adapter for this target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowSystem {
    /// macOS AppKit through winit.
    AppKit,
    /// Microsoft Windows through winit.
    Windows,
    /// The X Window System through winit.
    X11,
    /// The Wayland protocol through winit.
    Wayland,
}

/// Opaque proof of the renderer and window systems selected by this build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Build {
    rawr_version: &'static str,
    adapter_version: &'static str,
    iced_version: &'static str,
    iced_revision: &'static str,
    renderers: &'static [Renderer],
    window_systems: &'static [WindowSystem],
}

impl Build {
    /// Inspect the capabilities of the linked native adapter.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            rawr_version: rawr::VERSION,
            adapter_version: env!("CARGO_PKG_VERSION"),
            iced_version: "0.15.0-dev",
            iced_revision: ICED_REVISION,
            renderers: &[Renderer::Wgpu, Renderer::TinySkia],
            window_systems: window_systems(),
        }
    }

    /// Return the linked Rawr runtime version.
    #[must_use]
    pub const fn rawr_version(&self) -> &'static str {
        self.rawr_version
    }

    /// Return the linked rawr-iced adapter version.
    #[must_use]
    pub const fn adapter_version(&self) -> &'static str {
        self.adapter_version
    }

    /// Return the linked iced package version.
    #[must_use]
    pub const fn iced_version(&self) -> &'static str {
        self.iced_version
    }

    /// Return the exact linked iced source revision.
    #[must_use]
    pub const fn iced_revision(&self) -> &'static str {
        self.iced_revision
    }

    /// Return every renderer compiled into this build.
    #[must_use]
    pub const fn renderers(&self) -> &'static [Renderer] {
        self.renderers
    }

    /// Return every window system compiled for this target.
    #[must_use]
    pub const fn window_systems(&self) -> &'static [WindowSystem] {
        self.window_systems
    }
}

#[cfg(target_os = "macos")]
const fn window_systems() -> &'static [WindowSystem] {
    &[WindowSystem::AppKit]
}

#[cfg(target_os = "windows")]
const fn window_systems() -> &'static [WindowSystem] {
    &[WindowSystem::Windows]
}

#[cfg(target_os = "linux")]
const fn window_systems() -> &'static [WindowSystem] {
    &[WindowSystem::X11, WindowSystem::Wayland]
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const fn window_systems() -> &'static [WindowSystem] {
    &[]
}

#[cfg(test)]
mod tests {
    use super::{Build, Renderer};

    #[test]
    fn current_build_reports_the_exact_linked_contract() {
        let build = Build::current();

        assert_eq!(build.rawr_version(), rawr::VERSION);
        assert_eq!(build.adapter_version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(build.iced_version(), "0.15.0-dev");
        assert_eq!(build.iced_revision(), super::ICED_REVISION);
        assert_eq!(build.renderers(), &[Renderer::Wgpu, Renderer::TinySkia]);
    }

    #[test]
    fn capability_constants_cannot_drift_from_the_adapter_manifest() {
        let manifest = include_str!("../Cargo.toml");

        assert!(manifest.contains(&format!("rev = \"{}\"", super::ICED_REVISION)));
        // The standalone build links both renderers and Linux's window systems.
        assert!(manifest.contains("\"iced/tiny-skia\""));
        assert!(manifest.contains("\"iced/wgpu\""));
        assert!(manifest.contains("\"iced/x11\", \"iced/wayland\""));
    }
}
