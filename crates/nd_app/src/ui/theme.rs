//! The HUD's fonts, its type scale and its shared colours: one place for
//! every text size and for the colours more than one panel paints.
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

use bevy_egui::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
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
fn display_family() -> FontFamily {
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
/// home folder; `ND_UI_SCALE` overrides it for a run.
pub const SCALES: [(f32, &str); 4] = [(0.9, "Small"), (1.0, "Normal"), (1.15, "Large"), (1.3, "Huge")];
const SETTINGS_PATH: &str = "settings.ron";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Settings {
    #[serde(default)]
    ui_scale: Option<f32>,
    /// [social] Speech bubbles over people talking around town. Off unless
    /// the player turns them on in the View window.
    #[serde(default)]
    speech_bubbles: Option<bool>,
    /// [settings] The graphics setting ("Auto", "Low", "Medium", "High").
    #[serde(default)]
    quality: Option<String>,
}

fn load_settings() -> Settings {
    std::fs::read_to_string(SETTINGS_PATH).ok().and_then(|t| ron::from_str::<Settings>(&t).ok()).unwrap_or_default()
}

fn save_settings(settings: &Settings) {
    if let Ok(text) = ron::ser::to_string_pretty(settings, Default::default()) {
        let _ = std::fs::write(SETTINGS_PATH, text);
    }
}

/// 0 not read yet, 1 off, 2 on.
static BUBBLES: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Whether speech bubbles show over people talking (saved; off by default).
pub fn bubbles_on() -> bool {
    use std::sync::atomic::Ordering;
    match BUBBLES.load(Ordering::Relaxed) {
        0 => {
            let on = load_settings().speech_bubbles.unwrap_or(false);
            BUBBLES.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        v => v == 2,
    }
}

fn set_bubbles(on: bool) {
    BUBBLES.store(if on { 2 } else { 1 }, std::sync::atomic::Ordering::Relaxed);
    let mut settings = load_settings();
    settings.speech_bubbles = Some(on);
    save_settings(&settings);
}

/// [settings] The graphics setting saved last run, if any.
pub fn saved_quality() -> Option<String> {
    load_settings().quality
}

/// [settings] Save the graphics setting for the next run.
pub fn save_quality(label: &str) {
    let mut settings = load_settings();
    settings.quality = Some(label.to_string());
    save_settings(&settings);
}

fn clamp_scale(s: f32) -> f32 {
    if s.is_finite() {
        s.clamp(0.75, 2.0)
    } else {
        1.0
    }
}

/// The UI scale to start with: `ND_UI_SCALE`, else the saved one, else 1.
fn saved_scale() -> f32 {
    if let Some(s) = std::env::var("ND_UI_SCALE").ok().and_then(|v| v.trim().parse::<f32>().ok()) {
        return clamp_scale(s);
    }
    load_settings().ui_scale.map_or(1.0, clamp_scale)
}

/// Set the UI scale now and save it for the next run.
fn set_scale(ctx: &egui::Context, scale: f32) {
    let scale = clamp_scale(scale);
    ctx.set_zoom_factor(scale);
    let mut settings = load_settings();
    settings.ui_scale = Some(scale);
    save_settings(&settings);
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
    let mut on = bubbles_on();
    if ui.checkbox(&mut on, "Speech bubbles over people talking").clicked() {
        set_bubbles(on);
    }
}

// ── colours ────────────────────────────────────────────────────────────────

/// The accent colour of the HUD: headings, the active tool, window titles.
pub const GOLD: Color32 = Color32::from_rgb(235, 180, 90);
/// Light, slightly warm text for names and figures that should stand out.
pub const IVORY: Color32 = Color32::from_rgb(230, 225, 210);
/// Colour for a figure that should worry the player.
pub const WARN: Color32 = Color32::from_rgb(255, 120, 90);
/// Good news, in three shades that grew up apart; each panel keeps its own.
/// The plain one: incomes, the employed, a served grid.
pub const GOOD: Color32 = Color32::from_rgb(110, 200, 140);
/// A lighter green: the directory's open businesses, the committee's yes.
pub const GOOD_LIGHT: Color32 = Color32::from_rgb(120, 220, 140);
/// The brightest green: a road in good repair, close friends, "click to build".
pub const GOOD_BRIGHT: Color32 = Color32::from_rgb(120, 230, 150);
/// A caution, short of [`WARN`]: no staff, homeless, hungry.
pub const AMBER: Color32 = Color32::from_rgb(240, 200, 100);
/// Services and their water: a service building's tag, people at work.
pub const SERVICE_BLUE: Color32 = Color32::from_rgb(120, 200, 255);
/// Homes and households in charts, bus lines, free toll passage.
pub const TEAL: Color32 = Color32::from_rgb(110, 200, 220);
/// The backdrop of a chart's plot area.
pub const PLOT_BG: Color32 = Color32::from_rgba_premultiplied(20, 22, 34, 220);
/// The same colour given as unmultiplied (so a little darker): the People
/// page's charts.
pub const PLOT_BG_UNMULTIPLIED: Color32 = Color32::from_rgba_unmultiplied_const(20, 22, 34, 220);
/// The bars' and the toast's fill.
pub const PANEL_FILL: Color32 = Color32::from_rgba_unmultiplied_const(8, 10, 20, 235);
/// A window's edge and the rule under a header.
pub const EDGE: Color32 = Color32::from_rgb(95, 75, 45);
/// The edge of a card or tool button that is not lit.
pub const EDGE_IDLE: Color32 = Color32::from_rgb(60, 60, 72);
/// The square shown where a save has no picture (yet).
pub const NO_PICTURE: Color32 = Color32::from_rgb(20, 22, 34);

/// Between two colours, each channel rounded to the nearest step.
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// [`lerp`] with each channel rounded down (the concerns chart's).
pub fn lerp_floor(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// An sRGB colour given as 0..1 floats (zone and district colours), each
/// channel capped at 1 and rounded down.
pub fn rgb_f(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8)
}

/// An sRGB colour given as bytes.
pub fn rgb_u8(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A linear colour (as the map's materials take them), capped at 1,
/// converted to sRGB by egui.
pub fn rgb_linear(c: [f32; 3]) -> Color32 {
    Color32::from(egui::Rgba::from_rgb(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0)))
}

/// A linear colour to sRGB by a plain 2.2 gamma, clamped to 0..1 and
/// rounded down (the train swatches).
pub fn rgb_gamma(c: [f32; 3]) -> Color32 {
    let g = |x: f32| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(g(c[0]), g(c[1]), g(c[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_helpers_keep_their_rounding() {
        let (a, b) = (Color32::from_rgb(0, 0, 0), Color32::from_rgb(255, 255, 255));
        // 255 * 0.3 = 76.5: one rounds up, the other down.
        assert_eq!(lerp(a, b, 0.3), Color32::from_rgb(77, 77, 77));
        assert_eq!(lerp_floor(a, b, 0.3), Color32::from_rgb(76, 76, 76));
        assert_eq!(lerp(GOLD, WARN, 0.0), GOLD);
        assert_eq!(rgb_f([1.5, 0.5, 0.0]), Color32::from_rgb(255, 127, 0));
        assert_eq!(rgb_u8([1, 2, 3]), Color32::from_rgb(1, 2, 3));
        assert_eq!(rgb_linear([1.0, 0.0, 2.0]), Color32::from_rgb(255, 0, 255));
        assert_eq!(rgb_gamma([1.0, 0.0, -1.0]), Color32::from_rgb(255, 0, 0));
        assert_eq!(PANEL_FILL, Color32::from_rgba_unmultiplied(8, 10, 20, 235));
        assert_eq!(PLOT_BG_UNMULTIPLIED, Color32::from_rgba_unmultiplied(20, 22, 34, 220));
    }

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
