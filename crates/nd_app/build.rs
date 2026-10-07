//! Embeds the District 2040 icon in the Windows exe, so Explorer and the
//! taskbar show it before the game sets its window icon. Where no resource
//! compiler is available the build goes on without it.

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
    std::fs::write(&rc, format!("1 ICON \"{path}\"
")).expect("write icon.rc");
    if let Err(e) = embed_resource::compile(&rc, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=exe icon not embedded: {e}");
    }
}
