//! [review] No double-encoded UTF-8 ("mojibake") in the workspace's
//! sources: a "·" saved as "Â·", "—" as "â€”", "×" as "Ã—" once showed up
//! in the citizen inspector. Scans every .rs file under crates/.

use std::path::Path;

/// Pairs that only appear when UTF-8 bytes were read as Windows-1252 and
/// written back as UTF-8.
const MARKS: [&str; 6] = ["Â·", "Â±", "Â°", "Ã—", "â€", "â”"];

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).expect("read dir").flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            scan(&p, hits);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            for (i, line) in text.lines().enumerate() {
                // This file names the marks it looks for.
                if p.ends_with("source_encoding.rs") {
                    continue;
                }
                if MARKS.iter().any(|m| line.contains(m)) {
                    hits.push(format!("{}:{}: {}", p.display(), i + 1, line.trim()));
                }
            }
        }
    }
}

#[test]
fn no_double_encoded_text_in_sources() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates/");
    let mut hits = Vec::new();
    scan(crates, &mut hits);
    assert!(hits.is_empty(), "double-encoded UTF-8:\n{}", hits.join("\n"));
}
