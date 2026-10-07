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
    let steep = w.steep_legs(pts, levels, kind);
    let plan = w.road_pillar_plan(pts, levels, f, b, kind);
    let raised = nd_sim::pillars::is_raised(levels);
    let pv = road_preview_with(w, pts, levels, f, b, kind, pal, &steep, pal.preview, WARN_TINT, alpha, raised.then_some(&plan));
    let mut red = MeshData::default();
    for lot in w.road_takes(pts, levels, f, b, kind) {
        let Some(l) = w.lots.get(lot) else { continue };
        let y = w.lot_pad(lot) + 0.35;
        let mut outline = l.polygon.clone();
        outline.push(l.polygon[0]);
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
        outline.push(l.polygon[0]);
        red.ground_strip(&outline, 2.4, y, [WARN_TINT[0] * 2.0, WARN_TINT[1] * 1.5, WARN_TINT[2] * 1.5, 1.0]);
    }
    // [tools] Buildings a column would buy out, and where the column goes.
    for (lot, _) in &plan.buyouts {
        let Some(l) = w.lots.get(*lot) else { continue };
        let y = w.lot_pad(*lot) + 0.4;
        let mut outline = l.polygon.clone();
        outline.push(l.polygon[0]);
        red.ground_polygon(&l.polygon, y, [PILLAR_TINT[0] * 0.3, PILLAR_TINT[1] * 0.3, PILLAR_TINT[2] * 0.3, 1.0]);
        red.ground_strip(&outline, 1.6, y + 0.02, [PILLAR_TINT[0], PILLAR_TINT[1], PILLAR_TINT[2], 1.0]);
    }
    for p in &plan.pillars {
        let on_building = plan.buyouts.iter().any(|(lot, _)| w.lots.get(*lot).is_some_and(|l| l.contains(p.at) || nd_sim::geom::distance_to_outline(p.at, &l.polygon) < nd_sim::pillars::PILLAR_RADIUS));
        let c = if on_building { PILLAR_TINT } else { [0.35, 0.9, 1.3] };
        let ring: Vec<SVec2> = (0..=16).map(|k| p.at + SVec2::from_angle(k as f32 / 16.0 * std::f32::consts::TAU) * nd_sim::pillars::PILLAR_RADIUS).collect();
        red.ground_strip(&ring, if on_building { 0.6 } else { 0.3 }, w.ground_height(p.at) + 0.5, [c[0], c[1], c[2], 1.0]);
    }
    // No room: a bright red band along the stretch on the ground, and the
    // deck over it red.
    let deck_w = half_width(kind, f, b) * 2.0;
    let level_at = |p: SVec2| {
        let mut best = (f32::MAX, 0.0);
        for (i, s) in pts.windows(2).enumerate() {
            let (t, q) = nd_sim::roads::project_on_segment(p, s[0], s[1]);
            let (la, lb) = (levels.get(i).copied().unwrap_or(0.0), levels.get(i + 1).copied().unwrap_or(0.0));
            if q.distance(p) < best.0 {
                best = (q.distance(p), la + (lb - la) * t);
            }
        }
        best.1
    };
    for &(ga, gb) in &plan.gaps {
        red.ground_strip(&[ga, gb], 3.0, w.ground_height(ga.lerp(gb, 0.5)) + 0.6, [WARN_TINT[0] * 2.0, WARN_TINT[1], WARN_TINT[2], 1.0]);
        let n = ((ga.distance(gb) / 4.0).ceil() as usize).max(1);
        for k in 0..n {
            let (p, q) = (ga.lerp(gb, k as f32 / n as f32), ga.lerp(gb, (k + 1) as f32 / n as f32));
            let m = p.lerp(q, 0.5);
            red.ground_strip(&[p, q], deck_w, w.ground_height(m) + level_at(m) + 0.25, [WARN_TINT[0] * 1.6, WARN_TINT[1] * 0.8, WARN_TINT[2] * 0.8, 1.0]);
        }
    }
    (pv.mesh, red, plan)
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
) {
    let w = &sim.world;
    let key = Some((w.plan_version, w.roads.version, overlays.projects));
    if key == *last && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = key;
    despawn_all(&mut commands, &existing);
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
        ),
    >,
    mut tunnels: Query<&mut Visibility, (Or<(With<TunnelTag>, With<GhostRoadTag>)>, Without<CarTag>)>,
    mut cars: Query<(Entity, &Transform, &mut Visibility, Has<DepthHidden>), (With<CarTag>, Without<TunnelTag>)>,
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
    for (e, tf, mut v, marked) in &mut cars {
        apply(e, &mut v, marked, !under || tf.translation.y < UNDERGROUND_BELOW, true);
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
