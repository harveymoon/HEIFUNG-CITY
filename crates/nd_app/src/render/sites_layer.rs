//! [w1] The Construction map layer (`construction`): every building site
//! over a dimmed city (`focus::Focus::Sites`), coloured by which way it is
//! going (`nd_sim::world::sites::SiteKind`):
//!
//! - **going up** (cyan): the plot outlined at the ground and a wire box
//!   standing to the planned final height, the part built so far drawn
//!   bright;
//! - **coming down, rebuild planned** (amber): the plot outlined and a
//!   dim wire box of the building that will follow;
//! - **coming down, lot left vacant** (red): the plot outlined.
//!
//! Each site carries a label with how far along it is and its planned
//! floors and height. Rebuilt every two seconds.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_sim::world::sites::SiteKind;
use nd_sim::World;

use super::Overlays;
use crate::sim::{Sim, SimSet};

pub(super) struct SitesLayerPlugin;

impl Plugin for SitesLayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, site_gizmos.in_set(SimSet::Sync).after(super::lift_movers))
            .add_systems(bevy_egui::EguiPrimaryContextPass, site_labels.after(crate::ui::side_panel));
    }
}

/// A site kind's colour (linear, over 1 so it blooms over the dimmed city).
pub fn site_color(kind: SiteKind) -> [f32; 3] {
    match kind {
        SiteKind::GoingUp => [0.25, 1.9, 2.4],
        SiteKind::Replacing => [2.4, 1.3, 0.15],
        SiteKind::Clearing => [2.4, 0.25, 0.2],
    }
}

/// One site's lines: the ground outline, and the box to the planned height
/// with the share built so far.
struct SiteLines {
    ring: Vec<Vec3>,
    /// The ring raised to the planned height and to the height built so
    /// far (`None` with nothing planned).
    top: Option<(f32, f32)>,
    kind: SiteKind,
}

fn collect(w: &World) -> Vec<SiteLines> {
    let mut out = Vec::new();
    for (bid, kind, progress) in w.construction_sites() {
        let Some(lot) = w.footprint(bid) else { continue };
        let Some(plan) = w.site_plan(bid) else { continue };
        let ground = w.surface_height(lot.center);
        let ring: Vec<Vec3> = lot
            .polygon
            .iter()
            .map(|p| {
                let q = *p + (lot.center - *p).normalize_or_zero() * 1.5;
                Vec3::new(q.x, ground + 0.8, q.y)
            })
            .collect();
        if ring.len() < 3 {
            continue;
        }
        let top = plan.next.as_ref().map(|n| {
            let h = n.height_m.max(3.0);
            let built = if kind == SiteKind::GoingUp { h * progress.clamp(0.0, 1.0) } else { 0.0 };
            (h, built)
        });
        out.push(SiteLines { ring, top, kind });
    }
    out
}

fn site_gizmos(sim: Res<Sim>, overlays: Res<Overlays>, lighting: Res<super::Lighting>, time: Res<Time>, mut cache: Local<Option<(f32, Vec<SiteLines>)>>, mut gizmos: Gizmos<super::layers::LoadGizmos>) {
    if lighting.heat || !overlays.construction {
        *cache = None;
        return;
    }
    let now = time.elapsed_secs();
    if cache.as_ref().is_none_or(|(at, _)| now - at >= 2.0) || sim.full_rebuild {
        *cache = Some((now, collect(&sim.world)));
    }
    let Some((_, sites)) = cache.as_ref() else { return };
    for s in sites {
        let c = site_color(s.kind);
        let bright = Color::linear_rgb(c[0], c[1], c[2]);
        let faint = Color::linear_rgb(c[0] * 0.3, c[1] * 0.3, c[2] * 0.3);
        let closed = || s.ring.iter().copied().chain(s.ring.first().copied());
        gizmos.linestrip(closed(), bright);
        let Some((h, built)) = s.top else { continue };
        // Planned height: a faint box; a going-up site's work so far bright.
        let up = |dy: f32| closed().map(move |p| p + Vec3::Y * dy);
        gizmos.linestrip(up(h), if s.kind == SiteKind::GoingUp { bright.with_alpha(0.8) } else { faint });
        for p in &s.ring {
            gizmos.line(*p, *p + Vec3::Y * h, faint);
            if built > 0.5 {
                gizmos.line(*p, *p + Vec3::Y * built, bright);
            }
        }
        if built > 0.5 {
            gizmos.linestrip(up(built), bright);
        }
    }
}

/// "↑ 40% · 12 fl, 37 m" over each site.
pub fn site_label(w: &World, bid: nd_sim::BuildingId) -> Option<(String, SiteKind)> {
    let p = w.site_plan(bid)?;
    let next = p.next.as_ref().map(|n| format!("{} fl, {:.0} m", n.floors, n.height_m));
    let text = match p.kind {
        SiteKind::GoingUp => format!("↑ {:.0}% · {}", p.progress * 100.0, next.unwrap_or_default()),
        SiteKind::Replacing => format!("↓ {:.0}% · then {}", p.progress * 100.0, next.unwrap_or_default()),
        SiteKind::Clearing => format!("↓ {:.0}% · {} fl coming down", p.progress * 100.0, p.coming_down_floors.unwrap_or(1)),
    };
    Some((text, p.kind))
}

fn site_labels(mut contexts: EguiContexts, sim: Res<Sim>, overlays: Res<Overlays>, flags: Res<crate::ui::UiFlags>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    if !overlays.construction {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let w = &sim.world;
    let screen = flags.map_rect(ctx.viewport_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("site_labels"))).with_clip_rect(screen);
    let cam = gt.translation();
    let mut shown = 0;
    for (bid, _, _) in w.construction_sites() {
        let Some(l) = w.building_lot(bid) else { continue };
        let at3 = Vec3::new(l.center.x, w.surface_height(l.center) + 3.0, l.center.y);
        if at3.distance(cam) > 1400.0 || shown > 200 {
            continue;
        }
        let Ok(v) = camera.world_to_viewport(gt, at3) else { continue };
        let at = crate::ui::theme::to_points(ctx, v);
        if !screen.contains(at) {
            continue;
        }
        let Some((text, kind)) = site_label(w, bid) else { continue };
        shown += 1;
        let c = site_color(kind);
        let k = c[0].max(c[1]).max(c[2]).max(1.0);
        let col = egui::Color32::from(egui::Rgba::from_rgb(c[0] / k, c[1] / k, c[2] / k));
        let galley = painter.layout_no_wrap(text, crate::ui::theme::small(), egui::Color32::WHITE);
        let rect = egui::Rect::from_center_size(at, galley.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(190));
        painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Outside);
        painter.galley(rect.min + egui::vec2(5.0, 2.0), galley, egui::Color32::WHITE);
    }
    Ok(())
}
