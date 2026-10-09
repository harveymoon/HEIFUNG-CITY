//! The plan preview: drawn gestures and roundabout effects.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

pub(super) fn dashes(mesh: &mut MeshData, a: SVec2, b: SVec2, width: f32, y: f32, dash: f32, gap: f32, color: [f32; 4]) {
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let d = (b - a) / len;
    let mut t = gap * 0.5;
    while t + dash <= len {
        mesh.ground_strip(&[a + d * t, a + d * (t + dash)], width, y, color);
        t += dash + gap;
    }
}

/// The projects as holograms: the active one bright (its roads, zones,
/// and every parcel whose land use it changes, filled in the new use's
/// colour or outlined grey where it unzones), the parked ones dim.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<PreviewTag>>,
) {
    let key = (sim.world.plan_version, sim.world.parcels.version, overlays.projects);
    if state.preview_key == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.preview_key = Some(key);
    despawn_all(&mut commands, &existing);
    let pv = sim.world.preview();
    let pal = &assets.palette;
    let mut glow = MeshData::default();
    let w = &sim.world;
    let unzoned = [0.55f32, 0.55, 0.6];
    // Parcels the active project re-zones.
    // [waterfront] Public beaches painted or unpainted show as sand.
    let beaches = w.planned_beaches();
    let planned = w.planned_zoning();
    for (i, p) in w.parcels.parcels.iter().enumerate() {
        let Some(&want) = planned.get(i) else { break };
        let beach_want = beaches.get(i).copied().unwrap_or(false);
        let beach_now = p.lot.and_then(|l| w.lots.get(l)).is_some_and(|l| l.shore.beach);
        if want == p.land_use && beach_want == beach_now {
            continue;
        }
        let mut outline = p.polygon.clone();
        if let Some(&first) = p.polygon.first() { outline.push(first); }
        match want {
            Some(lu) => {
                let c = if beach_want { BEACH_TINT } else { pal.zone_color(lu) };
                glow.ground_polygon(&p.polygon, 0.05, scale(c, 0.22));
                glow.ground_strip(&outline, 0.45, 0.21, scale(c, 1.2));
            }
            None => glow.ground_strip(&outline, 0.45, 0.21, scale(unzoned, 0.9)),
        }
    }
    // Parked projects, dim.
    if overlays.projects {
        for other in w.inactive_previews() {
            let dim = 0.35;
            // Their planned roads are holograms (`holo::sync_plan_holograms`).
            for (_, pts, _, _, _) in &other.preview.removed_roads {
                if let (Some(&a), Some(&b)) = (pts.first(), pts.last()) {
                    dashes(&mut glow, a, b, 0.5, 0.21, 4.0, 3.0, scale(pal.preview_remove, dim));
                }
            }
            for (_, poly, lu) in &other.preview.planned_zones {
                let mut outline = poly.clone();
                if let Some(&first) = poly.first() { outline.push(first); }
                glow.ground_strip(&outline, 0.6, 0.2, scale(pal.zone_color(*lu), 1.3 * dim));
            }
            for &(i, want) in &other.zoning {
                let Some(p) = w.parcels.parcels.get(i) else { continue };
                let mut outline = p.polygon.clone();
                if let Some(&first) = p.polygon.first() { outline.push(first); }
                let c = want.map_or(unzoned, |lu| pal.zone_color(lu));
                glow.ground_strip(&outline, 0.35, 0.2, scale(c, dim));
            }
        }
    }
    // Planned roads are drawn as holograms of the real road
    // (`holo::sync_plan_holograms`).
    for (_, pts, fwd, bwd, kind) in &pv.removed_roads {
        let w = preview_width(*kind, *fwd, *bwd);
        glow.ground_strip(pts, w, 0.22, scale(pal.preview_remove, 0.35));
        if let (Some(&a), Some(&b)) = (pts.first(), pts.last()) {
            dashes(&mut glow, a, b, 0.6, 0.23, 4.0, 3.0, rgba(pal.preview_remove));
        }
    }
    for (_, poly, lu) in &pv.planned_zones {
        let c = pal.zone_color(*lu);
        let mut outline = poly.clone();
        if let Some(&first) = poly.first() { outline.push(first); }
        glow.ground_strip(&outline, 0.8, 0.22, scale(c, 1.3));
        glow.ground_polygon(poly, 0.045, scale(c, 0.12));
    }
    for (_, poly) in &pv.removed_zones {
        let mut outline = poly.clone();
        if let Some(&first) = poly.first() { outline.push(first); }
        glow.ground_strip(&outline, 0.8, 0.22, rgba(pal.preview_remove));
    }
    for (_, fx) in &pv.planned_rings {
        draw_ring_effect(&mut glow, pal, fx, 1.0);
    }
    for (_, ring) in &pv.removed_rings {
        let road = ring.polyline(&[]).0;
        let wd = ring.half_width() * 2.0;
        glow.ground_strip(&road, wd, 0.22, scale(pal.preview_remove, 0.35));
        for w in road.windows(2).step_by(2) {
            dashes(&mut glow, w[0], w[1], 0.6, 0.23, 4.0, 3.0, rgba(pal.preview_remove));
        }
    }
    if !glow.is_empty() {
        spawn_glow(&mut commands, &mut meshes, &mats, &glow, PreviewTag);
    }
}

/// A roundabout as a hologram: the ring road with its direction of travel,
/// its inner kerb (the middle is open land), where the arms join, and the parts of roads it takes up
/// (dashed red). `k` dims it for a parked project.
pub(super) fn draw_ring_effect(glow: &mut MeshData, pal: &nd_procgen::Palette, fx: &nd_sim::plan::RingEffect, k: f32) {
    let Some(ring) = fx.ring else { return };
    let tint = if fx.blocked { pal.preview_remove } else { pal.preview };
    let wd = ring.half_width() * 2.0;
    glow.ground_strip(&fx.road, wd, 0.22, scale(tint, 0.35 * k));
    glow.ground_strip(&fx.road, 0.5, 0.23, scale(tint, k));
    // Chevrons round the ring show the way traffic turns.
    let n = fx.road.len().saturating_sub(1).max(1);
    for (i, w) in fx.road.windows(2).enumerate() {
        if i % 2 == 1 && n > 4 {
            continue;
        }
        let mid = (w[0] + w[1]) * 0.5;
        let d = (w[1] - w[0]).normalize_or_zero();
        let side = SVec2::new(-d.y, d.x);
        let tip = mid + d * 1.6;
        for s in [-1.0f32, 1.0] {
            glow.ground_strip(&[mid - d * 0.6 + side * s * 1.4, tip], 0.45, 0.235, scale(tint, 1.2 * k));
        }
    }
    // The middle stays open land: only its inner kerb is outlined.
    let mut kerb: Vec<SVec2> = (0..=64).map(|i| ring.center + SVec2::from_angle(i as f32 / 64.0 * std::f32::consts::TAU) * ring.inner_radius()).collect();
    kerb.dedup();
    glow.ground_strip(&kerb, 0.3, 0.21, scale(tint, 0.6 * k));
    for (i, cut) in fx.cut.iter().enumerate() {
        let wd = fx.cut_widths.get(i).copied().unwrap_or(6.0);
        glow.ground_strip(cut, wd, 0.225, scale(pal.preview_remove, 0.18 * k));
        for w in cut.windows(2) {
            dashes(glow, w[0], w[1], 0.6, 0.24, 4.0, 3.0, scale(pal.preview_remove, k));
        }
    }
    for &j in &fx.joins {
        let dot: Vec<SVec2> = (0..12).map(|i| j + SVec2::from_angle(i as f32 / 12.0 * std::f32::consts::TAU) * 1.6).collect();
        glow.ground_polygon(&dot, 0.24, scale(tint, 1.4 * k));
    }
}

/// The Road tool's roundabout under the pointer, as Cities: Skylines
/// shows a road before it is built: the ring, where existing roads are cut
/// back and joined, and the lots whose buildings would be taken (red).
/// Rebuilt only when the ring or the plan changes.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_ring_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    tools: Res<crate::tools::ToolState>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut last: Local<Option<(i64, i64, i64, u8, u64, u64)>>,
    existing: Query<Entity, With<RingPreviewTag>>,
) {
    let w = &sim.world;
    let ring = tools.ring_preview();
    let key = ring.map(|r| ((r.center.x * 4.0).round() as i64, (r.center.y * 4.0).round() as i64, (r.radius * 4.0).round() as i64, r.lanes, w.plan_version, w.lots.len() as u64 ^ w.roads.version << 20));
    if key == *last && !assets.palette_changed {
        return;
    }
    *last = key;
    despawn_all(&mut commands, &existing);
    let Some(ring) = ring else { return };
    let pal = &assets.palette;
    let mut glow = MeshData::default();
    let fx = nd_sim::plan::ring_effect(w.effective_plan(), ring);
    draw_ring_effect(&mut glow, pal, &fx, 1.0);
    if !fx.blocked {
        for lot in w.roundabout_takes(ring) {
            let Some(l) = w.lots.get(lot) else { continue };
            let mut outline = l.polygon.clone();
            if let Some(&first) = l.polygon.first() { outline.push(first); }
            glow.ground_polygon(&l.polygon, 0.2, scale(pal.preview_remove, 0.3));
            glow.ground_strip(&outline, 0.7, 0.24, scale(pal.preview_remove, 1.3));
        }
    }
    spawn_glow(&mut commands, &mut meshes, &mats, &glow, RingPreviewTag);
}

pub(super) fn preview_width(kind: nd_sim::roads::RoadKind, fwd: u8, bwd: u8) -> f32 {
    match kind {
        nd_sim::roads::RoadKind::Path => nd_sim::roads::PATH_HALF_WIDTH * 2.0,
        nd_sim::roads::RoadKind::Rail => nd_sim::roads::RAIL_HALF_WIDTH * 2.0,
        nd_sim::roads::RoadKind::Street => (fwd + bwd) as f32 * LANE_WIDTH + SHOULDER * 2.0,
    }
}
