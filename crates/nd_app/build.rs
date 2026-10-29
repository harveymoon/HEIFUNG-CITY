//! Embeds the HEIFUNG://CITY icon and version details (product name, file
//! description, version) in the Windows exe, so Explorer, the taskbar and the
//! file's Properties show them before the game sets its window icon. Where no
//! resource compiler is available the build goes on without them.

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let ico = std::path::Path::new(&manifest).join("../../assets/brand/icon.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it"));
    let rc = out.join("icon.rc");
    let path = ico.canonicalize().unwrap_or(ico).display().to_string().replace(std::path::MAIN_SEPARATOR, "/").trim_start_matches("//?/").to_string();
    std::fs::write(&rc, format!("1 ICON \"{path}\"\n{}", version_info())).expect("write icon.rc");
    if let Err(e) = embed_resource::compile(&rc, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=exe icon not embedded: {e}");
    }
}

/// The VERSIONINFO block: the game's name as the player sees it (the crate
/// names stay internal). `BIN` matches `[[bin]] name` in Cargo.toml.
fn version_info() -> String {
    const NAME: &str = "HEIFUNG://CITY";
    const BIN: &str = "heifung_city";
    let v = std::env::var("CARGO_PKG_VERSION").expect("cargo sets it");
    let mut n = v.split('.').map(|p| p.parse::<u16>().unwrap_or(0));
    let (a, b, c) = (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0));
    format!(
        "1 VERSIONINFO\nFILEVERSION {a},{b},{c},0\nPRODUCTVERSION {a},{b},{c},0\nFILEOS 0x40004\nFILETYPE 0x1\nBEGIN\n  BLOCK \"StringFileInfo\"\n  BEGIN\n    BLOCK \"040904B0\"\n    BEGIN\n      VALUE \"FileDescription\", \"{NAME}\"\n      VALUE \"ProductName\", \"{NAME}\"\n      VALUE \"InternalName\", \"{BIN}\"\n      VALUE \"OriginalFilename\", \"{BIN}.exe\"\n      VALUE \"FileVersion\", \"{v}\"\n      VALUE \"ProductVersion\", \"{v}\"\n    END\n  END\n  BLOCK \"VarFileInfo\"\n  BEGIN\n    VALUE \"Translation\", 0x409, 1200\n  END\nEND\n"
    )
}
