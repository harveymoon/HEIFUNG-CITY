//! [r4-leisure] "Demand for new buildings": the bar per zone (commercial
//! with its sub-zones, leisure among them), with the hover reasons and the
//! "include planned / under construction" extension. Drawn in City Data,
//! Market, Businesses and in a small movable window opened from the top
//! bar (`Info::Demand`), whose place, open state and checkbox are kept in
//! `settings.ron` between sessions.

use bevy_egui::egui::{self, Color32};
use nd_sim::commercial::CommercialSub;
use nd_sim::LandUse;
use std::sync::Mutex;

use super::{hint, UiCtx};

/// Width of the window's bars: compact enough to sit beside the Build pane.
pub const WINDOW_BAR_WIDTH: f32 = 236.0;

/// What the game remembers of the Demand window between sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DemandPrefs {
    /// The window was open when the game was last closed.
    #[serde(default)]
    pub open: bool,
    /// Its top-left corner, in points.
    #[serde(default)]
    pub pos: Option<[f32; 2]>,
    /// "include planned / under construction" ticked.
    #[serde(default)]
    pub include_sites: bool,
}

impl DemandPrefs {
    /// Note where the window is and whether it is open; true when that
    /// changed enough to save (a move of under a point is not news).
    pub fn note(&mut self, open: bool, pos: Option<[f32; 2]>) -> bool {
        let moved = match (self.pos, pos) {
            (Some(a), Some(b)) => (a[0] - b[0]).abs() >= 1.0 || (a[1] - b[1]).abs() >= 1.0,
            (None, Some(_)) => true,
            _ => false,
        };
        let changed = moved || self.open != open;
        if moved {
            self.pos = pos;
        }
        self.open = open;
        changed
    }
}

/// The prefs as last read or saved (read from `settings.ron` once).
static PREFS: Mutex<Option<DemandPrefs>> = Mutex::new(None);

pub fn prefs() -> DemandPrefs {
    let mut p = PREFS.lock().unwrap_or_else(|e| e.into_inner());
    *p.get_or_insert_with(|| super::theme::saved_demand().unwrap_or_default())
}

fn set_prefs(new: DemandPrefs) {
    let mut p = PREFS.lock().unwrap_or_else(|e| e.into_inner());
    if p.as_ref() != Some(&new) {
        *p = Some(new);
        super::theme::save_demand(&new);
    }
}

/// [r4-leisure] The window's open state and place, from what the player
/// did this frame: saved when it changed and the mouse is up (a drag is
/// saved once, where it ends).
pub fn remember(ctx: &egui::Context, open: bool, pos: Option<egui::Pos2>) {
    let mut p = prefs();
    let dragging = ctx.input(|i| i.pointer.any_down());
    let pos = if dragging { p.pos } else { pos.map(|q| [q.x.round(), q.y.round()]).or(p.pos) };
    if p.note(open, pos) {
        set_prefs(p);
    }
}

/// The demand panel. `compact` is the window: narrow bars, reasons on
/// hover only; City Data's has the hints under the bars.
pub fn panel(ui: &mut egui::Ui, p: &UiCtx, compact: bool) {
    let w = &p.sim.world;
    let e = &w.economy;
    let width = if compact { WINDOW_BAR_WIDTH } else { super::detail::BAR_WIDTH };
    let height = if compact { 16.0 } else { 18.0 };
    // [w1] With the sites under way: a lighter extension for the demand
    // they already meet, so a bar that looks high but is being built for
    // reads as such.
    let mut prefs = prefs();
    let mut include = prefs.include_sites;
    if ui
        .checkbox(&mut include, egui::RichText::new("include planned / under construction").small())
        .on_hover_text("Adds a lighter extension to each bar: the demand the buildings going up there will meet once they open. A short dark bar with a long light one: the demand is already being built for.")
        .changed()
    {
        prefs.include_sites = include;
        set_prefs(prefs);
    }
    let (under, sites) = if include { (w.demand_under_way(), w.sites_by_use()) } else { ([0.0; 6], [(0, 0, 0); 6]) };
    let subs = w.commercial_demand();
    for lu in LandUse::ZONED {
        let col = super::theme::rgb_f(p.assets.palette.zone_color(lu));
        let i = lu.index();
        // [r4-leisure] Commercial: the highest of its sub-zones, with
        // leisure's sites when it is leisure that leads.
        let (demand, building, site) = if lu == LandUse::Commercial {
            let overall = subs.iter().copied().fold(0.0, f32::max);
            let shops = subs[..CommercialSub::Leisure.index()].iter().copied().fold(0.0, f32::max);
            let j = if subs[CommercialSub::Leisure.index()] > shops { LandUse::Entertainment.index() } else { i };
            (overall, under[j], sites[j])
        } else {
            (e.demand_for(lu), under[i], sites[i])
        };
        let reason = if lu == LandUse::Commercial { Some(commercial_reason(p, &subs)) } else { e.demand_reason(lu) };
        let resp = bar(ui, lu.label(), demand, if include { Some((building, site)) } else { None }, col, width, height);
        if let Some(r) = &reason {
            resp.on_hover_text(r.clone());
        }
        if lu == LandUse::Commercial {
            sub_zone_row(ui, &subs, col, width, compact);
        }
        if let (Some(r), false) = (&reason, compact) {
            if lu != LandUse::Commercial {
                hint(ui, r);
            }
        }
    }
    if include && !compact {
        hint(ui, "Dark: demand still open. Light: demand the sites going up already meet. Venues follow the population and whether they pay, so their sites add no light part.");
    }
}

/// Commercial's sub-zones in a row of small bars under its bar.
fn sub_zone_row(ui: &mut egui::Ui, subs: &[f32; 4], col: Color32, width: f32, compact: bool) {
    let gap = 4.0;
    let each = (width - 12.0 - gap * 3.0) / 4.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.add_space(12.0);
        for s in CommercialSub::ALL {
            let d = subs[s.index()];
            // The name and figure, a thin bar under them: readable at any fill.
            let font = egui::FontId::proportional(if compact { 10.0 } else { 11.5 });
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(each, font.size + 6.0), egui::Sense::hover());
            let painter = ui.painter();
            let name = if compact { short(s) } else { s.label() };
            painter.text(rect.left_top(), egui::Align2::LEFT_TOP, format!("{name} {:.0}%", d * 100.0), font, ui.visuals().text_color());
            let track = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 4.0), rect.max);
            painter.rect_filled(track, 1.0, ui.visuals().extreme_bg_color);
            let fill = egui::Rect::from_min_max(track.min, egui::pos2(track.left() + track.width() * d.clamp(0.0, 1.0), track.bottom()));
            painter.rect_filled(fill, 1.0, col.gamma_multiply(0.8));
            let kinds: Vec<&str> = s.kinds().iter().map(|k| k.label()).collect();
            resp.on_hover_text(format!("{}: {:.0}%. {} Grows: {}.", s.label(), d * 100.0, s.note(), kinds.join(", ")));
        }
    });
}

fn short(s: CommercialSub) -> &'static str {
    match s {
        CommercialSub::Neighbourhood => "local",
        CommercialSub::Retail => "retail",
        CommercialSub::Offices => "office",
        CommercialSub::Leisure => "leisure",
    }
}

/// Why commercial demand is what it is: the shops' and offices' demand,
/// which sub-zones the market would open now, and nights out.
fn commercial_reason(p: &UiCtx, subs: &[f32; 4]) -> String {
    let w = &p.sim.world;
    let shops = w.economy.demand_for(LandUse::Commercial);
    let closed: Vec<&str> = [CommercialSub::Neighbourhood, CommercialSub::Retail, CommercialSub::Offices].into_iter().filter(|s| subs[s.index()] <= 0.0).map(|s| s.label()).collect();
    let mut t = format!("Shops and offices {:.0}%: they follow the jobs the town needs.", shops * 100.0);
    if !closed.is_empty() {
        t.push_str(&format!(" The market would open nothing new in {} now; lots zoned for them wait.", closed.join(" or ")));
    }
    let leisure = w.economy.demand_reason(LandUse::Entertainment).unwrap_or_default();
    t.push_str(&format!(" Leisure {:.0}%: {leisure}", subs[CommercialSub::Leisure.index()] * 100.0));
    t.push_str(" Lots on auto pick their sub-zone from the street, the homes near, land value, demand and the district's story.");
    t
}

/// A demand bar: the open demand dark, with `sites` the demand the sites
/// going up already meet as a lighter extension and the sites in the text.
fn bar(ui: &mut egui::Ui, label: &str, demand: f32, sites: Option<(f32, (u32, u32, u32))>, col: Color32, width: f32, height: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let x = |t: f32| rect.left() + rect.width() * t.clamp(0.0, 1.0);
    let dark = egui::Rect::from_min_max(rect.min, egui::pos2(x(demand), rect.bottom()));
    painter.rect_filled(dark, 3.0, col.gamma_multiply(0.7));
    let (building, (n, homes, jobs)) = sites.unwrap_or((0.0, (0, 0, 0)));
    if building > 0.0 {
        let light = egui::Rect::from_min_max(egui::pos2(x(demand), rect.top()), egui::pos2(x(demand + building), rect.bottom()));
        painter.rect_filled(light, 0.0, col.gamma_multiply(0.28));
        painter.rect_stroke(light, 0.0, egui::Stroke::new(1.0, col.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    }
    let what = match (homes, jobs) {
        (0, 0) => String::new(),
        (h, 0) => format!(", {h} homes"),
        (0, j) => format!(", {j} jobs"),
        (h, j) => format!(", {h} homes, {j} jobs"),
    };
    let text = if n > 0 { format!("{label}  ·  {:.0}%  ·  {n} site{} going up{what}", demand * 100.0, if n == 1 { "" } else { "s" }) } else { format!("{label}  ·  {:.0}%", demand * 100.0) };
    let font = if height < 17.0 { egui::FontId::proportional(11.5) } else { egui::TextStyle::Body.resolve(ui.style()) };
    // A dark shadow under light text, so it reads over the fill and the track.
    painter.text(rect.left_center() + egui::vec2(7.0, 1.0), egui::Align2::LEFT_CENTER, &text, font.clone(), Color32::from_black_alpha(220));
    painter.text(rect.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, text, font, Color32::from_rgb(245, 240, 228));
    if sites.is_some() {
        resp.on_hover_text(format!("Open demand {:.0}%; the sites going up meet about {:.0}% more.", demand * 100.0, building * 100.0))
    } else {
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_state_is_saved_and_old_settings_load_without_it() {
        let mut p = DemandPrefs::default();
        assert!(!p.open && p.pos.is_none());
        // Opened at a place: news.
        assert!(p.note(true, Some([900.0, 80.0])));
        // The same again, or a wobble under a point: not.
        assert!(!p.note(true, Some([900.4, 80.2])));
        assert_eq!(p.pos, Some([900.0, 80.0]));
        // Dragged, then closed: each saved.
        assert!(p.note(true, Some([640.0, 120.0])));
        assert!(p.note(false, Some([640.0, 120.0])));
        p.include_sites = true;
        // It round-trips through settings.ron.
        let text = super::super::theme::settings_text_with_demand("", &p);
        assert_eq!(super::super::theme::demand_from_settings_text(&text), Some(p));
        // A settings.ron from before the window: no state, and the others
        // in it are kept when the window's is added.
        let old = "(ui_scale: Some(1.15), speech_bubbles: Some(true))";
        assert_eq!(super::super::theme::demand_from_settings_text(old), None);
        let both = super::super::theme::settings_text_with_demand(old, &p);
        assert!(both.contains("1.15") && both.contains("speech_bubbles"));
        assert_eq!(super::super::theme::demand_from_settings_text(&both), Some(p));
    }
}
