//! [waterfront] The shore as the city uses it: lots that run down to the
//! water, public beaches, and the private jetties businesses on the water
//! build for themselves.
//!
//! - **Waterfront lots.** Where a street or path runs within
//!   [`crate::zoning::SHORE_REACH`] of a river, lake or the sea, the land
//!   between it and the water is parcelled the whole way down to the
//!   waterline (`zoning::generate_parcels_with`). Every lot knows its
//!   frontage on the water (`Lot::shore`); waterfront lots carry a land
//!   value premium, and the beach resorts, waterfront hotels, restaurants
//!   on piers, beach bars and boat rentals that live on the view open only
//!   on them (`crate::pressure`).
//! - **Public beaches.** A park painted as a beach (the Zone tool's
//!   "Public beach", a park sub-zone) on parcels that touch the water. Its
//!   layout is sand: a boardwalk from the way in down towards the water,
//!   and towel spots people walk to and lie on (the park's benches, so the
//!   seats system crowds it). A beach joins the walk graph only through a
//!   street or path it fronts, or a park next to it (`walk.rs`); one with
//!   neither is shut off ("no access"). How many go depends on the weather
//!   ([`beach_weather`]: nobody in the cold or the rain, Arctic beaches are
//!   empty), the hour, and the people within reach; ground or water fouled
//!   by dirty industry closes it ([`BEACH_CLOSE_AT`]) with a sign until it
//!   clears. Open beaches lift the land around, cheer the households near
//!   them and draw tourists.
//! - **Private jetties.** A fishing company, a fish works or a shipping
//!   and industrial business on a waterfront lot whose water is navigable
//!   (the sea, a lake, a river at least [`NAVIGABLE_RIVER`] wide) builds a
//!   small jetty of its own ([`plan_jetty`]): a trawler's berth, or a quay
//!   where a coaster lands its imports at the sea price. Pier restaurants
//!   and boat rentals get a pier or a pontoon the same way.
//!
//! Pure data and rules here; the world side is `world/shore.rs`. Nothing
//! here draws a random number: choices hash the seed.

use crate::buildings::{BuildingId, Tier};
use crate::geom::{distance_to_outline, point_in_polygon, polygon_area};
use crate::landscape::{Landscape, WaterKind};
use crate::park::{pull_inside, ParkLayout};
use crate::traffic::Cargo;
use crate::weather::Spell;
use crate::zoning::Lot;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The park sub-zone a beach is painted with (`Gesture::ParcelZone`'s
/// sub-zone string).
pub const BEACH_SUBZONE: &str = "beach";
/// Contamination over the sand or the water off it that closes a beach,
/// and the level it must fall below to open again.
pub const BEACH_CLOSE_AT: f32 = 0.2;
pub const BEACH_REOPEN_AT: f32 = 0.12;
/// Square metres of sand per bather before a beach is crowded.
pub const M2_PER_BATHER: f32 = 16.0;
/// What an open beach does for the land round it: (radius, lift).
pub const BEACH_LIFT: (f32, f32) = (240.0, 0.16);
/// The premium a waterfront lot carries: (radius, lift at full frontage).
pub const WATERFRONT_LIFT: (f32, f32) = (55.0, 0.12);
/// Frontage (m) at which a lot takes the whole premium.
pub const FULL_FRONTAGE: f32 = 30.0;
/// Households this close (straight line) to an open beach are cheered by it.
pub const BEACH_HAPPY_REACH: f32 = 700.0;
/// Narrowest river a jetty's boats can use. Lakes and the sea always do.
pub const NAVIGABLE_RIVER: f32 = 18.0;
/// What a private jetty costs the business that builds it.
pub const JETTY_COST: f32 = 600.0;
/// Hours a load ordered through a private cargo jetty takes to come in.
pub const JETTY_LEAD_HOURS: u64 = 5;
/// Residents within this of a beach (straight line) count as its catchment.
pub const BEACH_CATCHMENT: f32 = 1500.0;
/// Share of a beach's catchment on the sand at the best hour of a perfect day.
pub const PEAK_SHARE: f32 = 0.08;

/// How inviting the beach is, `0..1`, for the air temperature, the sky,
/// the spell and the hour: nobody below 12 °C or in a storm, few under a
/// sky heavy enough to rain, the crowd building from the morning to a
/// mid-afternoon peak and nearly gone after dark (a few night swimmers
/// under the string lights).
pub fn beach_weather(temp_c: f32, cloud: f32, spell: Spell, hour: f32) -> f32 {
    if spell == Spell::Storm {
        return 0.0;
    }
    let smooth = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let warm = smooth(12.0, 26.0, temp_c) * (1.0 - 0.6 * smooth(38.0, 46.0, temp_c));
    // Heavy cloud reads as rain.
    let sky = if cloud > 0.82 { 0.15 } else if cloud > 0.6 { 0.55 } else { 1.0 - 0.3 * cloud };
    let h = hour.rem_euclid(24.0);
    let day = if (7.0..21.0).contains(&h) {
        // Up from the morning, a peak 12-16, down into the evening.
        let up = smooth(7.0, 12.0, h);
        let down = 1.0 - smooth(16.0, 21.0, h);
        0.08 + 0.92 * up.min(down)
    } else {
        0.03
    };
    (warm * sky * day).clamp(0.0, 1.0)
}

/// Is this water a boat can work: the sea, a lake, or a river wide enough?
pub fn navigable(kind: WaterKind, width: f32) -> bool {
    match kind {
        WaterKind::Sea | WaterKind::Lake => true,
        WaterKind::River => width >= NAVIGABLE_RIVER,
        WaterKind::Stream => false,
    }
}

/// A beach's name from its seed: fictional, like every name in the city.
pub fn beach_name(seed: u64) -> String {
    const FIRST: [&str; 16] = ["Coral", "Driftwood", "Saltglass", "Tidewater", "Moonrise", "Cobalt", "Seaglass", "Lantern", "Sunset", "Pearl", "Static", "Halcyon", "Palisade", "Kestrel", "Marigold", "Ember"];
    const LAST: [&str; 6] = ["Beach", "Sands", "Strand", "Cove", "Shore", "Beach"];
    let h = mix(seed, 0xBEAC);
    format!("{} {}", FIRST[(h % 16) as usize], LAST[((h >> 8) % 6) as usize])
}

fn mix(a: u64, b: u64) -> u64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 29)
}

/// The way down to the water on a beach lot, and the towel spots: a
/// boardwalk from the entrance (the street side, or the side nearest the
/// lot's middle for a beach reached through a park) towards the water,
/// and spots on a loose grid across the sand, each facing the water. The
/// spots are the lot's benches: people walk to one and lie down.
pub fn plan_beach(lot: &Lot, tier: Tier) -> ParkLayout {
    let poly = &lot.polygon;
    let sea = if lot.shore.seaward != Vec2::ZERO { lot.shore.seaward } else { -lot.front_dir };
    let entrance = pull_inside(poly, lot.entrance() - lot.front_dir * 1.5, lot.center);
    // The waterline: the far end of the lot towards the water.
    let reach = poly.iter().map(|p| (*p - entrance).dot(sea)).fold(0.0f32, f32::max);
    let down = pull_inside(poly, entrance + sea * (reach - 6.0).max(2.0), lot.center);
    let mid = entrance.lerp(down, 0.5);
    let walk = vec![entrance, mid, down];
    let facing = sea.y.atan2(sea.x);
    let mut benches: Vec<(Vec2, f32)> = Vec::new();
    let along = Vec2::new(-sea.y, sea.x);
    let area = polygon_area(poly).abs();
    let cap = (area / 26.0).clamp(4.0, 90.0) as usize;
    let (spacing_u, spacing_v) = (4.2f32, 4.8f32);
    let (u0, u1) = poly.iter().map(|p| p.dot(along)).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (v0, v1) = poly.iter().map(|p| p.dot(sea)).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let seed = (lot.center.x.to_bits() as u64) << 32 | lot.center.y.to_bits() as u64;
    // Rows from the water's edge back up the sand, staggered.
    let mut row = 0;
    let mut v = v1 - 4.0;
    while v > v0 + 3.0 && benches.len() < cap {
        let shift = if row % 2 == 0 { 0.0 } else { spacing_u * 0.5 };
        let mut u = u0 + 2.5 + shift;
        while u < u1 - 2.0 && benches.len() < cap {
            let jitter = (mix(seed, (row * 131 + benches.len()) as u64) % 1000) as f32 / 1000.0 - 0.5;
            let p = along * (u + jitter * 1.2) + sea * (v + jitter * 0.8);
            let clear_of_walk = walk.windows(2).all(|w| crate::geom::project_on_segment(p, w[0], w[1]).1.distance(p) > 2.2);
            if point_in_polygon(p, poly) && distance_to_outline(p, poly) > 2.0 && clear_of_walk {
                benches.push((p, facing + jitter * 0.4));
            }
            u += spacing_u;
        }
        v -= spacing_v;
        row += 1;
    }
    ParkLayout {
        tier,
        paths: vec![walk],
        benches,
        features: Vec::new(),
        entrances: vec![entrance],
        shared_edges: Vec::new(),
        cluster_lots: 1,
        cluster_area: area,
        anchor: true,
        seams: Vec::new(),
    }
}

/// What a private jetty is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JettyKind {
    /// A fishing company's: a trawler's berth.
    Fishing,
    /// A works' quay: a coaster lands its imports.
    Cargo,
    /// A restaurant's deck on piles over the water.
    Pier,
    /// A boat rental's floating pontoon of small boats.
    Pontoon,
}

impl JettyKind {
    pub fn label(self) -> &'static str {
        match self {
            JettyKind::Fishing => "fishing jetty",
            JettyKind::Cargo => "cargo quay",
            JettyKind::Pier => "restaurant pier",
            JettyKind::Pontoon => "boat pontoon",
        }
    }

    /// Length out over the water, and width, metres.
    pub fn size(self) -> (f32, f32) {
        match self {
            JettyKind::Fishing => (26.0, 3.5),
            JettyKind::Cargo => (22.0, 7.0),
            JettyKind::Pier => (16.0, 12.0),
            JettyKind::Pontoon => (20.0, 2.6),
        }
    }

    /// Boats that tie up at once: a small dock.
    pub fn berths(self) -> u32 {
        match self {
            JettyKind::Fishing => 1,
            JettyKind::Cargo => 1,
            JettyKind::Pier => 0,
            JettyKind::Pontoon => 6,
        }
    }
}

/// A business's own small dock on the water off its lot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Jetty {
    pub kind: JettyKind,
    /// On the shore, and the end out over the water.
    pub root: Vec2,
    pub tip: Vec2,
    /// Where a boat lies alongside.
    pub berth: Vec2,
    /// Unit vector out over the water.
    pub seaward: Vec2,
    pub water: WaterKind,
    /// Built on this tick.
    pub built: u64,
    /// Loads landed at it, all time.
    #[serde(default)]
    pub loads: u32,
}

/// Where a jetty of `kind` goes off a lot: at the middle of its stretch
/// of waterline, square to the shore, out into water a boat can work.
/// `Err` says why not.
pub fn plan_jetty(lot: &Lot, land: &Landscape, kind: JettyKind) -> Result<Jetty, &'static str> {
    if !lot.shore.is_waterfront() || land.is_flat() {
        return Err("the lot does not reach the water");
    }
    let sea = lot.shore.seaward;
    // The edge midpoint furthest out towards the water.
    let n = lot.polygon.len();
    let start = (0..n)
        .map(|i| lot.polygon[i].lerp(lot.polygon[(i + 1) % n], 0.5))
        .max_by(|a, b| a.dot(sea).total_cmp(&b.dot(sea)))
        .ok_or("no outline")?;
    let shore = land.nearest_shore(start - sea * 3.0, 25.0).ok_or("the lot does not reach the water")?;
    let w = shore.water;
    if !navigable(w.kind, w.width) {
        return Err(match w.kind {
            WaterKind::Stream => "a stream is too narrow for boats",
            _ => "the river is too narrow here for boats",
        });
    }
    let (len, _) = kind.size();
    let len = if w.width.is_finite() { len.min(w.width * 0.35) } else { len };
    let seaward = shore.seaward;
    let root = shore.point - seaward * 3.0;
    let tip = shore.point + seaward * len;
    if !land.is_water(tip) || !land.is_water(shore.point + seaward * (len * 0.5)) {
        return Err("open water is too far out from this lot");
    }
    let side = Vec2::new(-seaward.y, seaward.x);
    let berth = shore.point + seaward * (len * 0.7) + side * (kind.size().1 * 0.5 + 3.0);
    Ok(Jetty { kind, root, tip, berth, seaward, water: w.kind, built: 0, loads: 0 })
}

/// How a beach is doing now.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeachState {
    pub name: String,
    /// Reached on foot from a street, a path or a park.
    pub access: bool,
    /// Open to bathers (not fouled).
    pub open: bool,
    /// Why it is closed, or has no visitors, in words.
    pub note: Option<String>,
    /// People on the sand and in the water now, by the hour's reckoning
    /// (citizens and visitors from out of town).
    pub visitors: u32,
    /// Of them, in the water.
    pub swimmers: u32,
    /// The crowd the hour would bring, before rounding to people.
    pub draw: f32,
    /// Citizens and tourists actually there (the sim's own people).
    pub people_here: u32,
    /// The comfortable crowd for its sand.
    pub capacity: u32,
    /// Most at once today, and the day's bather-hours.
    pub peak_today: u32,
    pub visits_today: u32,
    pub visits_yesterday: u32,
    /// How inviting it is now (weather and hour), `0..1`.
    pub appeal: f32,
    /// Worst contamination over the sand and the water off it.
    pub contamination: f32,
    /// Residents within reach.
    pub catchment: u32,
    /// What the beach looked like when last drawn (crowd bucket, shut,
    /// night), to redraw it only when that changes.
    #[serde(skip)]
    pub shown: u32,
}

impl BeachState {
    pub fn crowded(&self) -> bool {
        self.capacity > 0 && self.visitors > self.capacity
    }

    /// Open and reachable: people go.
    pub fn usable(&self) -> bool {
        self.access && self.open
    }
}

/// The saved waterfront state (`World::shore`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShoreState {
    /// Parcels along a road near the water reach down to it. True for
    /// cities started since; a save from before keeps the parcels it was
    /// cut with (lots reshaped under standing buildings would be pulled
    /// down), and can switch it on (`World::set_waterfront_parcels`).
    pub reach_water: bool,
    /// Beaches by the park building on their lot.
    pub beaches: BTreeMap<BuildingId, BeachState>,
    /// Private jetties by the business that built them.
    pub jetties: BTreeMap<BuildingId, Jetty>,
    /// Loads coming in by a private quay: the buyer, the goods, the tick
    /// they land.
    pub inbound: Vec<(BuildingId, Cargo, u64)>,
    /// The walk graph version beach access was last read for.
    #[serde(skip)]
    pub access_version: u64,
}

impl ShoreState {
    pub fn beach(&self, b: BuildingId) -> Option<&BeachState> {
        self.beaches.get(&b)
    }

    /// Bathers on every beach now.
    pub fn bathers(&self) -> u32 {
        self.beaches.values().map(|b| b.visitors).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beach_weather_wants_warmth_sun_and_daylight() {
        let best = beach_weather(30.0, 0.1, Spell::Normal, 14.0);
        assert!(best > 0.8, "{best}");
        assert!(beach_weather(5.0, 0.1, Spell::Normal, 14.0) < 0.01, "cold");
        assert!(beach_weather(-6.0, 0.5, Spell::Normal, 14.0) == 0.0, "arctic");
        assert!(beach_weather(30.0, 0.9, Spell::Normal, 14.0) < best * 0.2, "rain");
        assert_eq!(beach_weather(30.0, 0.1, Spell::Storm, 14.0), 0.0);
        assert!(beach_weather(30.0, 0.1, Spell::Normal, 2.0) < 0.05, "night");
        assert!(beach_weather(30.0, 0.1, Spell::Normal, 9.0) < best, "morning builds");
    }

    #[test]
    fn navigable_water_takes_boats() {
        assert!(navigable(WaterKind::Sea, f32::INFINITY));
        assert!(navigable(WaterKind::Lake, 80.0));
        assert!(navigable(WaterKind::River, 40.0));
        assert!(!navigable(WaterKind::River, 10.0));
        assert!(!navigable(WaterKind::Stream, 30.0));
    }

    #[test]
    fn beach_names_are_stable() {
        assert_eq!(beach_name(7), beach_name(7));
        assert!(beach_name(7).contains(' '));
    }
}
