//! [fire-scene] Emergency units as things to click, and what a fire engine's
//! crew does at a fire, worked out from the sim each frame.
//!
//! Nothing here is sim state: where the crew stands, how far the ladder is
//! up and where the water goes are derived from the scene (the engine where
//! it stopped, the burning building, the crew from the station's payroll)
//! and the time on scene, so the renderer, the picker, the inspector and the
//! remote API all see the same scene. Time is the sim's (ticks on scene plus
//! the frame's fraction, at `traffic_seconds_per_tick` seconds a tick), so a
//! paused game holds the scene still and a fast one plays it quickly.
//!
//! On arrival the crew climbs out and walks to their places: the pump
//! operator to the panel on the building side, the nozzle crew across to a
//! hose line in front of the building, and the ladder crew up the turntable.
//! The outriggers go down, the ladder raises, turns to the building and
//! extends towards the fire (aimed at the roof line or, on a tower, as high
//! up the facade as it reaches), and the ladder crew climbs it to spray
//! from the tip. When the fire is out (`Unit::packing`) it all comes back in
//! reverse within `PACK_UP_TICKS` and the engine drives off.

use bevy::prelude::Vec3;
use nd_procgen::people::FirePose;
use nd_procgen::BuildingRules;
use nd_sim::services::{IncidentKind, Outcome, UnitKind, UnitState};
use nd_sim::{BuildingId, CitizenId, UnitId, Vec2 as SVec2, World};

/// The roof ladder: its pivot on the turntable (truck frame: x forward, y
/// up, z to the left), one section's length, and the longest it reaches.
pub const LADDER_PIVOT: [f32; 3] = [-2.6, 3.2, 0.0];
pub const LADDER_SECTION: f32 = 7.0;
pub const LADDER_SECTIONS: usize = 4;
pub const LADDER_REACH: f32 = 26.0;
/// Steepest the ladder goes (radians).
pub const LADDER_MAX_PITCH: f32 = 1.3;
/// Litres a minute through a hand line and through the ladder's monitor.
pub const HOSE_FLOW: f32 = 950.0;
pub const LADDER_FLOW: f32 = 1900.0;
/// Walking pace of the crew on scene (m/s): a brisk walk, gear and all.
const PACE: f32 = 2.2;
/// Climbing pace on the ladder (m/s).
const CLIMB: f32 = 3.5;

/// What a crew member is there for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Pump,
    Nozzle,
    Ladder,
    /// A fourth or later hand: backs up the nozzle crew on the hose.
    Backup,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Pump => "pump operator",
            Role::Nozzle => "on the hose",
            Role::Ladder => "on the ladder",
            Role::Backup => "backing up the hose",
        }
    }
}

/// One firefighter as drawn this frame.
#[derive(Clone, Copy, Debug)]
pub struct Figure {
    pub citizen: Option<CitizenId>,
    pub role: Role,
    /// Feet, in render space (x, height, sim y).
    pub pos: Vec3,
    /// Facing, as a sim heading (radians from +x in the sim's plane).
    pub yaw: f32,
    pub pose: FirePose,
    pub walking: bool,
    /// Out of the cab (hidden while aboard).
    pub out: bool,
    pub skin: u8,
    pub stature: f32,
}

/// A stream of water: from a nozzle to where it lands, arcing up by `arc`
/// metres at its middle, at `flow` (0 off to 1 full).
#[derive(Clone, Copy, Debug)]
pub struct Stream {
    pub from: Vec3,
    pub to: Vec3,
    pub arc: f32,
    pub flow: f32,
}

impl Stream {
    /// The point `u` (0 nozzle .. 1 landing) along the stream.
    pub fn at(&self, u: f32) -> Vec3 {
        self.from + (self.to - self.from) * u + Vec3::Y * (self.arc * 4.0 * u * (1.0 - u))
    }
}

/// The ladder's pose relative to the truck.
#[derive(Clone, Copy, Debug, Default)]
pub struct LadderPose {
    /// Turn of the turntable from straight ahead (radians, sim sense).
    pub yaw_rel: f32,
    /// Elevation (radians).
    pub pitch: f32,
    /// Length from the pivot to the tip (metres).
    pub length: f32,
    /// Outriggers out, 0 stowed to 1 down.
    pub outriggers: f32,
}

impl LadderPose {
    /// Stowed on the roof, pointing forward over the cab.
    pub fn stowed() -> Self {
        LadderPose { yaw_rel: 0.0, pitch: 0.0, length: LADDER_SECTION, outriggers: 0.0 }
    }

    /// How far each fly section slides out past the one below it.
    pub fn section_offset(&self) -> f32 {
        ((self.length - LADDER_SECTION) / (LADDER_SECTIONS - 1) as f32).max(0.0)
    }
}

/// A fire engine at work.
#[derive(Clone, Debug)]
pub struct EngineScene {
    pub unit: UnitId,
    pub incident: u32,
    pub building: Option<BuildingId>,
    pub ladder: LadderPose,
    /// The ladder's tip in render space.
    pub tip: Vec3,
    pub figures: Vec<Figure>,
    /// Hose lines on the ground, as points in render space.
    pub hoses: Vec<Vec<Vec3>>,
    pub streams: Vec<Stream>,
    pub packing: bool,
    /// Seconds (at 1x) since the engine arrived.
    pub secs: f32,
    /// Litres of water put on the fire so far (minutes of flow at the
    /// lines' rates).
    pub litres: f32,
    /// Where the water is aimed: the roof line, or the facade of a tower.
    pub aim: Vec3,
    /// Too tall for the ladder to reach the roof: it works the upper floors
    /// it can reach, where the renderer shows flames in the windows.
    pub tower: bool,
    /// The fire's intensity now (0 once out).
    pub intensity: f32,
    /// Which way the facade the crew works faces (out of the building).
    pub facing: SVec2,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn wrap(mut a: f32) -> f32 {
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

fn heading_of(v: SVec2) -> f32 {
    v.y.atan2(v.x)
}

/// A stable small hash for looks of crews with nobody on the payroll.
fn hash(a: u64) -> u64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    x.wrapping_mul(0xBF58_476D_1CE4_E5B9)
}

/// The ground (or the deck) at `p`.
fn ground_at(w: &World, p: SVec2) -> f32 {
    w.surface_height_near(p, w.ground_height(p))
}

/// [fire-scene] How tall each building is drawn (the top of its walls and
/// roof above its pad), written by the renderer as it builds them: style
/// towers rise well above floors x storey height. Missing (the picker, a
/// building not drawn yet), the storey count stands in.
#[derive(Default)]
pub struct BuildingTops(pub std::collections::HashMap<BuildingId, f32>);

impl BuildingTops {
    /// The building's drawn height, else floors at the storey height.
    pub fn height(&self, w: &World, b: BuildingId, rules: &BuildingRules) -> f32 {
        match (self.0.get(&b), w.buildings.get(b)) {
            (Some(t), _) if *t > 1.0 => t.min(400.0),
            (_, Some(bld)) => (bld.floors.max(1) as f32 * rules.for_land_use(bld.land_use).floor_height).min(160.0),
            _ => 3.0,
        }
    }
}

/// The default building rules, for callers without the game's assets (the
/// picker): heights come out the same unless a mod changes storeys.
pub fn default_rules() -> &'static BuildingRules {
    static R: std::sync::OnceLock<BuildingRules> = std::sync::OnceLock::new();
    R.get_or_init(BuildingRules::default)
}

/// The point of the building's outline nearest `p`, pulled in by the land
/// use's setback, and the building's height (as the generator stacks it).
fn facade(w: &World, b: BuildingId, p: SVec2, rules: &BuildingRules, tops: &BuildingTops) -> Option<(SVec2, SVec2, f32)> {
    let bld = w.buildings.get(b)?;
    let lot = w.footprint(b)?;
    let poly = &lot.polygon;
    let mut best = (f32::MAX, lot.center);
    for i in 0..poly.len() {
        let (a, c) = (poly[i], poly[(i + 1) % poly.len()]);
        let ab = c - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let q = a + ab * t;
        let d = q.distance(p);
        if d < best.0 {
            best = (d, q);
        }
    }
    let lu = rules.for_land_use(bld.land_use);
    let inward = (lot.center - best.1).normalize_or_zero();
    let f = best.1 + inward * lu.inset.min(best.1.distance(lot.center) * 0.5);
    Some((f, lot.center, tops.height(w, b, rules)))
}

/// The scene of fire engine `uid`, if it is on scene at a fire (fighting
/// it, or packing up after).
pub fn engine_scene(w: &World, uid: UnitId, frac: f32, rules: &BuildingRules, tops: &BuildingTops) -> Option<EngineScene> {
    let u = w.services.units.get(uid)?;
    if u.kind != UnitKind::FireEngine || u.state != UnitState::OnScene || u.car.is_some() {
        return None;
    }
    let inc = w.services.incident(u.incident?)?;
    if inc.kind != IncidentKind::Fire {
        return None;
    }
    let spt = w.config.traffic_seconds_per_tick.max(0.1);
    let tick = w.clock.tick;
    let arrived = inc.arrived.unwrap_or(tick);
    let secs = ((tick.saturating_sub(arrived)) as f32 + frac) * spt;
    let packing = u.packing() || !inc.active();
    let pack = if packing { ((tick.saturating_sub(inc.resolved.unwrap_or(tick))) as f32 + frac) * spt } else { 0.0 };
    let (p, h) = (u.pos, u.heading);
    let fwd = SVec2::new(h.cos(), h.sin());
    let left = SVec2::new(-h.sin(), h.cos());
    let ground = ground_at(w, p);
    let building = Some(inc.building).filter(|b| w.buildings.contains_key(*b));
    let (face, centre, height) = building.and_then(|b| facade(w, b, p, rules, tops)).unwrap_or((inc.pos, inc.pos, 3.0));
    let face_ground = ground_at(w, face);
    let side = if (face - p).dot(left) >= 0.0 { 1.0 } else { -1.0 };
    let world = |q: SVec2, y: f32| Vec3::new(q.x, y, q.y);
    let local = |x: f32, z: f32| p + fwd * x + left * z;

    // ── the ladder ──
    let pivot2 = local(LADDER_PIVOT[0], LADDER_PIVOT[2]);
    let pivot = world(pivot2, ground + LADDER_PIVOT[1]);
    let to_face = (face - pivot2).normalize_or(fwd);
    let depth = face.distance(centre);
    // Over the roof line of anything it can reach; a tower's facade as high
    // up as it goes.
    let stand_off = pivot2.distance(face - to_face * 1.8);
    let can_rise = (LADDER_REACH * LADDER_REACH - stand_off * stand_off).max(0.0).sqrt().min(LADDER_REACH * LADDER_MAX_PITCH.sin()) * 0.97;
    let low = face_ground + height + 1.5 - pivot.y < can_rise;
    // A tower: the highest floor it can reach from where it stands.
    let tip_goal = if low { world(face - to_face * 1.2, face_ground + height + 1.5) } else { world(face - to_face * 1.8, (face_ground + height - 2.0).min(pivot.y + can_rise)) };
    let v = tip_goal - pivot;
    let horiz = SVec2::new(v.x, v.z).length();
    let pitch_goal = v.y.atan2(horiz).clamp(0.05, LADDER_MAX_PITCH);
    let length_goal = v.length().clamp(LADDER_SECTION, LADDER_REACH);
    let yaw_goal = wrap(heading_of(to_face) - h);
    // Deploying: outriggers, raise, turn, extend. Packing: the reverse.
    let mut out = smooth((secs - 0.5) / 2.0);
    let mut raise = smooth((secs - 2.5) / 3.0);
    let mut turn = smooth((secs - 3.5) / 3.5);
    let mut extend = smooth((secs - 6.5) / 5.0);
    if packing {
        extend = extend.min(1.0 - smooth(pack / 5.5));
        turn = turn.min(1.0 - smooth((pack - 5.5) / 2.0));
        raise = raise.min(1.0 - smooth((pack - 7.0) / 2.2));
        out = out.min(1.0 - smooth((pack - 9.4) / 1.5));
    }
    let ladder = LadderPose { yaw_rel: yaw_goal * turn, pitch: pitch_goal * raise, length: LADDER_SECTION + (length_goal - LADDER_SECTION) * extend, outriggers: out };
    let ldir = {
        let yaw = h + ladder.yaw_rel;
        Vec3::new(yaw.cos() * ladder.pitch.cos(), ladder.pitch.sin(), yaw.sin() * ladder.pitch.cos())
    };
    let along = |d: f32| pivot + ldir * d;
    let tip = along(ladder.length);

    // ── the crew ──
    let crew = w.unit_crew(uid);
    let n = u.kind.crew().max(crew.len()).max(3);
    let door = |i: usize| local(if i == 0 { 2.9 } else { 1.7 }, side * 1.5);
    let pump_at = local(-0.4, side * 1.85);
    // The hose crew stands off the building, to the front of the ladder's line.
    let dist = face.distance(p);
    let to_b = (face - p).normalize_or(left * side);
    let mut perp = SVec2::new(-to_b.y, to_b.x);
    if perp.dot(fwd) < 0.0 {
        perp = -perp;
    }
    let nozzle_at = face - to_b * (dist * 0.45).clamp(5.0, 11.0) + perp * 3.5;
    // Where the hand line's water lands: over a low roof, else the facade
    // a few floors up.
    let hand_goal = if height <= 16.0 { world(face + (centre - face).normalize_or_zero() * depth.min(3.0), face_ground + height + 0.3) } else { world(face, face_ground + height.min(13.0)) };
    let rear = local(-4.0, side * 0.6);
    let mut figures = Vec::new();
    let mut nozzle_hand: Option<Vec3> = None;
    let mut nozzle_on = 0.0f32;
    let mut ladder_on = 0.0f32;
    let mut pump_on = false;
    for i in 0..n {
        let role = match i {
            0 => Role::Pump,
            1 => Role::Nozzle,
            2 => Role::Ladder,
            _ => Role::Backup,
        };
        let citizen = crew.get(i).copied();
        let (skin, stature) = match citizen.and_then(|c| w.citizens.get(c)) {
            Some(c) => {
                let look = c.appearance();
                (((look.genes.skin.clamp(0.0, 1.0) * nd_procgen::people::SKIN_STEPS as f32) as u8).min(nd_procgen::people::SKIN_STEPS - 1), look.stature(c.age))
            }
            None => ((hash(i as u64 + u.number as u64 * 7) % nd_procgen::people::SKIN_STEPS as u64) as u8, 1.0),
        };
        let from = door(i.min(1));
        let start = 0.9 + 0.45 * i as f32;
        let back_at = match role {
            Role::Ladder => 8.0,
            Role::Pump => 2.5,
            _ => 1.0,
        };
        let station = match role {
            Role::Pump => pump_at,
            Role::Nozzle => nozzle_at,
            Role::Backup => nozzle_at - to_b * (1.3 * (i - 2) as f32) + perp * 0.4,
            Role::Ladder => rear,
        };
        let leg = from.distance(station).max(0.1);
        let walk_secs = leg / PACE;
        // Walking out (and back in when packing): how far along the leg.
        let t_out = ((secs - start) / walk_secs).clamp(0.0, 1.0);
        let t_back = if packing { ((pack - back_at) / walk_secs).clamp(0.0, 1.0) } else { 0.0 };
        let k = (t_out - t_back).max(0.0);
        let aboard = secs < start || (packing && t_back >= 1.0);
        let mut pos2 = from + (station - from) * k;
        let moving = !aboard && ((t_out > 0.0 && t_out < 1.0) || (t_back > 0.0 && t_back < 1.0));
        let mut yaw = if t_back > 0.0 { heading_of(from - station) } else { heading_of(station - from) };
        let mut y = ground_at(w, pos2);
        let mut pose = FirePose::Stand;
        let arrived = t_out >= 1.0 && t_back <= 0.0;
        match role {
            Role::Pump if arrived => {
                pose = FirePose::Pump;
                yaw = heading_of(-left * side);
                pump_on = true;
            }
            Role::Nozzle | Role::Backup if arrived => {
                pose = FirePose::Nozzle;
                let g = SVec2::new(hand_goal.x, hand_goal.z);
                yaw = heading_of(g - pos2);
                if role == Role::Nozzle {
                    let reached = start + walk_secs;
                    nozzle_on = smooth((secs - reached - 0.8) / 0.8);
                }
            }
            Role::Ladder if !aboard => {
                // Out at the back, up onto the turntable, up the ladder to
                // the tip; down and back the same way.
                let up_at = start + walk_secs;
                let climb_up = smooth((secs - up_at) / 1.0);
                let climb_down = if packing { smooth((pack - 7.4) / 0.6) } else { 0.0 };
                let on_deck = (climb_up - climb_down).max(0.0);
                if on_deck > 0.0 && t_back <= 0.0 {
                    let stand = pivot + Vec3::new(0.0, -0.25, 0.0) + Vec3::new(-fwd.x, 0.0, -fwd.y) * 0.6;
                    let foot = world(rear, ground_at(w, rear));
                    let mut at = foot.lerp(stand, on_deck);
                    // Up the ladder once it is extending.
                    let mut on_ladder = (CLIMB * (secs - 6.5)).max(0.0).min(ladder.length - 0.8);
                    if packing {
                        on_ladder = on_ladder.min((ladder.length - 0.8 - CLIMB * (pack - 5.0).max(0.0)).max(0.0));
                    }
                    if on_ladder > 0.0 && on_deck >= 1.0 {
                        at = along(on_ladder) + Vec3::Y * 0.2;
                    }
                    pos2 = SVec2::new(at.x, at.z);
                    y = at.y;
                    yaw = h + ladder.yaw_rel;
                    if on_ladder >= ladder.length - 0.85 && extend >= 0.98 {
                        pose = FirePose::Nozzle;
                        ladder_on = smooth((secs - 6.5 - (ladder.length - 0.8) / CLIMB - 0.6) / 0.8);
                    }
                }
            }
            _ => {}
        }
        if role == Role::Nozzle && !aboard {
            nozzle_hand = Some(world(pos2, y) + Vec3::Y * 1.05 * stature);
        }
        figures.push(Figure { citizen, role, pos: world(pos2, y), yaw, pose, walking: moving, out: !aboard, skin, stature });
    }
    if packing {
        nozzle_on = nozzle_on.min(1.0 - smooth(pack / 0.6));
        ladder_on = ladder_on.min(1.0 - smooth(pack / 0.6));
    }
    if !pump_on {
        nozzle_on = 0.0;
        ladder_on = 0.0;
    }

    // ── hoses and water ──
    let mut hoses = Vec::new();
    let outlet = world(local(-0.4, side * 1.3), ground + 0.9);
    if let Some(hand) = nozzle_hand {
        // Down from the panel, out in a lazy S along the ground to the hands.
        let a = SVec2::new(outlet.x, outlet.z) + left * side * 1.0;
        let b = SVec2::new(hand.x, hand.z) - (SVec2::new(hand.x, hand.z) - a).normalize_or_zero() * 0.6;
        let mut pts = vec![outlet];
        let bend = (b - a).perp().normalize_or_zero() * (a.distance(b) * 0.12).min(2.5);
        let segs = 8;
        for k in 0..=segs {
            let t = k as f32 / segs as f32;
            let q = a + (b - a) * t + bend * (std::f32::consts::PI * 2.0 * t).sin() * (1.0 - t * 0.3);
            pts.push(world(q, ground_at(w, q) + 0.07));
        }
        pts.push(hand - Vec3::Y * 0.1);
        hoses.push(pts);
    }
    let mut streams = Vec::new();
    if let Some(hand) = nozzle_hand.filter(|_| nozzle_on > 0.0) {
        let dir = (hand_goal - hand).normalize_or_zero();
        let from = hand + dir * 0.9 * figures.get(1).map_or(1.0, |f| f.stature);
        let flat = SVec2::new(hand_goal.x - from.x, hand_goal.z - from.z).length();
        streams.push(Stream { from, to: hand_goal, arc: flat * 0.16 + 1.0, flow: nozzle_on });
    }
    if ladder_on > 0.0 {
        let ladder_goal = if low { world(face + (centre - face).normalize_or_zero() * depth.min(6.0), face_ground + height + 0.2) } else { world(face, tip.y + 0.5) };
        let from = tip + ldir * 0.6 + Vec3::Y * 0.9;
        let flat = SVec2::new(ladder_goal.x - from.x, ladder_goal.z - from.z).length();
        streams.push(Stream { from, to: ladder_goal, arc: flat * 0.12 + 0.5, flow: ladder_on });
    }
    // Water so far: each line's minutes at full flow, roughly.
    let end = inc.resolved.unwrap_or(tick).min(tick);
    let mins = end.saturating_sub(arrived) as f32;
    let litres = (mins - 1.0).max(0.0) * HOSE_FLOW + (mins - 3.0).max(0.0) * LADDER_FLOW;
    Some(EngineScene {
        unit: uid,
        incident: inc.id,
        building,
        ladder,
        tip,
        figures,
        hoses,
        streams,
        packing,
        secs,
        litres,
        aim: if low { world(face, face_ground + height) } else { world(face, tip_goal.y) },
        tower: !low,
        intensity: if inc.active() { inc.intensity } else { 0.0 },
        facing: -to_face,
    })
}

/// Every fire engine at work this frame.
pub fn engine_scenes(w: &World, frac: f32, rules: &BuildingRules, tops: &BuildingTops) -> Vec<EngineScene> {
    w.services.units.iter().filter(|(_, u)| u.kind == UnitKind::FireEngine && u.state == UnitState::OnScene && u.car.is_none()).filter_map(|(id, _)| engine_scene(w, id, frac, rules, tops)).collect()
}

// ── units as things to click ───────────────────────────────────────────────

/// Where a unit is drawn this frame: on the road (its car), or standing
/// (on scene, on a beat). `None` while it is in its station.
pub fn unit_pose(w: &World, uid: UnitId, frac: f32) -> Option<(SVec2, f32)> {
    let u = w.services.units.get(uid)?;
    match u.car.and_then(|c| w.traffic.cars.get(c)) {
        Some(car) => Some(car.pose_at(frac)),
        None if matches!(u.state, UnitState::OnScene | UnitState::Patrol) => Some((u.pos, u.heading)),
        None => None,
    }
}

/// Half length and half width of a unit's body.
fn half_size(k: UnitKind) -> (f32, f32) {
    match k {
        UnitKind::FireEngine => (4.0, 1.4),
        UnitKind::PoliceCar => (2.3, 1.0),
        UnitKind::Ambulance => (2.9, 1.2),
    }
}

/// The unit standing (not driving) under `p`, by its body's outline plus a
/// little: how far from its middle, and which. Driving units are picked as
/// their car (`unit_of_car`).
pub fn pick_standing(w: &World, p: SVec2) -> Option<(f32, UnitId)> {
    w.services
        .units
        .iter()
        .filter(|(_, u)| u.car.is_none() && matches!(u.state, UnitState::OnScene | UnitState::Patrol))
        .filter_map(|(id, u)| {
            let (hl, hw) = half_size(u.kind);
            let d = p - u.pos;
            let fwd = SVec2::new(u.heading.cos(), u.heading.sin());
            let (a, b) = (d.dot(fwd).abs(), d.perp_dot(fwd).abs());
            (a < hl + 1.0 && b < hw + 1.0).then_some((d.length(), id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// A firefighter on scene under `p` (their feet), with who they are.
pub fn pick_crew(w: &World, tops: &BuildingTops, p: SVec2, frac: f32) -> Option<(f32, CitizenId)> {
    engine_scenes(w, frac, default_rules(), tops)
        .iter()
        .flat_map(|s| s.figures.iter())
        .filter(|f| f.out && f.pos.y < 2.5 + ground_at(w, SVec2::new(f.pos.x, f.pos.z)))
        .filter_map(|f| f.citizen.map(|c| (SVec2::new(f.pos.x, f.pos.z).distance(p), c)))
        .filter(|(d, _)| *d < 1.6)
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// "Engine 2 · Harbour Fire Station".
pub fn unit_title(w: &World, uid: UnitId) -> String {
    let Some(u) = w.services.units.get(uid) else { return "a unit that is gone".into() };
    let station = w.buildings.get(u.station).map_or("", |b| b.name.as_str());
    format!("{} {}  ·  {station}", u.kind.call_sign(), u.number)
}

/// What a unit is doing, in a line: "on scene at Mira Tower fire, 12 min".
pub fn unit_doing(w: &World, uid: UnitId) -> String {
    let Some(u) = w.services.units.get(uid) else { return String::new() };
    let tick = w.clock.tick;
    let inc = u.incident.and_then(|i| w.services.incident(i));
    let place = inc.map(|i| if i.place.is_empty() { w.buildings.get(i.building).map_or_else(|| "the scene".to_string(), |b| b.name.clone()) } else { i.place.clone() });
    let what = |k: IncidentKind| match k {
        IncidentKind::Fire => "fire",
        IncidentKind::Crime => "call",
        IncidentKind::Medical => "medical call",
        IncidentKind::Crash => "crash",
    };
    match (u.state, inc) {
        (UnitState::OnScene, Some(i)) if u.packing() => format!("packing up at {}, the fire {}", place.unwrap_or_default(), if i.outcome == Some(Outcome::Extinguished) { "out" } else { "over" }),
        (UnitState::OnScene, Some(i)) => format!("on scene at {} {}, {} min", place.unwrap_or_default(), what(i.kind), tick.saturating_sub(i.arrived.unwrap_or(tick))),
        (UnitState::Responding, Some(i)) => format!("responding to a {} at {}, {} min since the call", what(i.kind), place.unwrap_or_default(), tick.saturating_sub(i.dispatched.unwrap_or(tick))),
        (UnitState::Transporting, _) => format!("taking a patient to {}", u.hospital.and_then(|h| w.buildings.get(h)).map_or("hospital", |b| b.name.as_str())),
        (UnitState::Pursuing, _) => "in pursuit".into(),
        (UnitState::Patrol, _) if u.kind == UnitKind::PoliceCar => if u.car.is_some() { "on patrol".into() } else { "on patrol, parked on the beat".into() },
        (UnitState::Patrol, _) => if u.car.is_some() { "on standby, moving".into() } else { "on standby in the district".into() },
        (UnitState::Returning, _) => "going back to the station".into(),
        (s, _) => s.label().to_string(),
    }
}

/// How bad a fire is, in a word.
pub fn fire_word(intensity: f32) -> &'static str {
    match intensity {
        i if i <= 0.0 => "out",
        i if i < 0.2 => "smouldering",
        i if i < 0.45 => "burning",
        i if i < 0.75 => "well alight",
        _ => "fully involved",
    }
}

/// Minutes until a fire with `intensity` is out at one engine's rate.
pub fn minutes_to_out(intensity: f32) -> u32 {
    (intensity.max(0.0) / nd_sim::services::ENGINE_RATE).ceil() as u32
}

/// The incident a unit is at or on its way to (also just after it ended,
/// while packing up).
pub fn unit_incident(w: &World, uid: UnitId) -> Option<&nd_sim::services::Incident> {
    w.services.units.get(uid)?.incident.and_then(|i| w.services.incident(i))
}

/// The share of a fire's knock-down done (0..1): what the engine has
/// knocked off since it got there (`ENGINE_RATE` a minute) against what is
/// left.
pub fn fire_progress(w: &World, i: &nd_sim::services::Incident) -> Option<f32> {
    let arrived = i.arrived?;
    if !i.active() {
        return Some(if i.outcome == Some(Outcome::Extinguished) { 1.0 } else { 0.0 });
    }
    let knocked = w.clock.tick.saturating_sub(arrived) as f32 * nd_sim::services::ENGINE_RATE;
    Some((knocked / (knocked + i.intensity.max(0.0)).max(1e-6)).clamp(0.0, 1.0))
}

/// [fire-scene] Where a crew member is drawn while out with their unit: on
/// foot at the scene (their figure), else riding in it. `(pos, heading,
/// eye height)`.
pub fn crew_pose(w: &World, tops: &BuildingTops, c: CitizenId, frac: f32) -> Option<(SVec2, f32, f32)> {
    let uid = w.crew_unit_of(c)?;
    if let Some(fig) = engine_scene(w, uid, frac, default_rules(), tops).and_then(|s| s.figures.into_iter().find(|f| f.citizen == Some(c) && f.out)) {
        return Some((SVec2::new(fig.pos.x, fig.pos.z), fig.yaw, fig.pos.y + 1.65 * fig.stature));
    }
    let (p, h) = unit_pose(w, uid, frac)?;
    Some((p, h, ground_at(w, p) + 2.2))
}
