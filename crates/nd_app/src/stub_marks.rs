//! [dock-access] "Connect here" marks on the free ends of the street stubs
//! of docks and stations that are not on the road network yet: a pulsing
//! ring and post at each end (gizmos) and a short label over it (egui).
//! Draw a road onto the ring with the Road tool and it snaps to the stub's
//! end like any other road end.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_sim::Vec2 as SVec2;

use crate::overlay::shapes::flat_at;
use crate::sim::Sim;
use crate::ui::UiFlags;

pub struct StubMarksPlugin;

impl Plugin for StubMarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stubs>().add_systems(Update, stub_rings.in_set(crate::overlay::OverlaySet::Draw)).add_systems(EguiPrimaryContextPass, stub_labels);
    }
}

/// [overlay] The unjoined stubs as `stub_rings` last listed them, with the
/// `Sim` change tick they were read at: the labels reuse the list while
/// the tick holds (nothing has touched the sim since) rather than walk
/// every building again (review P-O2).
#[derive(Resource, Default)]
struct Stubs {
    at: Option<bevy::ecs::change_detection::Tick>,
    list: Vec<(nd_sim::BuildingId, Vec<SVec2>)>,
    /// [r2-traffic] Building sites no truck can get to.
    sites: Vec<(nd_sim::BuildingId, SVec2, nd_sim::SiteBlock)>,
}

/// The marks' colour: the amber of an unfinished connection.
fn amber(pulse: f32) -> Color {
    Color::linear_rgb(3.6 + pulse, 1.6 + pulse * 0.5, 0.2)
}

fn stub_rings(mut gizmos: Gizmos, sim: Res<Sim>, time: Res<Time>, mut stubs: ResMut<Stubs>) {
    let w = &sim.world;
    stubs.list = w.unjoined_stubs();
    stubs.sites = w.blocked_sites();
    stubs.at = Some(sim.last_changed());
    let t = time.elapsed_secs();
    let pulse = (t * 2.6).sin() * 0.5 + 0.5;
    let col = amber(pulse);
    // [r2-traffic] A site no truck can get to: a red ring on its plot and
    // a post, like a road end waiting for its road (K6).
    let red = Color::linear_rgb(3.4 + pulse, 0.35, 0.2);
    for &(_, c, _) in &stubs.sites {
        let at = Vec3::new(c.x, w.surface_height(c) + 0.4, c.y);
        gizmos.circle(flat_at(at), 9.0, red);
        gizmos.circle(flat_at(at), 9.0 + 3.0 * pulse, red.with_alpha(0.5));
        gizmos.line(at, at + Vec3::Y * (18.0 + 2.0 * pulse), red);
    }
    for (b, ends) in &stubs.list {
        let gate = w.building_entrance(*b);
        for &e in ends {
            let y = w.surface_height(e) + 0.4;
            let at = Vec3::new(e.x, y, e.y);
            // A steady ring the size of a road end, and one that swells
            // out from it like a sonar ping.
            gizmos.circle(flat_at(at), 6.0, col);
            let ping = (t * 0.9).fract();
            gizmos.circle(flat_at(at), 6.0 + 12.0 * ping, Color::linear_rgba(3.6, 1.6, 0.2, 1.0 - ping));
            gizmos.line(at, at + Vec3::Y * (14.0 + 2.0 * pulse), col);
            // A dotted lead from the gate along the stub, so the end reads as
            // belonging to that dock or station.
            if let Some(g) = gate {
                let n = (g.distance(e) / 4.0).ceil().max(1.0) as usize;
                for i in (0..n).step_by(2) {
                    let (a, c) = (g.lerp(e, i as f32 / n as f32), g.lerp(e, ((i + 1) as f32 / n as f32).min(1.0)));
                    gizmos.line(Vec3::new(a.x, w.surface_height(a) + 0.5, a.y), Vec3::new(c.x, w.surface_height(c) + 0.5, c.y), col.with_alpha(0.6));
                }
            }
        }
    }
}

fn stub_labels(mut contexts: EguiContexts, sim: Res<Sim>, flags: Res<UiFlags>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>, cache: Res<Stubs>) -> Result {
    let w = &sim.world;
    let (fresh, fresh_sites);
    let (stubs, sites) = if cache.at == Some(sim.last_changed()) {
        (&cache.list, &cache.sites)
    } else {
        fresh = w.unjoined_stubs();
        fresh_sites = w.blocked_sites();
        (&fresh, &fresh_sites)
    };
    if stubs.is_empty() && sites.is_empty() {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let screen = flags.map_rect(ctx.viewport_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("stub_labels"))).with_clip_rect(screen);
    let to_screen = |p: SVec2, up: f32| -> Option<egui::Pos2> {
        let v = camera.world_to_viewport(gt, Vec3::new(p.x, w.surface_height(p) + up, p.y)).ok()?;
        Some(crate::ui::theme::to_points(ctx, v))
    };
    let text_col = egui::Color32::from_rgb(255, 196, 90);
    for (b, ends) in stubs {
        let name = w.buildings.get(*b).map_or("", |x| x.name.as_str());
        for &e in ends {
            let Some(at) = to_screen(e, 16.0) else { continue };
            if !screen.contains(at) {
                continue;
            }
            let text = format!("connect a road here\n{name}: not on the road network");
            for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0), egui::vec2(1.0, -1.0), egui::vec2(-1.0, -1.0)] {
                painter.text(at + off, egui::Align2::CENTER_BOTTOM, &text, crate::ui::theme::small(), egui::Color32::BLACK);
            }
            painter.text(at, egui::Align2::CENTER_BOTTOM, &text, crate::ui::theme::small(), text_col);
        }
    }
    let site_col = egui::Color32::from_rgb(255, 120, 90);
    for &(b, c, why) in sites {
        let Some(at) = to_screen(c, 20.0) else { continue };
        if !screen.contains(at) {
            continue;
        }
        let name = w.buildings.get(b).map_or("", |x| x.name.as_str());
        let text = format!("{}
{name}: building site on hold", why.text());
        for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0), egui::vec2(1.0, -1.0), egui::vec2(-1.0, -1.0)] {
            painter.text(at + off, egui::Align2::CENTER_BOTTOM, &text, crate::ui::theme::small(), egui::Color32::BLACK);
        }
        painter.text(at, egui::Align2::CENTER_BOTTOM, &text, crate::ui::theme::small(), site_col);
    }
    Ok(())
}
