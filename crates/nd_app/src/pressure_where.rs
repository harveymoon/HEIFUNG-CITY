//! [r4-hunger] "Show where" for a held business pressure: City Data's
//! Economy, Overview (or `pressure {show_where}` through the API) sets
//! [`UiFlags::pressure_where`], and the map shows what the trade needs
//! (`World::lot_need`): a post at each of its anchors (the fishing ports,
//! the farms...) with the reach drawn round it, and the vacant lots of the
//! land uses it takes outlined by how they measure up (`World::lot_need_lots`):
//! green suits, amber too small, orange on a footpath, blue not on the water,
//! grey too far. A banner over the map says it in words.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_sim::pressure::PressureKind;
use nd_sim::{LotFit, LotId, LotNeed, Vec2 as SVec2};

use crate::overlay::shapes::flat_at;
use crate::sim::Sim;
use crate::ui::UiFlags;

pub struct PressureWherePlugin;

impl Plugin for PressureWherePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shown>().add_systems(Update, draw.in_set(crate::overlay::OverlaySet::Draw)).add_systems(EguiPrimaryContextPass, banner);
    }
}

/// What is shown, worked out again when the kind changes and once a
/// second after (placement walks the homes and lots: not every frame).
#[derive(Resource, Default)]
struct Shown {
    kind: Option<PressureKind>,
    age: f32,
    need: Option<LotNeed>,
    lots: Vec<(LotId, LotFit)>,
}

/// A lot outline's colour by how it measures up.
pub fn fit_color(f: LotFit) -> Color {
    match f {
        LotFit::Suits => Color::linear_rgb(0.3, 3.2, 0.8),
        LotFit::TooSmall => Color::linear_rgb(3.4, 2.0, 0.2),
        LotFit::NoStreet => Color::linear_rgb(3.4, 0.9, 0.2),
        LotFit::NotOnWater | LotFit::NoJettyRoom => Color::linear_rgb(0.3, 1.2, 3.4),
        LotFit::TooFar | LotFit::Taken | LotFit::WrongUse => Color::linear_rgba(0.6, 0.6, 0.7, 0.6),
    }
}

fn draw(mut gizmos: Gizmos, sim: Res<Sim>, flags: Res<UiFlags>, time: Res<Time>, mut shown: ResMut<Shown>) {
    let w = &sim.world;
    let Some(kind) = flags.pressure_where else {
        if shown.kind.is_some() {
            *shown = Shown::default();
        }
        return;
    };
    shown.age += time.delta_secs();
    if shown.kind != Some(kind) || shown.age > 1.0 {
        shown.kind = Some(kind);
        shown.age = 0.0;
        shown.need = w.lot_need(kind);
        shown.lots = w.lot_need_lots(kind);
    }
    let Some(need) = &shown.need else { return };
    let pulse = (time.elapsed_secs() * 2.4).sin() * 0.5 + 0.5;
    let gold = Color::linear_rgb(3.6 + pulse, 2.4 + pulse * 0.5, 0.4);
    for &a in &need.anchors {
        let at = Vec3::new(a.x, w.surface_height(a) + 0.6, a.y);
        gizmos.line(at, at + Vec3::Y * (40.0 + 4.0 * pulse), gold);
        gizmos.circle(flat_at(at), 12.0 + 4.0 * pulse, gold);
        // The reach, or a soft ring where it would rather be.
        let (r, col) = match need.reach {
            Some(r) => (r, gold),
            None => (300.0, gold.with_alpha(0.4)),
        };
        gizmos.circle(flat_at(at + Vec3::Y * 2.0), r, col).resolution(160);
    }
    for (id, fit) in &shown.lots {
        let Some(lot) = w.lots.get(*id) else { continue };
        let col = fit_color(*fit);
        let y = w.surface_height(lot.center) + 0.8;
        let pts: Vec<Vec3> = lot.polygon.iter().chain(lot.polygon.first()).map(|p| Vec3::new(p.x, y, p.y)).collect();
        gizmos.linestrip(pts, col);
        // A post on each, so they read from far up too.
        let c = Vec3::new(lot.center.x, y, lot.center.y);
        gizmos.line(c, c + Vec3::Y * 16.0, col);
        gizmos.circle(flat_at(c), 4.0, col);
    }
}

fn banner(mut contexts: EguiContexts, sim: Res<Sim>, flags: Res<UiFlags>, shown: Res<Shown>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    let Some(kind) = flags.pressure_where else { return Ok(()) };
    let Some(need) = &shown.need else { return Ok(()) };
    let w = &sim.world;
    let ctx = contexts.ctx_mut()?;
    let screen = flags.map_rect(ctx.viewport_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("pressure_where"))).with_clip_rect(screen);
    let text_col = egui::Color32::from_rgb(255, 210, 110);
    let outlined = |at: egui::Pos2, align: egui::Align2, text: &str, col: egui::Color32| {
        for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0), egui::vec2(1.0, -1.0), egui::vec2(-1.0, -1.0)] {
            painter.text(at + off, align, text, crate::ui::theme::small(), egui::Color32::BLACK);
        }
        painter.text(at, align, text, crate::ui::theme::small(), col);
    };
    let suits = shown.lots.iter().filter(|(_, f)| *f == LotFit::Suits).count();
    let mut lines = vec![format!("{}: {}", crate::ui::fmt::capitalise(kind.label()), need.text())];
    lines.push(if suits > 0 { format!("{suits} vacant lot{} suit now (green)", if suits == 1 { "" } else { "s" }) } else { "No vacant lot suits yet: green would. Amber too small, orange on a footpath, blue not on the water, grey too far.".to_string() });
    outlined(egui::pos2(screen.center().x, screen.top() + 40.0), egui::Align2::CENTER_TOP, &lines.join("\n"), text_col);
    if let Some((camera, gt)) = cams.iter().next() {
        // One label for anchors close together (a beach's few centres).
        let mut labelled: Vec<SVec2> = Vec::new();
        for &a in &need.anchors {
            if labelled.iter().any(|l| l.distance(a) < 200.0) {
                continue;
            }
            labelled.push(a);
            let Ok(v) = camera.world_to_viewport(gt, Vec3::new(a.x, w.surface_height(a) + 44.0, a.y)) else { continue };
            let at = crate::ui::theme::to_points(ctx, v);
            if !screen.contains(at) {
                continue;
            }
            let reach = match need.reach {
                Some(r) if r >= 1000.0 => format!("\nwithin {:.1} km", r / 1000.0),
                Some(r) => format!("\nwithin {r:.0} m"),
                None => String::new(),
            };
            outlined(at, egui::Align2::CENTER_BOTTOM, &format!("{}{reach}", need.near), text_col);
        }
    }
    Ok(())
}

/// Where "show where" flies the camera: the first anchor.
pub fn focus(need: &LotNeed) -> Option<SVec2> {
    need.anchors.first().copied()
}
