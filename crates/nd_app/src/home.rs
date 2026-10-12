//! [home] The game's home folder: where `saves/`, `flags/`, `screenshots/`,
//! `exports/`, `assets/` (palette and building rules), `settings.ron` and
//! `committee_settings.ron` live. Every path the game reads or writes is
//! relative to it.
//!
//! It is the folder the exe sits in, whatever folder the game was started
//! from (launched from the repo root, it used to read the repo's old
//! `saves/`), or `ND_HOME` when set (`ND_HOME=.` keeps the working
//! directory: test instances that want their own folder). `main` makes it the
//! working directory before anything else runs, so the relative paths
//! throughout (`saves::DIR`, `flags::FLAG_FILE`, ...) resolve inside it.

use std::path::{Path, PathBuf};

/// The home folder for an `ND_HOME` value and the exe's path: the value if
/// set and not blank (relative to `cwd`), else the exe's folder, else `cwd`.
pub fn resolve(nd_home: Option<&str>, exe: Option<&Path>, cwd: &Path) -> PathBuf {
    if let Some(h) = nd_home.map(str::trim).filter(|h| !h.is_empty()) {
        let p = Path::new(h);
        return if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    }
    exe.and_then(Path::parent).filter(|d| !d.as_os_str().is_empty()).map_or_else(|| cwd.to_path_buf(), Path::to_path_buf)
}

/// Make the home folder the working directory (see the module docs).
/// Returns it, or why it could not be entered (the game then runs where it
/// was started, as before).
pub fn enter() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().ok();
    let home = resolve(std::env::var("ND_HOME").ok().as_deref(), exe.as_deref(), &cwd);
    std::fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    std::env::set_current_dir(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    Ok(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_the_exe_folder_unless_nd_home_says() {
        let cwd = Path::new("/work/repo");
        let exe = Path::new("/games/nd/target/release/neon_district.exe");
        assert_eq!(resolve(None, Some(exe), cwd), PathBuf::from("/games/nd/target/release"));
        assert_eq!(resolve(Some("  "), Some(exe), cwd), PathBuf::from("/games/nd/target/release"), "blank is unset");
        assert_eq!(resolve(Some("."), Some(exe), cwd), cwd.join("."));
        assert_eq!(resolve(Some("mine"), Some(exe), cwd), cwd.join("mine"));
        let abs = std::env::temp_dir();
        assert_eq!(resolve(Some(abs.to_str().unwrap()), Some(exe), cwd), abs);
        assert_eq!(resolve(None, None, cwd), cwd);
        assert_eq!(resolve(None, Some(Path::new("neon_district.exe")), cwd), cwd, "no folder in the exe path");
    }
}
