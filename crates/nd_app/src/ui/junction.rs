//! [tools] The Intersection editor: Edit mode, click a junction or a
//! crosswalk. A modal with a top-down orthographic render of the roads
//! around it (no buildings: `render::junction_view` films the road layer
//! into a texture) and over it the approaches' lane arrows and the
//! crosswalks, clickable; beside it the junction's settings:
//!
//! - signals: as the junction decides, forced on, or none (yield);
//! - each phase's green, and the protected left arrow (as needed, always,
//!   never) and its length;
//! - left turns allowed or forbidden, for the whole junction or per street;
//! - lane permissions per lane (left, straight, right: the player's lane
//!   arrows);
//! - per crosswalk an upgrade: a push-button signal or a footbridge.
//!
//! Everything goes through `nd_sim` (`World::set_junction_rule`,
//! `set_lane_permissions`, `set_crossing_upgrade`), saved with the city;
//! the remote API's `intersection` command does the same.

use bevy_egui::egui;
use nd_sim::junctions::{CrossingUpgrade, JunctionRule, LeftArrow, SignalMode, ARROW_MAX_SECONDS, ARROW_MIN_SECONDS, GREEN_MAX, GREEN_MIN};
use nd_sim::traffic::{arrows_label, LEFT, RIGHT, STRAIGHT};
use nd_sim::{IntersectionView, Vec2 as SVec2, World};

use super::{UiCtx, WARN};

/// Pixels of the road view.
pub const VIEW_PX: f32 = 440.0;
/// How far the road view zooms (metres across).
pub const EXTENT_MIN: f32 = 30.0;
pub const EXTENT_MAX: f32 = 300.0;

/// The open editor.
#[derive(Clone, Debug)]
pub struct EditorState {
    /// The junction (or lone crosswalk) edited.
    pub at: SVec2,
    /// Metres across the road view.
    pub extent: f32,
    /// The road view's texture, once `render::junction_view` has made it.
    pub texture: Option<egui::TextureId>,
    /// The approach (index into the view's approaches) shown in detail.
    pub approach: Option<usize>,
    /// The crosswalk (walk graph index) shown in detail.
    pub crossing: Option<u32>,
    /// The last edit's outcome.
    pub message: Option<(String, bool)>,
}

impl EditorState {
    /// Open on whatever is at `p` (a junction, else a crosswalk).
    pub fn open(w: &World, p: SVec2) -> Option<Self> {
        let v = w.intersection_view(p)?;
        let extent = (v.radius * 2.0 + 64.0).clamp(64.0, 150.0);
        Some(Self { at: v.at, extent, texture: None, approach: None, crossing: v.crossings.first().filter(|_| v.node.is_none()).map(|c| c.index), message: None })
    }

    /// Where a point of the ground lands on the road view drawn in `rect`.
    pub fn to_view(&self, rect: egui::Rect, p: SVec2) -> egui::Pos2 {
        let u = (p.x - self.at.x) / self.extent + 0.5;
        let v = (p.y - self.at.y) / self.extent + 0.5;
        egui::pos2(rect.left() + u * rect.width(), rect.top() + v * rect.height())
    }
}

/// What a click on the map in Edit mode opens: a junction or a crosswalk
/// under the pointer.
pub fn target_at(w: &World, p: SVec2) -> Option<SVec2> {
    if w.crosswalk_at(p, 2.5).is_some() || w.junction_at(p, 0.0).is_some() {
        w.intersection_view(p).map(|v| v.at)
    } else {
        None
    }
}

fn arrow_text(a: u8) -> String {
    arrows_label(a)
}

/// Where a street arrives from, by the way it leaves the junction (north
/// is up on the view: -y).
pub fn compass(dir: SVec2) -> &'static str {
    let a = dir.y.atan2(dir.x).to_degrees();
    match a {
        a if (-22.5..22.5).contains(&a) => "east",
        a if (22.5..67.5).contains(&a) => "south-east",
        a if (67.5..112.5).contains(&a) => "south",
        a if (112.5..157.5).contains(&a) => "south-west",
        a if (-67.5..-22.5).contains(&a) => "north-east",
        a if (-112.5..-67.5).contains(&a) => "north",
        a if (-157.5..-112.5).contains(&a) => "north-west",
        _ => "west",
    }
}

/// The modal, when open.
pub fn window(ctx: &egui::Context, p: &mut UiCtx) {
    let Some(mut st) = p.tools.junction_editor.clone() else { return };
    let Some(view) = p.sim.world.intersection_view(st.at) else {
        p.tools.junction_editor = None;
        return;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("junction_editor")).show(ctx, |ui| {
        ui.set_max_width(VIEW_PX + 420.0);
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("Intersection editor").color(egui::Color32::from_rgb(120, 230, 255)));
            ui.label(egui::RichText::new(title(&view)).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            road_view(ui, &mut st, &view);
            ui.vertical(|ui| {
                ui.set_width(400.0);
                egui::ScrollArea::vertical().max_height(VIEW_PX + 20.0).show(ui, |ui| controls(ui, p, &mut st, &view));
            });
        });
        if let Some((msg, ok)) = &st.message {
            ui.separator();
            ui.label(egui::RichText::new(msg).color(if *ok { egui::Color32::from_rgb(140, 230, 160) } else { WARN }));
        }
    });
    if modal.should_close() {
        close = true;
    }
    p.tools.junction_editor = if close { None } else { Some(st) };
}

fn title(v: &IntersectionView) -> String {
    if v.node.is_none() {
        return "a crosswalk".into();
    }
    let mut names: Vec<&str> = v.approaches.iter().map(|a| a.name.as_str()).collect();
    names.dedup();
    names.sort();
    names.dedup();
    let kind = if v.roundabout {
        "roundabout"
    } else if v.signalized {
        "signalised"
    } else {
        "unsignalised"
    };
    format!("{} ({kind}, {} streets)", names.join(" \u{00d7} "), v.approaches.len())
}

/// The road view with the lane arrows and crosswalks over it.
fn road_view(ui: &mut egui::Ui, st: &mut EditorState, v: &IntersectionView) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(VIEW_PX, VIEW_PX), egui::Sense::click());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(6, 8, 14));
    match st.texture {
        Some(tex) => {
            painter.image(tex, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "rendering the roads...", crate::ui::theme::body(), egui::Color32::GRAY);
        }
    }
    let click = resp.clicked().then(|| resp.interact_pointer_pos()).flatten();
    // The wheel zooms the view.
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            st.extent = (st.extent * (1.0 - dy * 0.002)).clamp(EXTENT_MIN, EXTENT_MAX);
        }
    }
    // The approaches: a chip with each arriving lane's arrows.
    for (i, a) in v.approaches.iter().enumerate() {
        if a.arrows.is_empty() {
            continue;
        }
        let right_in = SVec2::new(a.dir.y, -a.dir.x);
        let spot = v.at + a.dir * (v.radius + 14.0) + right_in * 4.0;
        let c = st.to_view(rect, spot);
        let text = a.arrows.iter().map(|&x| arrow_text(x)).collect::<Vec<_>>().join(" | ");
        let sel = st.approach == Some(i);
        let galley = painter.layout_no_wrap(text, crate::ui::theme::body(), if a.no_left { egui::Color32::from_rgb(255, 190, 90) } else { egui::Color32::WHITE });
        let size = galley.size() + egui::vec2(12.0, 8.0);
        // Kept inside the view when zoomed in.
        let c = egui::pos2(c.x.clamp(rect.left() + size.x * 0.5 + 2.0, rect.right() - size.x * 0.5 - 2.0), c.y.clamp(rect.top() + size.y * 0.5 + 2.0, rect.bottom() - size.y * 0.5 - 2.0));
        let r = egui::Rect::from_center_size(c, size);
        painter.rect_filled(r, 5.0, if sel { egui::Color32::from_rgb(20, 110, 140) } else { egui::Color32::from_black_alpha(190) });
        painter.rect_stroke(r, 5.0, egui::Stroke::new(1.0, if a.fixed { egui::Color32::from_rgb(120, 230, 255) } else { egui::Color32::from_gray(90) }), egui::StrokeKind::Outside);
        painter.galley(r.center() - galley.size() * 0.5, galley, egui::Color32::WHITE);
        if click.is_some_and(|p| r.contains(p)) {
            st.approach = Some(i);
            st.crossing = None;
        }
    }
    // The crosswalks: a marker in the middle of each.
    for c in &v.crossings {
        let m = st.to_view(rect, c.mid);
        let (label, col) = match c.upgrade {
            CrossingUpgrade::None => ("", egui::Color32::from_gray(200)),
            CrossingUpgrade::Button => ("B", egui::Color32::from_rgb(255, 200, 60)),
            CrossingUpgrade::Footbridge => ("F", egui::Color32::from_rgb(90, 230, 255)),
        };
        // A footbridge: its walkway and stairs, outlined.
        if let Some((top, foot, width)) = c.footbridge {
            let px = rect.width() / st.extent;
            let stroke = egui::Stroke::new((width * px).max(3.0), egui::Color32::from_rgba_unmultiplied(90, 230, 255, 150));
            painter.line_segment([st.to_view(rect, top[0]), st.to_view(rect, top[1])], stroke);
            for i in 0..2 {
                painter.line_segment([st.to_view(rect, top[i]), st.to_view(rect, foot[i])], egui::Stroke::new((width * px * 0.8).max(2.0), egui::Color32::from_rgba_unmultiplied(90, 230, 255, 110)));
            }
        }
        let a = st.to_view(rect, c.ends[0]);
        let b = st.to_view(rect, c.ends[1]);
        // [footbridge] A crosswalk its bridge replaced, faint.
        painter.line_segment([a, b], egui::Stroke::new(if st.crossing == Some(c.index) { 4.0 } else { 2.0 }, col.gamma_multiply(if c.closed { 0.25 } else { 0.8 })));
        painter.circle_filled(m, 9.0, egui::Color32::from_black_alpha(200));
        painter.circle_stroke(m, 9.0, egui::Stroke::new(1.5, col));
        if !label.is_empty() {
            painter.text(m, egui::Align2::CENTER_CENTER, label, crate::ui::theme::small(), col);
        }
        if click.is_some_and(|p| p.distance(m) < 12.0) {
            st.crossing = Some(c.index);
            st.approach = None;
        }
    }
    painter.text(rect.left_bottom() + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("{:.0} m across \u{00b7} north up \u{00b7} wheel zooms", st.extent), crate::ui::theme::small(), egui::Color32::from_gray(160));
}

/// The settings beside the view.
fn controls(ui: &mut egui::Ui, p: &mut UiCtx, st: &mut EditorState, v: &IntersectionView) {
    let w = &mut p.sim.world;
    if v.node.is_some() {
        if v.roundabout {
            ui.label("A roundabout's junctions give way by the ring's own rules; only its crosswalks can be upgraded.");
        } else {
            junction_controls(ui, w, st, v);
        }
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Crosswalks").strong());
    if v.crossings.is_empty() {
        ui.label(egui::RichText::new("None here.").weak());
    }
    for c in &v.crossings {
        let street = w.roads.nearest_where(c.mid, |s| s.is_street()).and_then(|(sid, ..)| w.streets.of(sid).map(|s| s.name.clone())).unwrap_or_else(|| "the street".into());
        let sel = st.crossing == Some(c.index);
        ui.horizontal(|ui| {
            if ui.selectable_label(sel, format!("Across {street}")).clicked() {
                st.crossing = Some(c.index);
            }
            let mut pick = c.upgrade;
            egui::ComboBox::from_id_salt(("crossing", c.index)).selected_text(upgrade_label(c.upgrade)).show_ui(ui, |ui| {
                for u in [CrossingUpgrade::None, CrossingUpgrade::Button, CrossingUpgrade::Footbridge] {
                    let enabled = !(u == CrossingUpgrade::Button && c.signalised);
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.selectable_value(&mut pick, u, format!("{}{}", upgrade_label(u), if u.cost() > 0.0 { format!(" (${:.0})", u.cost()) } else { String::new() }));
                    });
                }
            });
            if pick != c.upgrade {
                st.message = Some(match w.set_crossing_upgrade(c.index, pick) {
                    Ok(()) => (format!("Crosswalk across {street}: {}.", upgrade_label(pick).to_lowercase()), true),
                    Err(e) => (format!("Not done: {e}."), false),
                });
            }
        });
        if let Some(light) = c.light {
            ui.label(egui::RichText::new(format!("    push button: {}", light.label())).small().weak());
        }
        if c.signalised && c.upgrade == CrossingUpgrade::None {
            ui.label(egui::RichText::new("    walk lights run with the junction's signals").small().weak());
        }
        // [footbridge] The crosswalk under a footbridge goes, unless kept.
        if c.upgrade == CrossingUpgrade::Footbridge {
            let mut keep = c.kept;
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.checkbox(&mut keep, "Keep the crosswalk under it").on_hover_text("Off: the footbridge replaces the crosswalk (its paint and walk lights go) and everyone crossing here climbs over. On: the crosswalk stays at grade and walkers take the bridge only when it is no real detour.");
            });
            if keep != c.kept {
                st.message = Some(match w.set_crosswalk_kept(c.index, keep) {
                    Ok(()) => (format!("Crosswalk across {street}: {}.", if keep { "kept under the footbridge" } else { "replaced by the footbridge" }), true),
                    Err(e) => (format!("Not done: {e}."), false),
                });
            }
        }
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("A push button stops traffic only when someone presses it. A footbridge replaces the crosswalk: walkers climb over without holding anyone up (keep the crosswalk to let them choose).").small().weak());
}

fn upgrade_label(u: CrossingUpgrade) -> &'static str {
    match u {
        CrossingUpgrade::None => "Plain crosswalk",
        CrossingUpgrade::Button => "Push-button signal",
        CrossingUpgrade::Footbridge => "Footbridge",
    }
}

fn junction_controls(ui: &mut egui::Ui, w: &mut World, st: &mut EditorState, v: &IntersectionView) {
    let mut rule: JunctionRule = v.rule.clone();
    let before = rule.clone();
    ui.label(egui::RichText::new("Signals").strong());
    ui.horizontal(|ui| {
        ui.selectable_value(&mut rule.signal, SignalMode::Auto, "Auto").on_hover_text("As the junction's shape decides: signals at busy crossroads.");
        ui.selectable_value(&mut rule.signal, SignalMode::Signals, "Signals");
        ui.selectable_value(&mut rule.signal, SignalMode::Unsignalised, "None (yield)");
        ui.label(egui::RichText::new(if v.signalized { "  now: signalised" } else { "  now: no signals" }).weak());
    });
    if v.signalized {
        ui.label(egui::RichText::new("Phase greens").strong());
        for (k, ph) in v.phases.iter().enumerate() {
            let mut names: Vec<String> = ph.approaches.iter().filter_map(|&ap| v.approaches.get(ap)).map(|a| short(&a.name)).collect();
            names.dedup();
            ui.horizontal(|ui| {
                let mut g = ph.green;
                let label = format!("{}{} {}", if ph.running { "\u{25b6} " } else { "" }, k + 1, names.join(" + "));
                if ui.add(egui::Slider::new(&mut g, GREEN_MIN..=GREEN_MAX).step_by(1.0).suffix(" s").text(label)).changed() {
                    for &ap in &ph.approaches {
                        if let Some(a) = v.approaches.get(ap) {
                            rule.set_green(a.dir, Some(g));
                        }
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Protected left:");
            ui.selectable_value(&mut rule.left_arrow, LeftArrow::Auto, "When needed");
            ui.selectable_value(&mut rule.left_arrow, LeftArrow::Always, "Always");
            ui.selectable_value(&mut rule.left_arrow, LeftArrow::Never, "Never");
        });
        if rule.left_arrow != LeftArrow::Never {
            let mut s = if rule.arrow_seconds > 0.0 { rule.arrow_seconds } else { nd_sim::traffic::Intersection::DEFAULT_ARROW };
            if ui.add(egui::Slider::new(&mut s, ARROW_MIN_SECONDS..=ARROW_MAX_SECONDS).step_by(1.0).suffix(" s").text("left arrow")).changed() {
                rule.arrow_seconds = s;
            }
        }
        if ui.small_button("Default timing").clicked() {
            rule.greens.clear();
            rule.arrow_seconds = 0.0;
            rule.left_arrow = LeftArrow::Auto;
        }
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Left turns").strong());
    let possible: Vec<&nd_sim::ApproachView> = v.approaches.iter().filter(|a| a.left_possible).collect();
    let mut all = possible.iter().all(|a| !rule.left_forbidden(a.dir));
    if ui.checkbox(&mut all, "Allow left turns").changed() {
        for a in &possible {
            rule.set_left_forbidden(a.dir, !all);
        }
    }
    for a in &possible {
        let mut ok = !rule.left_forbidden(a.dir);
        if ui.checkbox(&mut ok, format!("  from {} ({})", a.name, compass(a.dir))).changed() {
            rule.set_left_forbidden(a.dir, !ok);
        }
    }
    if rule != before {
        st.message = Some(match w.set_junction_rule(rule) {
            Ok(()) => ("Junction settings applied.".into(), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
    // Lane permissions.
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Lane permissions").strong());
    ui.label(egui::RichText::new("Click a street's arrows on the view, or pick it here. Each lane: left, straight, right.").small().weak());
    for (i, a) in v.approaches.iter().enumerate() {
        if a.arrows.is_empty() {
            continue;
        }
        let sel = st.approach == Some(i);
        let head = format!("{} from the {} ({} lane{}{})", a.name, compass(a.dir), a.arrows.len(), super::fmt::s(a.arrows.len()), if a.fixed { ", set" } else { "" });
        if ui.selectable_label(sel, head).clicked() {
            st.approach = if sel { None } else { Some(i) };
        }
        if !sel {
            continue;
        }
        let k = v.intersection.unwrap_or(usize::MAX);
        let mut arrows = a.arrows.clone();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("    leftmost:");
            for (lane, x) in arrows.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (bit, name) in [(LEFT, "L"), (STRAIGHT, "S"), (RIGHT, "R")] {
                        let on = *x & bit != 0;
                        let offered = a.exits & bit != 0;
                        let resp = ui.add_enabled(offered, egui::Button::selectable(on, name)).on_hover_text(format!("lane {}: {}", lane + 1, arrows_label(*x)));
                        if resp.clicked() {
                            *x ^= bit;
                            changed = true;
                        }
                    }
                });
            }
        });
        if changed {
            st.message = Some(match w.set_lane_permissions(k, a.segment, Some(arrows)) {
                Ok(()) => (format!("{}: lanes set.", a.name), true),
                Err(e) => (format!("Not done: {e}."), false),
            });
        }
        if a.fixed && ui.small_button("    back to the automatic arrows").clicked() {
            st.message = Some(match w.set_lane_permissions(k, a.segment, None) {
                Ok(()) => (format!("{}: automatic arrows.", a.name), true),
                Err(e) => (format!("Not done: {e}."), false),
            });
        }
        if a.queued > 0 {
            ui.label(egui::RichText::new(format!("    {} queued", a.queued)).small().weak());
        }
    }
}

fn short(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}
