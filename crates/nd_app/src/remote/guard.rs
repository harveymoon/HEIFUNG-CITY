//! Who may call the remote API, and where it may write.
//!
//! The server binds 127.0.0.1, but that alone does not keep web pages out:
//! a page can send a "simple" cross-origin POST (no preflight) to localhost,
//! and with DNS rebinding it can even read the reply. So every request is
//! checked before its body is read:
//!
//! - `Host`, when sent, must be `127.0.0.1:<port>` or `localhost:<port>`
//!   (a rebound attacker domain fails this).
//! - An `Origin` header is refused unless it is this server itself; browsers
//!   send one on every cross-origin POST, so pages are turned away.
//! - `Sec-Fetch-Site`, when sent, must be `same-origin` or `none` (typed in
//!   the address bar).
//! - A POST needs `Content-Type: application/json` (which a page cannot send
//!   without a CORS preflight, and the preflight is never granted) or the
//!   `X-ND-Remote` header (likewise). Plain `curl -d` (form-encoded) and
//!   `text/plain` bodies are accepted only from clients that send no browser
//!   headers at all.
//!
//! Commands that touch files take paths only inside the project's `saves/`,
//! `screenshots/` and `exports/` folders (under the working directory) or
//! the OS temp directory, with the right extension and no `..`.

use std::path::{Component, Path, PathBuf};

/// Largest request body accepted (a command is a few hundred bytes; the
/// longest real ones, polygons and road point lists, a few kilobytes).
pub const MAX_BODY: usize = 1 << 20;
/// Requests handled at once. More are answered 503 straight away, so a
/// client that trickles its body cannot hold up everyone else.
pub const MAX_IN_FLIGHT: usize = 16;
/// The header a non-JSON client can send instead of the JSON content type.
pub const CLIENT_HEADER: &str = "X-ND-Remote";

/// Check a request's headers. `Err((status, why))` turns it away.
pub fn check_headers(method: &str, headers: &[(String, String)], port: u16) -> Result<(), (u16, String)> {
    let get = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim());
    let local = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if let Some(host) = get("Host") {
        if !local.iter().any(|l| host.eq_ignore_ascii_case(l)) {
            return Err((403, format!("Host {host:?} refused: use http://127.0.0.1:{port}")));
        }
    }
    let browser = get("Origin").is_some() || headers.iter().any(|(k, _)| k.get(..10).is_some_and(|p| p.eq_ignore_ascii_case("Sec-Fetch-")));
    if let Some(origin) = get("Origin") {
        let ok = local.iter().any(|l| origin.eq_ignore_ascii_case(&format!("http://{l}")));
        if !ok {
            return Err((403, format!("requests from web pages are refused (Origin {origin:?})")));
        }
    }
    if let Some(site) = get("Sec-Fetch-Site") {
        if !site.eq_ignore_ascii_case("same-origin") && !site.eq_ignore_ascii_case("none") {
            return Err((403, format!("requests from web pages are refused (Sec-Fetch-Site {site:?})")));
        }
    }
    if method.eq_ignore_ascii_case("POST") {
        let ctype = get("Content-Type").map(|c| c.split(';').next().unwrap_or("").trim().to_ascii_lowercase());
        let json = ctype.as_deref() == Some("application/json");
        let marked = get(CLIENT_HEADER).is_some();
        // `curl -d` sends form encoding; scripts sometimes send text/plain or
        // nothing. Fine from a client with no browser headers.
        let plain = !browser && matches!(ctype.as_deref(), None | Some("application/x-www-form-urlencoded") | Some("text/plain"));
        if !(json || marked || plain) {
            return Err((415, format!("send the command with Content-Type: application/json (got {:?})", ctype.unwrap_or_default())));
        }
    }
    Ok(())
}

/// Where a file-writing (or save-reading) command may go: `raw` resolved
/// against `cwd`, inside one of the allowed folders, with extension `ext`.
/// The folders are `saves/`, `screenshots/` and `exports/` under `cwd`, and
/// `temp`. Returns the absolute path (not canonicalised, for readable
/// replies); its parent folder may not exist yet.
pub fn confine(raw: &str, ext: &str, cwd: &Path, temp: &Path) -> Result<PathBuf, String> {
    let allowed = || format!("paths must be inside saves/, screenshots/ or exports/ (relative to {}) or the temp folder {}", show(cwd), show(temp));
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('\0') {
        return Err("empty path".into());
    }
    // UNC and device paths (\\server\share, \\?\..., //host): never, and
    // never touched on disk (resolving one can reach out to the network).
    if raw.starts_with("\\\\") || raw.starts_with("//") {
        return Err(format!("network and device paths are refused; {}", allowed()));
    }
    let p = Path::new(raw);
    if p.components().any(|c| c == Component::ParentDir) {
        return Err(format!("'..' is not allowed in a path; {}", allowed()));
    }
    // `C:foo` or `\foo` on Windows: relative to some other drive or root.
    if !p.is_absolute() && (p.has_root() || p.components().any(|c| matches!(c, Component::Prefix(_)))) {
        return Err(format!("give a relative path or a full one, not {raw:?}; {}", allowed()));
    }
    if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case(ext)) {
        return Err(format!("the path must end in .{ext}"));
    }
    let abs: PathBuf = cwd.join(p).components().filter(|c| *c != Component::CurDir).collect();
    let real = resolve(&abs).map_err(|e| format!("cannot resolve {}: {e}", show(&abs)))?;
    for root in [cwd.join("saves"), cwd.join("screenshots"), cwd.join("exports"), temp.to_path_buf()] {
        let Ok(root) = resolve(&root) else { continue };
        if real.starts_with(&root) && real != root {
            return Ok(abs);
        }
    }
    Err(format!("{} is outside the allowed folders; {}", show(&abs), allowed()))
}

/// `path` with its longest existing ancestor canonicalised (symlinks and
/// junctions followed, case as on disk) and the rest appended. The rest has
/// no `..`, so the result is where a write would land.
fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    let mut existing = path;
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if existing.exists() {
            let mut out = existing.canonicalize()?;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return Ok(out);
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no existing folder above it")),
        }
    }
}

fn show(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn web_pages_are_refused() {
        // A no-cors fetch from a page: Origin set, text/plain body.
        let page = h(&[("Host", "127.0.0.1:4817"), ("Origin", "https://evil.example"), ("Content-Type", "text/plain;charset=UTF-8"), ("Sec-Fetch-Site", "cross-site")]);
        assert_eq!(check_headers("POST", &page, 4817).unwrap_err().0, 403);
        // A sandboxed iframe sends Origin: null.
        let null = h(&[("Host", "127.0.0.1:4817"), ("Origin", "null"), ("Content-Type", "application/json")]);
        assert_eq!(check_headers("POST", &null, 4817).unwrap_err().0, 403);
        // DNS rebinding: the attacker's name in Host.
        let rebound = h(&[("Host", "attacker.example:4817"), ("Content-Type", "application/json")]);
        assert_eq!(check_headers("POST", &rebound, 4817).unwrap_err().0, 403);
        // Wrong port in Host.
        assert_eq!(check_headers("GET", &h(&[("Host", "127.0.0.1:80")]), 4817).unwrap_err().0, 403);
        // A form post without Origin but from a browser (Sec-Fetch headers).
        let form = h(&[("Host", "localhost:4817"), ("Content-Type", "application/x-www-form-urlencoded"), ("Sec-Fetch-Mode", "no-cors")]);
        assert_eq!(check_headers("POST", &form, 4817).unwrap_err().0, 415);
        // Multipart is never a command.
        let multi = h(&[("Host", "127.0.0.1:4817"), ("Content-Type", "multipart/form-data; boundary=x")]);
        assert_eq!(check_headers("POST", &multi, 4817).unwrap_err().0, 415);
    }

    #[test]
    fn scripts_get_through() {
        // The MCP bridge / urllib / requests with JSON.
        assert!(check_headers("POST", &h(&[("Host", "127.0.0.1:4817"), ("Content-Type", "application/json")]), 4817).is_ok());
        // `curl -d '{...}'`: form encoding, no browser headers.
        assert!(check_headers("POST", &h(&[("Host", "127.0.0.1:4817"), ("User-Agent", "curl/8.4"), ("Content-Type", "application/x-www-form-urlencoded")]), 4817).is_ok());
        // Any client that marks itself.
        assert!(check_headers("POST", &h(&[("Host", "LOCALHOST:4817"), ("X-ND-Remote", "1"), ("Content-Type", "text/plain")]), 4817).is_ok());
        // HTTP/1.0 without Host; GET typed into the address bar.
        assert!(check_headers("POST", &h(&[]), 4817).is_ok());
        assert!(check_headers("GET", &h(&[("Host", "127.0.0.1:4817"), ("Sec-Fetch-Site", "none"), ("Sec-Fetch-Mode", "navigate")]), 4817).is_ok());
    }

    #[test]
    fn paths_stay_in_their_folders() {
        let base = std::env::temp_dir().join(format!("nd_guard_test_{}", std::process::id()));
        let cwd = base.join("project");
        let temp = base.join("tmp");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        // Allowed: relative into the project folders (made or not), temp.
        assert!(confine("saves/a.ron", "ron", &cwd, &temp).is_ok());
        assert!(confine("./saves/sub/b.RON", "ron", &cwd, &temp).is_ok());
        assert!(confine("screenshots/x.png", "png", &cwd, &temp).is_ok());
        assert!(confine("exports/city.json", "json", &cwd, &temp).is_ok());
        let in_temp = temp.join("t.ron");
        assert!(confine(&in_temp.to_string_lossy(), "ron", &cwd, &temp).is_ok());
        let in_saves = cwd.join("saves").join("abs.ron");
        assert!(confine(&in_saves.to_string_lossy(), "ron", &cwd, &temp).is_ok());
        // Refused: traversal, other folders, wrong extension, the folder itself.
        assert!(confine("saves/../evil.ron", "ron", &cwd, &temp).is_err());
        assert!(confine("../outside/saves/x.ron", "ron", &cwd, &temp).is_err());
        assert!(confine("city.ron", "ron", &cwd, &temp).is_err());
        assert!(confine("src/main.rs", "ron", &cwd, &temp).is_err());
        assert!(confine("saves/a.exe", "ron", &cwd, &temp).is_err());
        assert!(confine("saves/a", "ron", &cwd, &temp).is_err());
        assert!(confine("saves/x.png", "ron", &cwd, &temp).is_err());
        let outside = base.join("elsewhere.ron");
        assert!(confine(&outside.to_string_lossy(), "ron", &cwd, &temp).is_err());
        assert!(confine("\\\\server\\share\\x.ron", "ron", &cwd, &temp).is_err());
        assert!(confine("//server/share/x.ron", "ron", &cwd, &temp).is_err());
        if cfg!(windows) {
            assert!(confine("C:x.ron", "ron", &cwd, &temp).is_err());
            assert!(confine("\\Windows\\x.ron", "ron", &cwd, &temp).is_err());
            assert!(confine("C:/Windows/x.ron", "ron", &cwd, &temp).is_err());
        } else {
            assert!(confine("/etc/x.ron", "ron", &cwd, &temp).is_err());
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
