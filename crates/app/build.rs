//! Reads the product version from `VERSION` in the repository root and gives the Windows
//! executable its icon (Explorer, desktop shortcuts) and version; the running window sets its
//! own icon from the same picture in `main.rs`.
//!
//! The version is not Cargo's: Cargo puts the package version into the identity of every build
//! artifact, so each bump left a full new set in `target/`. `Cargo.toml` stays at 0.0.0, and the
//! program uses `env!("APP_VERSION")`.

use std::path::Path;

const VERSION_FILE: &str = "../../VERSION";
const ICON: &str = "assets/brand/icon.ico";

fn main() {
    let version = product_version();
    println!("cargo:rustc-env=APP_VERSION={version}");
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(ICON);
    // winresource fills both from Cargo's version unless told otherwise.
    resource.set("FileVersion", &version);
    resource.set("ProductVersion", &version);
    let numeric = numeric_version(&version);
    resource.set_version_info(winresource::VersionInfo::FILEVERSION, numeric);
    resource.set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric);
    resource
        .compile()
        .expect("the icon resource compiles with the Windows SDK's rc.exe");
}

fn product_version() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(VERSION_FILE);
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let version = text.trim();
    let well_formed = version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|digit| digit.is_ascii_digit()));
    assert!(
        well_formed,
        "{} must hold one version like 0.2.1, found {version:?}",
        path.display()
    );
    version.to_owned()
}

/// Windows' four 16-bit fields, major first; the fourth stays 0.
fn numeric_version(version: &str) -> u64 {
    version
        .split('.')
        .map(|part| part.parse::<u64>().expect("checked digits"))
        .zip([48, 32, 16])
        .fold(0, |packed, (part, shift)| packed | (part << shift))
}
