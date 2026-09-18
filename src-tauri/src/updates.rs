//! How this copy was installed, which decides whether it can update itself.
//!
//! The updater replaces an AppImage or runs the Windows installer. A .deb or
//! .rpm belongs to the package manager, so those copies are pointed at the
//! download instead of being overwritten behind apt's back.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// Windows installer or AppImage: the updater can install in place.
    Updatable,
    /// Installed by a package manager: link to the download instead.
    Package,
    /// `tauri dev` or a platform with no published update.
    Unsupported,
}

/// Pure, for tests: the OS, the `$APPIMAGE` path, and whether this is a debug build.
fn classify(os: &str, appimage: Option<&str>, debug: bool) -> InstallKind {
    if debug {
        return InstallKind::Unsupported;
    }
    match os {
        "windows" => InstallKind::Updatable,
        "linux" if appimage.is_some_and(|path| !path.is_empty()) => InstallKind::Updatable,
        "linux" => InstallKind::Package,
        _ => InstallKind::Unsupported,
    }
}

#[tauri::command]
pub fn install_kind() -> InstallKind {
    let appimage = std::env::var("APPIMAGE").ok();
    classify(std::env::consts::OS, appimage.as_deref(), cfg!(debug_assertions))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_appimage_update_themselves() {
        assert_eq!(classify("windows", None, false), InstallKind::Updatable);
        assert_eq!(classify("linux", Some("/home/u/ParolaSSH.AppImage"), false), InstallKind::Updatable);
    }

    #[test]
    fn a_deb_install_is_left_to_the_package_manager() {
        assert_eq!(classify("linux", None, false), InstallKind::Package);
        assert_eq!(classify("linux", Some(""), false), InstallKind::Package);
    }

    #[test]
    fn dev_builds_and_macos_never_update() {
        assert_eq!(classify("windows", None, true), InstallKind::Unsupported);
        assert_eq!(classify("macos", None, false), InstallKind::Unsupported);
    }
}
