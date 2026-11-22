//! [r12-yards] Yards and fronts that say who lives there, drawn near the
//! camera only.
//!
//! What a household puts out comes from the sim (`nd_sim::yards`: its
//! children, its old, its trades and leanings, the washing by the
//! weather); where it stands from the drawn building (`nd_procgen::yards::
//! spots`, worked out in the chunk job that draws the home and kept in
//! `Sim::yard_spots`); what it looks like from a dozen small procedural
//! meshes built once (`nd_procgen::yards::prop_geometry`), every thing an
//! entity sharing its look's mesh and the building surface material, so
//! Bevy draws each look as one instanced batch.
//!
//! Only round the camera, in the shared close band (`nd_procgen::detail`,
//! [r13-detail]): the homes within `CLOSE_REACH` of the eye are given their
//! things, each fading out over `DetailBand::Close` (a `VisibilityRange`),
//! and an eye more than `CLOSE_EYE` over the ground has none at all: zoomed
//! out it costs nothing. The washing on building styles' own lines and
//! poles is drawn here too (`nd_procgen::yards::HUNG`): the styles draw
//! only the lines. Nothing a frame otherwise: the homes in reach
//! are looked at again twice a second, or when the eye has moved, and a
//! home's things are placed again only when its household changed
//! ([`nd_sim::yards::home_signature`]), its drawing did, or the washing
//! went in or out. `ND_YARDS=0` draws none.

use super::*;
use nd_procgen::yards::{self as py, Placed, Spots};
use nd_sim::yards::{self as sy, PropKind};

/// Homes this near the eye (along the ground) get their things.
pub const REACH: f32 = nd_procgen::detail::CLOSE_REACH;
/// A home drawn stays drawn until the eye is this much farther off.
const KEEP: f32 = 25.0;
/// An eye higher than this over the ground draws no yards at all.
pub const FAR_EYE: f32 = nd_procgen::detail::CLOSE_EYE;
/// Seconds between looks at the homes in reach while the eye stands still.
const REFRESH_SECS: f32 = 0.5;
/// The eye moving this far brings the next look forward, metres.
const MOVED: f32 = 6.0;
/// A pavement stands this much over the built ground (the kerb).
const KERB: f32 = 0.16;

/// `ND_YARDS=0` draws no yards (and works out no free ground for them).
pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("ND_YARDS").map_or(true, |v| v != "0"))
}

/// One household's thing on screen: the home, whose it is, what and
/// which look, its foot (world) and heading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YardProp {
    pub building: BuildingId,
    pub owner: nd_sim::CitizenId,
    pub kind: PropKind,
    pub variant: u8,
    pub at: Vec3,
    pub yaw: f32,
}

/// The free ground and windows round a home drawn as `g` (before it is
/// raised onto its pad), lifted onto the pad: in the chunk job, with the
/// building. None for anything that is not a standing home.
pub(super) fn home_spots(world: &World, id: BuildingId, g: &Geometry) -> Option<Spots> {
    if !on() {
        return None;
    }
    let b = world.buildings.get(id)?;
    if b.homes == 0 || b.construction.is_some() || b.demolition.is_some() || b.is_tenant() {
        return None;
    }
    let lot = world.footprint(id)?;
    let mut s = py::spots(g, &lot, sy::is_block(b.homes, b.floors), b.seed);
    if s.is_empty() {
        return None;
    }
    s.raise(world.lot_pad(b.lot), |p| world.surface_height(p) + KERB);
    Some(s)
}

#[derive(Component)]
pub(super) struct YardTag;

/// What one home's things were placed for, and their entities.
struct Home {
    /// Its household signature, its spots' version and the washing.
    key: (u64, u64, bool),
    entities: Vec<Entity>,
    props: Vec<YardProp>,
}

#[derive(Resource, Default)]
pub(super) struct YardRender {
    /// Each look's meshes, by surface, built the first time it is needed.
    looks: HashMap<(PropKind, u8), Vec<(Surface, Handle<Mesh>)>>,
    homes: HashMap<BuildingId, Home>,
    /// The eye, the time and the washing at the last look.
    eye: Option<Vec3>,
    last: f32,
    laundry: Option<bool>,
    spots_seen: u64,
    generation: u64,
    /// The public beaches, at the lots' version.
    beaches: Option<(u64, Vec<SVec2>)>,
}

impl YardRender {
    fn clear(&mut self, commands: &mut Commands) {
        for (_, h) in self.homes.drain() {
            for e in h.entities {
                commands.entity(e).despawn();
            }
        }
        self.eye = None;
    }
}

/// Whether a thing is too small or flat to bother the shadow passes with.
fn shadowless(kind: PropKind) -> bool {
    matches!(kind, PropKind::Chalk | PropKind::Ball | PropKind::Toys | PropKind::Plant)
}

/// Place one home's things: the household's wishes stood on the home's
/// spots.
fn place_home(w: &World, id: BuildingId, spots: &Spots, laundry: bool, beaches: &[SVec2]) -> Vec<YardProp> {
    let ctx = sy::home_ctx(w, id, beaches);
    let wishes = sy::home_wishes(w, id, laundry, ctx);
    let seed = w.buildings.get(id).map_or(0, |b| b.seed);
    py::place(&wishes, spots, seed).into_iter().map(|p: Placed| YardProp { building: id, owner: p.owner, kind: p.kind, variant: p.variant, at: p.at, yaw: p.yaw }).collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_yards(
    mut commands: Commands,
    mut sim: ResMut<Sim>,
    mats: Res<Materials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut yr: ResMut<YardRender>,
    time: Res<Time>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut stats: ResMut<RenderStats>,
) {
    let _t = crate::frame_stats::span("sync_yards");
    if sim.full_rebuild || sim.generation != yr.generation {
        yr.clear(&mut commands);
        yr.generation = sim.generation;
        sim.yard_props.clear();
    }
    if !on() {
        return;
    }
    let Some(eye) = cams.iter().next().map(|t| t.translation()) else { return };
    let here = SVec2::new(eye.x, eye.z);
    if eye.y - sim.world.surface_height(here) > FAR_EYE {
        // Zoomed out: none at all.
        if !yr.homes.is_empty() {
            yr.clear(&mut commands);
            sim.yard_props.clear();
            stats.yard_props = 0;
        }
        return;
    }
    let w = &sim.world;
    let laundry = sy::laundry_out(&w.weather, w.clock.hour());
    let now = time.elapsed_secs();
    let moved = yr.eye.is_none_or(|e| e.distance(eye) > MOVED);
    if !moved && now - yr.last < REFRESH_SECS && yr.laundry == Some(laundry) && yr.spots_seen == sim.yard_version {
        return;
    }
    yr.eye = Some(eye);
    yr.last = now;
    yr.laundry = Some(laundry);
    yr.spots_seen = sim.yard_version;
    let lots_v = w.lots.version();
    if yr.beaches.as_ref().is_none_or(|(v, _)| *v != lots_v) {
        yr.beaches = Some((lots_v, sy::beaches(w)));
    }
    let beaches = yr.beaches.as_ref().map(|(_, b)| b.clone()).unwrap_or_default();
    // The homes in reach, nearest first (ids break ties).
    let mut near: Vec<(f32, BuildingId)> = Vec::new();
    let mut keep: HashSet<BuildingId> = HashSet::new();
    for (&id, _) in sim.yard_spots.iter() {
        let Some(c) = w.buildings.get(id).and_then(|b| w.lots.get(b.lot)).map(|l| l.center) else { continue };
        let d = c.distance(here);
        if d < REACH {
            near.push((d, id));
        }
        if d < REACH + KEEP {
            keep.insert(id);
        }
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut changed = false;
    // Homes gone out of reach (or gone) give up their things.
    let gone: Vec<BuildingId> = yr.homes.keys().copied().filter(|id| !keep.contains(id)).collect();
    for id in gone {
        if let Some(h) = yr.homes.remove(&id) {
            for e in h.entities {
                commands.entity(e).despawn();
            }
            changed = true;
        }
    }
    let fade = band_range(nd_procgen::DetailBand::Close, false);
    for (_, id) in near {
        let Some((ver, spots)) = sim.yard_spots.get(&id) else { continue };
        let key = (sy::home_signature(w, id), *ver, laundry);
        if yr.homes.get(&id).is_some_and(|h| h.key == key) {
            continue;
        }
        changed = true;
        let props = place_home(w, id, spots, laundry, &beaches);
        let mut entities = yr.homes.remove(&id).map(|h| h.entities).unwrap_or_default();
        for e in entities.drain(..) {
            commands.entity(e).despawn();
        }
        for p in &props {
            let look = yr.looks.entry((p.kind, p.variant)).or_insert_with(|| py::prop_geometry(p.kind, p.variant).iter().map(|(s, m)| (s, meshes.add(shared_mesh(m, true)))).collect());
            let pose = Transform::from_translation(p.at).with_rotation(Quat::from_rotation_y(-p.yaw));
            for (s, mesh) in look.iter() {
                let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(*s)), pose, fade.clone(), YardTag));
                if shadowless(p.kind) {
                    e.insert(NotShadowCaster);
                }
                entities.push(e.id());
            }
        }
        yr.homes.insert(id, Home { key, entities, props });
    }
    if changed {
        let mut all: Vec<YardProp> = yr.homes.values().flat_map(|h| h.props.iter().copied()).collect();
        all.sort_by(|a, b| a.building.cmp(&b.building).then(a.at.x.total_cmp(&b.at.x)).then(a.at.z.total_cmp(&b.at.z)));
        stats.yard_props = all.len();
        stats.yard_homes = yr.homes.len();
        sim.yard_props = all;
    }
}

/// The household's thing on screen a ray from `o` along `d` meets first,
/// how far along, and whose it is.
pub(crate) fn pick_ray(sim: &Sim, o: Vec3, d: Vec3) -> Option<(f32, nd_sim::CitizenId)> {
    let props: Vec<(PropKind, u8, Vec3)> = sim.yard_props.iter().map(|p| (p.kind, p.variant, p.at)).collect();
    py::pick_ray(&props, o, d).map(|(i, t)| (t, sim.yard_props[i].owner))
}

/// The thing on screen standing at `p` (within its reach), how far, and
/// whose: a click with no ray (the remote API's `select {at}`).
pub(crate) fn pick_at(sim: &Sim, p: SVec2) -> Option<(f32, nd_sim::CitizenId)> {
    sim.yard_props
        .iter()
        .map(|y| (SVec2::new(y.at.x, y.at.z).distance(p), y))
        .filter(|(d, y)| *d <= py::reach(y.kind, y.variant).1)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(d, y)| (d, y.owner))
}
