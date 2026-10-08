//! Planned roads as holograms of the real thing, and the depth view.
//!
//! The Road tool's road under construction and every pending project's
//! roads are drawn with the city's own street generator
//! (`nd_procgen::preview::road_preview`), at their planned heights, with
//! viaduct decks, tunnel bores and junction pads, translucent and tinted;
//! legs too steep for the road are red, and so are the lots with buildings
//! the road would take. Regenerated only when what is drawn changes.
//!
//! The depth view (`ToolState::depth`) hides the surface in Underground:
//! terrain, water, buildings, lots, ground roads and walkers go, ground
//! roads stay as faint outlines, and the tunnels (drawn apart from the
//! surface roads) and their traffic show.

use super::*;
use crate::tools::{Depth, Tool, ToolState};
use nd_procgen::preview::road_preview_with;
use nd_sim::roads::RoadKind;

/// The tunnel layer of the streets (only shown underground).
#[derive(Component, Clone)]
pub struct TunnelTag;

/// The surface streets drawn see-through, shown only in the underground
/// view so the tunnels can be read against the streets above them.
#[derive(Component)]
pub struct GhostRoadTag;

/// The Road tool's road being drawn.
#[derive(Component, Clone)]
pub struct DrawHoloTag;

/// Pending projects' roads.
#[derive(Component, Clone)]
pub struct PlanHoloTag;

const WARN_TINT: [f32; 3] = [1.6, 0.25, 0.2];
/// [tools] What a pillar would buy out: orange, apart from the red of what
/// the road itself takes.
const PILLAR_TINT: [f32; 3] = [1.7, 0.75, 0.1];

/// The road the Road tool would build now: its points, then the pointer
/// (snapped, at the level the next click would place).
fn drawn_road(tools: &ToolState, sim: &Sim, cursor: Option<SVec2>) -> Option<(Vec<SVec2>, Vec<f32>)> {
    if tools.tool != Tool::Road || tools.roundabout {
        return None;
    }
    let mut pts = tools.points.clone();
    let mut levels = tools.levels();
    if let Some(c) = tools.cursor_pin.or(cursor) {
        let (c, _, l) = ToolState::road_snap_level(sim, c, tools.kind, tools.elevation, pts.is_empty());
        if pts.last().is_none_or(|p| p.distance(c) > 0.5) {
            pts.push(c);
            levels.push(l);
        }
    }
    (pts.len() >= 2).then_some((pts, levels))
}

/// The built polyline of control points and levels.
fn built(pts: &[SVec2], levels: &[f32], curved: bool) -> (Vec<SVec2>, Vec<f32>) {
    if curved && pts.len() >= 3 {
        (nd_sim::plan::sample_curve(pts), nd_sim::plan::sample_curve_levels(pts, levels))
    } else {
        (pts.to_vec(), levels.to_vec())
    }
}

fn half_width(kind: RoadKind, f: u8, b: u8) -> f32 {
    match kind {
        RoadKind::Street => (f.max(1) + b) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + nd_sim::roads::SHOULDER + nd_sim::zoning::SIDEWALK,
        RoadKind::Path => nd_sim::roads::PATH_HALF_WIDTH + 1.0,
        RoadKind::Rail => nd_sim::roads::RAIL_HALF_WIDTH + 1.0,
    }
}

/// A road's hologram and the red outlines of what it takes. A raised road
/// shows its columns where they would stand (`World::road_pillar_plan`):
/// the deck red over a stretch with no room for one, the buildings a
/// column would buy out outlined in orange with a marker where it stands.
#[allow(clippy::too_many_arguments)]
fn hologram(w: &World, pal: &nd_procgen::Palette, pts: &[SVec2], levels: &[f32], f: u8, b: u8, kind: RoadKind, alpha: f32) -> (MeshData, MeshData, nd_sim::pillars::PillarPlan) {
    // [interchange] A ramp as it will be built (`roads::ease_ramp_way`): the
    // hologram used to show the line as drawn while the build ran it
    // alongside the highway first.
    let (pts, levels) = nd_sim::roads::ease_ramp_way(pts, levels, f, b, kind, &w.roads);
    let (pts, levels) = (&pts[..], &levels[..]);
    let steep = w.steep_legs(pts, levels, kind);
    let plan = w.road_pillar_plan(pts, levels, f, b, kind);
    let raised = nd_sim::pillars::is_raised(levels);
    let pv = road_preview_with(w, pts, levels, f, b, kind, pal, &steep, pal.preview, WARN_TINT, alpha, raised.then_some(&plan));
    let mut red = MeshData::default();
    for lot in w.road_takes(pts, levels, f, b, kind) {
        let Some(l) = w.lots.get(lot) else { continue };
        let y = w.lot_pad(lot) + 0.35;
        let mut outline = l.polygon.clone();
        if let Some(&first) = l.polygon.first() { outline.push(first); }
        red.ground_polygon(&l.polygon, y, [WARN_TINT[0] * 0.25, WARN_TINT[1] * 0.25, WARN_TINT[2] * 0.25, 1.0]);
        red.ground_strip(&outline, 0.7, y + 0.02, [WARN_TINT[0], WARN_TINT[1], WARN_TINT[2], 1.0]);
    }
    // The city's own structures in the way: implementing is refused until
    // the road is moved (`World::try_implement`), so they get a wide,
    // bright border that cannot be mistaken for an ordinary buy-out.
    for bid in w.structures_in_way(pts, levels, f, b, kind) {
        let Some(lot) = w.buildings.get(bid).map(|b| b.lot) else { continue };
        let Some(l) = w.lots.get(lot) else { continue };
        let y = w.lot_pad(lot) + 0.45;
        let mut outline = l.polygon.clone();
        if let Some(&first) = l.polygon.first() { outline.push(first); }
        red.ground_strip(&outline, 2.4, y, [WARN_TINT[0] * 2.0, WARN_TINT[1] * 1.5, WARN_TINT[2] * 1.5, 1.0]);
    }
    // [tools] Buildings a column would buy out, and where the column goes.
    for (lot, _) in &plan.buyouts {
        let Some(l) = w.lots.get(*lot) else { continue };
        let y = w.lot_pad(*lot) + 0.4;
        let mut outline = l.polygon.clone();
        if let Some(&first) = l.polygon.first() { outline.push(first); }
        red.ground_polygon(&l.polygon, y, [PILLAR_TINT[0] * 0.3, PILLAR_TINT[1] * 0.3, PILLAR_TINT[2] * 0.3, 1.0]);
        red.ground_strip(&outline, 1.6, y + 0.02, [PILLAR_TINT[0], PILLAR_TINT[1], PILLAR_TINT[2], 1.0]);
    }
    for p in &plan.pillars {
        let on_building = plan.buyouts.iter().any(|(lot, _)| w.lots.get(*lot).is_some_and(|l| l.contains(p.at) || nd_sim::geom::distance_to_outline(p.at, &l.polygon) < nd_sim::pillars::PILLAR_RADIUS));
        let c = if on_building { PILLAR_TINT } else { [0.35, 0.9, 1.3] };
        let ring: Vec<SVec2> = (0..=16).map(|k| p.at + SVec2::from_angle(k as f32 / 16.0 * std::f32::consts::TAU) * nd_sim::pillars::PILLAR_RADIUS).collect();
        red.ground_strip(&ring, if on_building { 0.6 } else { 0.3 }, w.ground_height(p.at) + 0.5, [c[0], c[1], c[2], 1.0]);
    }
    // No room for a column: the stretch red, with a pin at the spot.
    let deck_w = half_width(kind, f, b) * 2.0;
    for g in &plan.gaps {
        gap_marks(w, g, deck_w, &mut red);
    }
    (pv.mesh, red, plan)
}

/// [tools] Where a stretch with no room for a column is: a red band on the
/// ground, the deck over it red, and a tall red pin at the spot (the end
/// left in the air, or the middle of the gap) so it can be found on the map.
fn gap_marks(w: &World, g: &nd_sim::pillars::Gap, deck_w: f32, red: &mut MeshData) {
    let (ga, gb) = (g.a, g.b);
    red.ground_strip(&[ga, gb], 3.0, w.ground_height(ga.lerp(gb, 0.5)) + 0.6, [WARN_TINT[0] * 2.0, WARN_TINT[1], WARN_TINT[2], 1.0]);
    let n = ((ga.distance(gb) / 4.0).ceil() as usize).max(1);
    for k in 0..n {
        let (p, q) = (ga.lerp(gb, k as f32 / n as f32), ga.lerp(gb, (k + 1) as f32 / n as f32));
        let m = p.lerp(q, 0.5);
        red.ground_strip(&[p, q], deck_w, w.ground_height(m) + g.level + 0.25, [WARN_TINT[0] * 1.6, WARN_TINT[1] * 0.8, WARN_TINT[2] * 0.8, 1.0]);
    }
    let spot = gap_spot(g);
    let z = w.ground_height(spot);
    let pin = [WARN_TINT[0] * 2.4, WARN_TINT[1] * 1.6, WARN_TINT[2] * 1.6, 1.0];
    red.boxed(glam::Vec3::new(spot.x, z + (g.level + 14.0) * 0.5, spot.y), glam::Vec3::new(0.6, g.level + 14.0, 0.6), 0.0, pin);
    red.boxed(glam::Vec3::new(spot.x, z + g.level + 15.0, spot.y), glam::Vec3::new(2.4, 2.4, 2.4), 0.785, pin);
    let ring: Vec<SVec2> = (0..=24).map(|k| spot + SVec2::from_angle(k as f32 / 24.0 * std::f32::consts::TAU) * 6.0).collect();
    red.ground_strip(&ring, 0.8, z + 0.7, pin);
}

/// [tools] A city structure the active project would take: its lot
/// outlined in a wide red border (also for one the rebuild loses, which no
/// per-road hologram shows), the stretch of the road that takes it red on
/// the ground, and a tall red pin on the structure so it can be found.
fn blocker_marks(w: &World, k: &nd_sim::Blocker, red: &mut MeshData) {
    let bright = [WARN_TINT[0] * 2.0, WARN_TINT[1] * 1.5, WARN_TINT[2] * 1.5, 1.0];
    if let Some(lot) = w.buildings.get(k.building).map(|b| b.lot) {
        if let Some(l) = w.lots.get(lot) {
            let y = w.lot_pad(lot) + 0.45;
            let mut outline = l.polygon.clone();
            if let Some(&first) = l.polygon.first() { outline.push(first); }
            red.ground_polygon(&l.polygon, y - 0.05, [WARN_TINT[0] * 0.3, WARN_TINT[1] * 0.3, WARN_TINT[2] * 0.3, 1.0]);
            red.ground_strip(&outline, 2.4, y, bright);
        }
    }
    // The road's stretch over it (or its nearest point): a red band.
    let (a, b) = if k.a.distance(k.b) < 2.0 {
        let d = (k.at - k.a).normalize_or_zero().perp() * 4.0;
        (k.a - d, k.a + d)
    } else {
        (k.a, k.b)
    };
    red.ground_strip(&[a, b], 4.0, w.ground_height(a.lerp(b, 0.5)) + 0.7, [WARN_TINT[0] * 2.4, WARN_TINT[1] * 1.2, WARN_TINT[2] * 1.2, 1.0]);
    let spot = k.at;
    let z = w.ground_height(spot);
    let pin = [WARN_TINT[0] * 2.4, WARN_TINT[1] * 1.6, WARN_TINT[2] * 1.6, 1.0];
    red.boxed(glam::Vec3::new(spot.x, z + 12.0, spot.y), glam::Vec3::new(0.6, 24.0, 0.6), 0.0, pin);
    red.boxed(glam::Vec3::new(spot.x, z + 25.0, spot.y), glam::Vec3::new(2.4, 2.4, 2.4), 0.785, pin);
}

/// [tools] A label on the map over each red pin: what is wrong there, for
/// the road being drawn and for the active project's pending roads.
pub fn gap_labels(mut contexts: bevy_egui::EguiContexts, sim: Res<Sim>, tools: Res<ToolState>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    use bevy_egui::egui;
    let mut gaps: Vec<nd_sim::pillars::Gap> = tools.pillars.gaps.clone();
    let mut texts: Vec<Option<String>> = vec![None; gaps.len()];
    // [interchange] Why the diamond under the pointer would be refused.
    if let Some((why, at)) = &tools.interchange_refusal {
        gaps.push(nd_sim::pillars::Gap { a: *at, b: *at, level: 0.0, end: true, over_road: false });
        texts.push(Some(format!("No interchange: {why}")));
    }
    if let Some(c) = &tools.plan_check {
        for g in &c.pillar_gaps {
            if !gaps.iter().any(|o| o.a.distance(g.a) < 3.0 && o.b.distance(g.b) < 3.0) {
                gaps.push(*g);
            }
        }
    }
    let blocks: Vec<nd_sim::Blocker> = tools.plan_check.as_ref().map(|c| c.blocks.clone()).unwrap_or_default();
    if gaps.is_empty() && blocks.is_empty() {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let w = &sim.world;
    let ctx = contexts.ctx_mut()?;
    // [city-data] The map is covered: nothing to label.
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("pillar_gap_labels")));
    let labels = gaps
        .iter()
        .enumerate()
        .map(|(k, g)| {
            let spot = gap_spot(g);
            let text = if let Some(Some(t)) = texts.get(k) {
                t.clone()
            } else if g.end {
                format!("Ends in the air: no room for a column within {:.0} m", nd_sim::pillars::PILLAR_END_REACH)
            } else {
                format!("No room for a column for {:.0} m (a span may be {:.0})", g.a.distance(g.b), nd_sim::pillars::PILLAR_MAX_SPAN)
            };
            (spot, w.ground_height(spot) + g.level + 17.0, text)
        })
        .chain(blocks.iter().map(|k| {
            let text = if k.runs_over { format!("The planned road would take {}", k.name) } else { format!("{} would lose its lot", k.name) };
            (k.at, w.ground_height(k.at) + 27.0, text)
        }));
    for (spot, top, text) in labels {
        let Ok(v) = camera.world_to_viewport(gt, Vec3::new(spot.x, top, spot.y)) else { continue };
        let v = crate::ui::theme::to_points(ctx, v);
        let galley = painter.layout_no_wrap(text, crate::ui::theme::small(), egui::Color32::WHITE);
        let at = egui::pos2(v.x - galley.size().x * 0.5, v.y - galley.size().y - 6.0);
        let r = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(6.0, 3.0));
        painter.rect_filled(r, 4.0, egui::Color32::from_rgba_unmultiplied(150, 20, 20, 220));
        painter.galley(at, galley, egui::Color32::WHITE);
    }
    Ok(())
}

/// The spot a gap is shown at: the end left in the air, else its middle.
pub fn gap_spot(g: &nd_sim::pillars::Gap) -> SVec2 {
    if g.end { g.b } else { g.a.lerp(g.b, 0.5) }
}

/// The road the Road tool is drawing, as it will be built.
#[allow(clippy::too_many_arguments)]
pub fn sync_draw_hologram(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mut tools: ResMut<ToolState>,
    cursor: Res<crate::camera::Cursor>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut last: Local<Option<u64>>,
    existing: Query<Entity, With<DrawHoloTag>>,
) {
    let w = &sim.world;
    let drawn = drawn_road(&tools, &sim, cursor.ground);
    let key = drawn.as_ref().map(|(pts, levels)| {
        let mut h = DefaultHasher::new();
        // The pointer moves in quarter metres: no regeneration for jitter.
        for p in pts {
            ((p.x * 4.0).round() as i64, (p.y * 4.0).round() as i64).hash(&mut h);
        }
        for l in levels {
            l.to_bits().hash(&mut h);
        }
        (tools.lanes_fwd, tools.lanes_bwd, format!("{:?}", tools.kind), tools.curved, w.plan_version, w.roads.version).hash(&mut h);
        h.finish()
    });
    if key == *last && !assets.palette_changed {
        return;
    }
    *last = key;
    despawn_all(&mut commands, &existing);
    let Some((ctrl, ctrl_levels)) = drawn else {
        tools.grade_warnings.clear();
        tools.pillars = Default::default();
        return;
    };
    let kind = tools.kind;
    let (f, b) = if kind == RoadKind::Street { (tools.lanes_fwd, tools.lanes_bwd) } else { (0, 0) };
    // [interchange] The Interchange preset: the whole diamond as it would
    // be laid, or the line red with a pin where it would be refused.
    tools.interchange_refusal = None;
    if tools.interchange {
        tools.grade_warnings.clear();
        tools.pillars = Default::default();
        let (a, z) = (ctrl[0], ctrl[ctrl.len() - 1]);
        let mut mesh = MeshData::default();
        let mut red = MeshData::default();
        match w.plan_diamond(a, z, (f.max(1), b.max(1))) {
            Ok(plan) => {
                for g in &plan.gestures {
                    if let nd_sim::Gesture::Road { points, lanes_fwd, lanes_bwd, kind, .. } = g {
                        let (m, r, _) = hologram(w, &assets.palette, points, &vec![0.0; points.len()], *lanes_fwd, *lanes_bwd, *kind, 0.55);
                        mesh.append(&m);
                        red.append(&r);
                    }
                }
            }
            Err(r) => {
                let (m, _, _) = hologram(w, &assets.palette, &[a, z], &[0.0, 0.0], f.max(1), b.max(1), kind, 0.35);
                mesh.append(&m);
                let line = nd_sim::pillars::Gap { a: r.at, b: r.at, level: 0.0, end: true, over_road: false };
                gap_marks(w, &line, 6.0, &mut red);
                tools.interchange_refusal = Some((r.why, r.at));
            }
        }
        if !mesh.is_empty() {
            spawn_overlay(&mut commands, &mut meshes, &mats, &mesh, DrawHoloTag);
        }
        if !red.is_empty() {
            spawn_glow(&mut commands, &mut meshes, &mats, &red, DrawHoloTag);
        }
        return;
    }
    tools.grade_warnings = w.road_grade_warnings(&ctrl, &ctrl_levels, kind, f, b);
    let (pts, levels) = built(&ctrl, &ctrl_levels, tools.curved);
    let (mesh, red, plan) = hologram(w, &assets.palette, &pts, &levels, f, b, kind, 0.55);
    // [tools] The columns, for the sub-pane and the remote API.
    tools.pillars = crate::tools::PillarNote::of(w, &plan);
    if !mesh.is_empty() {
        spawn_overlay(&mut commands, &mut meshes, &mats, &mesh, DrawHoloTag);
    }
    if !red.is_empty() {
        spawn_glow(&mut commands, &mut meshes, &mats, &red, DrawHoloTag);
    }
}

/// Every pending project's roads as holograms: the active one bright, the
/// parked ones dim.
#[allow(clippy::too_many_arguments)]
pub fn sync_plan_holograms(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    mut last: Local<Option<(u64, u64, bool)>>,
    existing: Query<Entity, With<PlanHoloTag>>,
    mut tools: ResMut<ToolState>,
) {
    let w = &sim.world;
    let key = Some((w.plan_version, w.roads.version, overlays.projects));
    if key == *last && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = key;
    despawn_all(&mut commands, &existing);
    // [tools] The active project checked as implementing will check it
    // (the trial rebuild included), so what would be refused shows before
    // the player implements: red on the map, the reason in the toolbar.
    let check = w.active_project_check();
    let mut all = MeshData::default();
    let mut red = MeshData::default();
    let mut add = |pv: &nd_sim::PlanPreview, alpha: f32, red_too: bool| {
        for (i, (_, pts, f, b, kind)) in pv.planned_roads.iter().enumerate() {
            let levels = pv.planned_levels.get(i).cloned().unwrap_or_else(|| vec![0.0; pts.len()]);
            let (m, r, _) = hologram(w, &assets.palette, pts, &levels, *f, *b, *kind, alpha);
            all.append(&m);
            if red_too {
                red.append(&r);
            }
        }
    };
    add(&w.preview(), 0.5, true);
    if overlays.projects {
        for other in w.inactive_previews() {
            add(&other.preview, 0.22, false);
        }
    }
    // Gaps the per-road previews did not show (found on the trial rebuild).
    for g in &check.pillar_gaps {
        gap_marks(w, g, 12.0, &mut red);
    }
    // [tools] Every structure implementing would refuse on, as the check
    // found it: the one the refusal names is always marked.
    for k in &check.blocks {
        blocker_marks(w, k, &mut red);
    }
    tools.plan_check = check.refusal.is_some().then_some(check);
    if !all.is_empty() {
        spawn_overlay(&mut commands, &mut meshes, &mats, &all, PlanHoloTag);
    }
    if !red.is_empty() {
        spawn_glow(&mut commands, &mut meshes, &mats, &red, PlanHoloTag);
    }
}

/// Marks an entity the depth view hid, so leaving Underground shows again
/// only what the depth view itself hid.
#[derive(Component)]
pub struct DepthHidden;

/// Show the layer the view is set to. Underground hides the surface
/// (terrain, water, buildings, lots, ground roads, walkers, cars above the
/// ground) and draws the ground roads as faint outlines over the tunnels.
///
/// Many surface entities have their own owner deciding whether they show:
/// a citizen indoors is hidden by `sync_citizens` (its entity keeps the
/// plain stand-in figure at the last place it was seen), the generic car
/// under an emergency unit by `sync_emergency`, a faded smog puff by
/// `sync_haze`. This system runs after all of them, so it must never
/// *show* anything it did not hide itself: it used to set every surface
/// entity to `Inherited` each frame above ground, which brought every
/// citizen indoors back as a frozen, white, unclickable old-style figure.
/// So it hides (and marks) what it must underground, and above ground
/// only restores what it marked. The waterworks (decks, viaducts, tunnel
/// portals and vent heads) are surface works too: a vent head used to
/// float in the middle of the bore in the underground view.
#[allow(clippy::type_complexity)]
pub fn apply_depth(
    mut commands: Commands,
    tools: Res<ToolState>,
    sim: Res<Sim>,
    mut gizmos: Gizmos,
    mut surface: Query<
        (Entity, &mut Visibility, Has<DepthHidden>, Has<CitizenTag>),
        (
            Or<(
                Or<(With<TerrainTag>, With<BuildingTag>, With<LotsTag>, With<ScatterTag>, With<ParkTag>, With<VacantTag>, With<RoadsTag>, With<WaterTag>, With<WaterworksTag>, With<LaneArrowsTag>, With<PathsTag>, With<WalkDetailTag>, With<SignalTag>, With<CitizenTag>)>,
                // Kerbside life: parked cars, trash and camps, stops, tolls, pavement bots.
                Or<(With<ParkedTag>, With<super::society::SocietyTag>, With<TransitTag>, With<TollTag>, With<BotTag>)>,
            )>,
            Without<TunnelTag>,
            Without<GhostRoadTag>,
            Without<CarTag>,
            Without<super::focus::UnitTag>,
        ),
    >,
    mut tunnels: Query<&mut Visibility, (Or<(With<TunnelTag>, With<GhostRoadTag>)>, Without<CarTag>, Without<super::focus::UnitTag>)>,
    // [fix/review-app] Emergency units (body and light bar) go by their lane like any car.
    mut cars: Query<(Entity, &Transform, &mut Visibility, Has<DepthHidden>, Option<&InTunnel>), (Or<(With<CarTag>, With<super::focus::UnitTag>)>, Without<TunnelTag>, Without<GhostRoadTag>)>,
) {
    let under = tools.depth == Depth::Underground;
    // Hide (and mark) or restore (only if marked); `restore` false leaves
    // the entity to its owner, which sets it every frame anyway.
    let mut apply = |e: Entity, v: &mut Visibility, marked: bool, on: bool, restore: bool| {
        if !on {
            if *v != Visibility::Hidden {
                *v = Visibility::Hidden;
                if !marked {
                    commands.entity(e).try_insert(DepthHidden);
                }
            }
        } else if marked {
            if restore {
                *v = Visibility::Inherited;
            }
            commands.entity(e).try_remove::<DepthHidden>();
        }
    };
    for (e, mut v, marked, citizen) in &mut surface {
        // Citizens: `sync_citizens` sets every one, every frame.
        apply(e, &mut v, marked, !under, !citizen);
    }
    for mut v in &mut tunnels {
        let want = if under { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    for (e, tf, mut v, marked, tunnel) in &mut cars {
        // A car on the road is in the tunnel layer by the lane it is on,
        // not by its height (a tunnel near its portal runs barely below
        // the graded ground); anything else by its height.
        let on = match tunnel {
            Some(t) if under => t.0 || tf.translation.y < UNDERGROUND_BELOW,
            Some(t) => !t.0,
            None => !under || tf.translation.y < UNDERGROUND_BELOW,
        };
        apply(e, &mut v, marked, on, true);
    }
    if under {
        let w = &sim.world;
        // Centre lines over the see-through streets (`GhostRoadTag`).
        let faint = Color::linear_rgba(0.55, 0.7, 0.95, 0.35);
        let z = |n: nd_sim::NodeId| w.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
        for (sid, s) in w.roads.alive_segments() {
            if s.tunnel || s.is_rail() {
                continue;
            }
            let (a, b) = w.roads.segment_points(sid);
            gizmos.line(Vec3::new(a.x, z(s.a) + 0.3, a.y), Vec3::new(b.x, z(s.b) + 0.3, b.y), faint);
        }
    }
}
