//! The HUD's fonts and its type scale: one place for every text size.
//!
//! Two faces, both SIL OFL 1.1 (licences in `assets/fonts/`):
//! - **IBM Plex Sans** for running text, figures and small print: an
//!   engineered grotesque with open shapes and a tall x-height, calm and
//!   legible at 13 px for hours on end.
//! - **Chakra Petch** for headings, window titles, buttons and map names:
//!   squared, cut-cornered letters that read as neon signage and
//!   instrument panels without turning every line into a poster.
//!
//! IBM Plex Mono sets the monospace style. Behind them, as fallbacks, are
//! Noto Sans Symbols 2 (★ ⏸ ✔ ⚡ ⚠ and friends) and egui's own emoji and
//! icon fonts, so every symbol the UI writes has a glyph (`tests` below
//! checks every non-ASCII character in the app's string literals).
//!
//! Sizes are logical points. egui's `TextStyle`s are set from them, so
//! `.small()`, `.heading()` and plain labels follow the scale; painters and
//! `RichText::font` take the `FontId`s from the functions here.

use bevy_egui::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::sync::Arc;

/// Small print: hints, captions, chart ticks, hotkeys. The smallest text
/// anywhere in the HUD.
pub const SMALL: f32 = 13.0;
/// Running text and figures.
pub const BODY: f32 = 15.0;
/// Buttons and other clickable labels (display face).
pub const BUTTON: f32 = 15.0;
/// An emphasised name or figure inside a panel (display face).
pub const LEAD: f32 = 17.0;
/// Window titles and section headings (display face).
pub const HEADING: f32 = 19.0;
/// The biggest text in a panel: the inspected thing's name, a headline figure.
pub const TITLE: f32 = 22.0;
/// Monospace.
pub const MONO: f32 = 14.0;

/// The display family's name (Chakra Petch SemiBold, then the fallbacks).
const DISPLAY: &str = "display";
/// Chakra Petch Medium, for buttons: a touch lighter than the headings.
const DISPLAY_MEDIUM: &str = "display-medium";

/// The display face: headings, titles, buttons, names on the map.
pub fn display_family() -> FontFamily {
    FontFamily::Name(DISPLAY.into())
}

pub fn small() -> FontId {
    FontId::proportional(SMALL)
}

pub fn body() -> FontId {
    FontId::proportional(BODY)
}

/// The display face at `size`, for painters that need an odd size (map
/// labels scaled with zoom).
pub fn display(size: f32) -> FontId {
    FontId::new(size, display_family())
}

pub fn button() -> FontId {
    FontId::new(BUTTON, FontFamily::Name(DISPLAY_MEDIUM.into()))
}

pub fn lead() -> FontId {
    display(LEAD)
}

pub fn heading() -> FontId {
    display(HEADING)
}

pub fn title() -> FontId {
    display(TITLE)
}

/// The fonts: the faces above, each family ending in the symbol and emoji
/// fallbacks, then egui's Ubuntu and Hack for anything else a player types.
pub fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let mut add = |name: &str, data: FontData| {
        defs.font_data.insert(name.to_owned(), Arc::new(data));
    };
    // Plex Sans is a variable font; a hair above Regular reads better as
    // light text on the dark panels.
    add(
        "plex-sans",
        FontData::from_static(include_bytes!("../../../../assets/fonts/IBMPlexSans[wdth,wght].ttf"))
            .tweak(egui::FontTweak { coords: egui::epaint::text::VariationCoords::new([(b"wght", 450.0)]), ..Default::default() }),
    );
    add("plex-mono", FontData::from_static(include_bytes!("../../../../assets/fonts/IBMPlexMono-Regular.ttf")));
    add("chakra-semibold", FontData::from_static(include_bytes!("../../../../assets/fonts/ChakraPetch-SemiBold.ttf")));
    add("chakra-medium", FontData::from_static(include_bytes!("../../../../assets/fonts/ChakraPetch-Medium.ttf")));
    add("noto-symbols-2", FontData::from_static(include_bytes!("../../../../assets/fonts/NotoSansSymbols2-Regular.ttf")));

    // egui's defaults: NotoEmoji-Regular, emoji-icon-font, Ubuntu-Light, Hack.
    let fallbacks = ["NotoEmoji-Regular", "noto-symbols-2", "emoji-icon-font", "Ubuntu-Light", "Hack"];
    let chain = |first: &str| -> Vec<String> { std::iter::once(first).chain(fallbacks).map(str::to_owned).collect() };
    defs.families.insert(FontFamily::Proportional, chain("plex-sans"));
    defs.families.insert(FontFamily::Monospace, chain("plex-mono"));
    defs.families.insert(display_family(), chain("chakra-semibold"));
    defs.families.insert(FontFamily::Name(DISPLAY_MEDIUM.into()), chain("chakra-medium"));
    defs
}

/// Install the fonts and the type scale, and size the widgets to match.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_zoom_factor(saved_scale());
    // Ctrl +/- would zoom behind the setting's back (and its keys are the
    // speed keys' neighbours); the View window's Text size does it.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, small()),
            (TextStyle::Body, body()),
            (TextStyle::Button, button()),
            (TextStyle::Heading, heading()),
            (TextStyle::Monospace, FontId::monospace(MONO)),
        ]
        .into();
        // Clickable things a little taller than egui's 18, for the bigger
        // text; a bit more air between items.
        style.spacing.interact_size.y = 22.0;
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    });
}

/// [fonts] The player's text size, as egui's zoom factor: every panel,
/// window and label scales together. Saved in `settings.ron` in the game's
/// working directory; `ND_UI_SCALE` overrides it for a run.
pub const SCALES: [(f32, &str); 4] = [(0.9, "Small"), (1.0, "Normal"), (1.15, "Large"), (1.3, "Huge")];
const SETTINGS_PATH: &str = "settings.ron";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Settings {
    #[serde(default)]
    ui_scale: Option<f32>,
}

fn clamp_scale(s: f32) -> f32 {
    if s.is_finite() {
        s.clamp(0.75, 2.0)
    } else {
        1.0
    }
}

/// The UI scale to start with: `ND_UI_SCALE`, else the saved one, else 1.
pub fn saved_scale() -> f32 {
    if let Some(s) = std::env::var("ND_UI_SCALE").ok().and_then(|v| v.trim().parse::<f32>().ok()) {
        return clamp_scale(s);
    }
    std::fs::read_to_string(SETTINGS_PATH).ok().and_then(|t| ron::from_str::<Settings>(&t).ok()).and_then(|s| s.ui_scale).map_or(1.0, clamp_scale)
}

/// Set the UI scale now and save it for the next run.
pub fn set_scale(ctx: &egui::Context, scale: f32) {
    let scale = clamp_scale(scale);
    ctx.set_zoom_factor(scale);
    let mut settings = std::fs::read_to_string(SETTINGS_PATH).ok().and_then(|t| ron::from_str::<Settings>(&t).ok()).unwrap_or_default();
    settings.ui_scale = Some(scale);
    if let Ok(text) = ron::ser::to_string_pretty(&settings, Default::default()) {
        let _ = std::fs::write(SETTINGS_PATH, text);
    }
}

/// A point Bevy projected (`Camera::world_to_viewport`, logical pixels) in
/// egui points, which are logical pixels divided by the UI scale.
pub fn to_points(ctx: &egui::Context, v: bevy::math::Vec2) -> egui::Pos2 {
    let z = ctx.zoom_factor();
    egui::pos2(v.x / z, v.y / z)
}

/// The text size picker for the View window.
pub fn scale_setting(ui: &mut egui::Ui) {
    super::section(ui, "TEXT SIZE");
    let now = ui.ctx().zoom_factor();
    ui.horizontal(|ui| {
        for (s, label) in SCALES {
            if ui.selectable_label((now - s).abs() < 0.01, label).clicked() {
                set_scale(ui.ctx(), s);
            }
        }
    });
    super::hint(ui, &format!("{:.0}% · scales every panel, window and label on the map", now * 100.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-ASCII character in a string or char literal of the app's
    /// source and the sim's (whose labels the HUD shows), with the file it came from.
    fn literal_chars() -> Vec<(char, String)> {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut out = Vec::new();
        let mut stack = vec![crate_dir.join("src"), crate_dir.join("../nd_sim/src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let src = std::fs::read_to_string(&path).unwrap();
                    for c in literal_text(&src) {
                        if !c.is_ascii() {
                            out.push((c, path.display().to_string()));
                        }
                    }
                }
            }
        }
        out
    }

    /// The characters inside string and char literals, with `\u{..}`
    /// escapes decoded; comments are skipped.
    fn literal_text(src: &str) -> Vec<char> {
        let mut out = Vec::new();
        let cs: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            match cs[i] {
                '/' if cs.get(i + 1) == Some(&'/') => {
                    while i < cs.len() && cs[i] != '\n' {
                        i += 1;
                    }
                }
                '/' if cs.get(i + 1) == Some(&'*') => {
                    while i + 1 < cs.len() && !(cs[i] == '*' && cs[i + 1] == '/') {
                        i += 1;
                    }
                    i += 2;
                }
                '"' => {
                    i += 1;
                    while i < cs.len() && cs[i] != '"' {
                        if cs[i] == '\\' {
                            i += 1;
                            if let Some((c, n)) = unicode_escape(&cs[i..]) {
                                out.push(c);
                                i += n;
                                continue;
                            }
                        } else {
                            out.push(cs[i]);
                        }
                        i += 1;
                    }
                    i += 1;
                }
                '\'' => {
                    // A char literal ('x', '\u{..}'); anything else is a lifetime.
                    if cs.get(i + 1) == Some(&'\\') {
                        if let Some((c, n)) = unicode_escape(&cs[i + 2..]) {
                            out.push(c);
                            i += 2 + n;
                        } else {
                            i += 3;
                        }
                    } else if cs.get(i + 2) == Some(&'\'') {
                        out.push(cs[i + 1]);
                        i += 2;
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// `u{2192}` at the start of `cs` (after the backslash): the char and
    /// how many chars the escape spans.
    fn unicode_escape(cs: &[char]) -> Option<(char, usize)> {
        if cs.first() != Some(&'u') || cs.get(1) != Some(&'{') {
            return None;
        }
        let end = cs.iter().position(|&c| c == '}')?;
        let hex: String = cs[2..end].iter().collect();
        Some((char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?, end + 1))
    }

    /// Does some font in `family`'s chain have a glyph for `c`? Read from
    /// the fonts' own character maps: egui's `has_glyph` reports a char as
    /// missing when it comes from the face that also draws the replacement
    /// box (NotoEmoji here), which would fail ⚡ and ✔ that render fine.
    fn covered(defs: &FontDefinitions, family: &FontFamily, c: char) -> bool {
        defs.families[family].iter().any(|name| {
            let data = &defs.font_data[name];
            ttf_parser::Face::parse(&data.font, data.index).is_ok_and(|f| f.glyph_index(c).is_some())
        })
    }

    #[test]
    fn every_ui_symbol_has_a_glyph() {
        let defs = fonts();
        let chars = literal_chars();
        assert!(chars.iter().any(|(c, _)| *c == '·'), "the scan found none of the UI's middle dots");
        let mut missing: Vec<String> = Vec::new();
        for family in defs.families.keys() {
            for (c, file) in &chars {
                if !covered(&defs, family, *c) {
                    missing.push(format!("{c:?} (U+{:04X}) in {file} with {family:?}", *c as u32));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "characters with no glyph:\n{}", missing.join("\n"));
    }

    #[test]
    fn the_symbols_that_were_boxes_have_glyphs() {
        let defs = fonts();
        for c in ['→', '✕', '·', '²', '°', '⚡', '★', '⏸', '−', '…', '✔', '⚠'] {
            for family in [FontFamily::Proportional, display_family()] {
                assert!(covered(&defs, &family, c), "{c:?} has no glyph in {family:?}");
            }
        }
    }

    #[test]
    fn the_scale_climbs() {
        let sizes = [SMALL, BODY, LEAD, HEADING, TITLE];
        assert!(sizes.windows(2).all(|w| w[0] < w[1]));
        assert!(SMALL >= 13.0);
    }
}
