//! Docks: shipping ports and ferry terminals. Both stand on the shoreline
//! with a pier built out into the water and a street along their landward
//! side. A shipping port brings freight to and from "elsewhere" by ship,
//! like the rail yard does by train: imports are cheaper and wait at the port
//! for the next freighter, which hands them to lorries; exports earn more.
//! A ferry terminal runs a ferry to every other terminal on connected water,
//! or out to "elsewhere" when it is the only one, carrying passengers for a
//! fare. Ships are drawn moving along their water route; they are not saved
//! (the next sailing brings fresh ones).
//!
//! [ferries] A ferry terminal on water that reaches the edge of the map is
//! also given one or two remote terminals in towns off the map
//! (`RemoteTerminal`), across the edge its water leaves by: a terminal on a
//! west coast only ever sails west. Its ferries alternate between the
//! terminals in town and those towns, sailing out over the edge and back;
//! visitors (`crate::tourism`) and day commuters come in on the returning
//! boats, residents go out on day trips (`world/ferry.rs`). One terminal is
//! enough for all of it.

use crate::buildings::BuildingId;
use crate::landscape::{Landscape, MapEdge, WaterKind};
use crate::traffic::Cargo;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct PortId;
    pub struct ShipId;
}

/// [dock-access] Length of a dock's access road, gate to free end; it is
/// shortened (down to `STUB_MIN`) or turned where water, a building or
/// another dock is in the way.
pub const STUB_LENGTH: f32 = 32.0;
pub const STUB_MIN: f32 = 14.0;
/// [dock-access] The access road starts this far off the yard's landward edge.
pub const STUB_GATE: f32 = 3.0;
/// How far from the water's edge a click may be and still find it.
pub const SHORE_REACH: f32 = 90.0;
/// Gap between the footprint and the waterline.
pub const SETBACK: f32 = 3.0;
/// Ships per hour of clock, metres per second.
pub const FREIGHTER_SPEED: f32 = 6.0;
pub const FERRY_SPEED: f32 = 8.0;
/// Sim minutes a ship stands at the pier.
pub const FREIGHTER_DWELL: u32 = 60;
pub const FERRY_DWELL: u32 = 20;
/// Minutes between freighters calling at a port, and between ferries.
pub const FREIGHT_HEADWAY: u64 = 240;
pub const FERRY_HEADWAY: u64 = 90;
pub const FERRY_SERVICE_START: u32 = 6 * 60;
pub const FERRY_SERVICE_END: u32 = 22 * 60;
pub const FERRY_CAPACITY: u32 = 150;
pub const FERRY_FARE: f32 = 3.0;
/// Share of the import bill paid when goods come by ship.
pub const SHIP_IMPORT_FACTOR: f32 = 0.7;
/// Export income multiplier for towns with a working shipping port.
pub const SHIP_EXPORT_BONUS: f32 = 1.25;
pub const MAX_SHIPS: usize = 12;
/// [ferries] Ferries a terminal runs at once (one can be away off the map
/// while the next sails).
pub const FERRIES_PER_TERMINAL: usize = 2;
/// [ferries] How much faster a ship goes once it is beyond the map edge,
/// out of sight, so a run to the next town does not take half a day.
pub const OFF_MAP_SPEEDUP: f32 = 6.0;
/// [ferries] Minutes a ferry stands at its own pier after a run, letting
/// people off.
pub const FERRY_UNLOAD: u32 = 8;
/// [ferries] What a day commuter from over the water costs the business
/// they work a shift at (the wage leaves town with them).
pub const COMMUTER_WAGE: f32 = 50.0;
/// [ferries] Commuters a terminal brings in a morning at most.
pub const MAX_COMMUTERS: u32 = 30;
/// [ferries] Residents a terminal sends out on day trips per day at most.
pub const MAX_DAY_TRIPS: u32 = 24;
/// [ferries] How far from a terminal people live who think of a day trip,
/// and businesses are that take on commuters off the boat.
pub const FERRY_CATCHMENT: f32 = 1500.0;

/// [ferries] Towns off the map a ferry may run to, by the edge they lie
/// beyond. A terminal draws its one or two from the list for its edge.
const REMOTE_TOWNS: [(MapEdge, [&str; 4]); 4] = [
    (MapEdge::North, ["Port Harrow", "Kestrel Sound", "Northwick", "Grey Isles"]),
    (MapEdge::East, ["Eastmere", "Lantern Quay", "Sunhaven", "Cobalt Bay"]),
    (MapEdge::South, ["Saltmarsh", "Coralton", "Tidewater", "Southreach"]),
    (MapEdge::West, ["Westholm", "Driftwood", "Amberport", "Rook Harbour"]),
];

/// [ferries] A terminal in a town off the map that a ferry runs to: its
/// name, the edge of the map it lies beyond (the ferry sails out across
/// it) and how far it is past the edge, for the panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteTerminal {
    pub name: String,
    pub edge: MapEdge,
    pub km: f32,
}

/// [ferries] The remote terminals for a terminal at `berth` whose water
/// leaves the map across `edges` (nearest first): one on the nearest edge,
/// and a second where the water reaches another edge or, half the time, a
/// second town on the same one. Never an edge the water does not reach.
/// Seeded by where the berth is, so the same dock always gets the same.
pub fn remote_terminals(berth: Vec2, edges: &[MapEdge]) -> Vec<RemoteTerminal> {
    use rand::{Rng, SeedableRng};
    let Some(&first) = edges.first() else { return Vec::new() };
    let seed = ((berth.x.round() as i64 as u64) << 32) ^ (berth.y.round() as i64 as u64) ^ 0xFE44_7000;
    let mut rng = crate::SimRng::seed_from_u64(seed);
    let town = |edge: MapEdge, k: usize| REMOTE_TOWNS.iter().find(|t| t.0 == edge).map_or("Elsewhere", |t| t.1[k % 4]).to_string();
    let k0 = rng.random_range(0..4usize);
    let mut out = vec![RemoteTerminal { name: town(first, k0), edge: first, km: rng.random_range(8.0..30.0f32).round() }];
    if let Some(&second) = edges.get(1) {
        out.push(RemoteTerminal { name: town(second, rng.random_range(0..4usize)), edge: second, km: rng.random_range(8.0..30.0f32).round() });
    } else if rng.random_bool(0.5) {
        out.push(RemoteTerminal { name: town(first, k0 + 1 + rng.random_range(0..3usize)), edge: first, km: rng.random_range(12.0..40.0f32).round() });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortKind {
    ShippingPort,
    FerryTerminal,
    // -- [economy] waterfront businesses on a dock the player places
    // (`world/waterfront.rs`). Appended so saved kinds keep their order. --
    /// A recreational marina: pontoons of small boats.
    MarinaSmall,
    /// A large marina: yacht berths and a clubhouse.
    MarinaLarge,
    /// A small fishing port: a quay and a few trawlers.
    FishingPortSmall,
    /// A large fishing port: a long quay, a crane, an ice house and a fleet.
    FishingPortLarge,
}

impl PortKind {
    pub const ALL: [PortKind; 6] = [PortKind::ShippingPort, PortKind::FerryTerminal, PortKind::MarinaSmall, PortKind::MarinaLarge, PortKind::FishingPortSmall, PortKind::FishingPortLarge];

    pub fn label(self) -> &'static str {
        match self {
            PortKind::ShippingPort => "shipping port",
            PortKind::FerryTerminal => "ferry terminal",
            PortKind::MarinaSmall => "small marina",
            PortKind::MarinaLarge => "large marina",
            PortKind::FishingPortSmall => "small fishing port",
            PortKind::FishingPortLarge => "large fishing port",
        }
    }

    /// [economy] A recreational marina, small or large.
    pub fn is_marina(self) -> bool {
        matches!(self, PortKind::MarinaSmall | PortKind::MarinaLarge)
    }

    /// [economy] A fishing port, small or large.
    pub fn is_fishing(self) -> bool {
        matches!(self, PortKind::FishingPortSmall | PortKind::FishingPortLarge)
    }

    /// [economy] The large version of a marina or fishing port.
    pub fn is_large(self) -> bool {
        matches!(self, PortKind::MarinaLarge | PortKind::FishingPortLarge)
    }

    /// [economy] The business that runs a waterfront dock: the city builds
    /// the quay, an operator runs it on its takings (`world/waterfront.rs`).
    pub fn business(self) -> Option<crate::market::BusinessKind> {
        match self {
            PortKind::MarinaSmall | PortKind::MarinaLarge => Some(crate::market::BusinessKind::Marina),
            PortKind::FishingPortSmall | PortKind::FishingPortLarge => Some(crate::market::BusinessKind::FishingPort),
            _ => None,
        }
    }

    /// [economy] Boats moored at the pontoons or quay: drawn at their
    /// berths, a few of them out on the water at a time.
    pub fn berths(self) -> u32 {
        match self {
            PortKind::MarinaSmall => 10,
            PortKind::MarinaLarge => 22,
            PortKind::FishingPortSmall => 4,
            PortKind::FishingPortLarge => 9,
            _ => 0,
        }
    }

    /// [economy] Land use of the dock's lot.
    pub fn land_use(self) -> crate::zoning::LandUse {
        match self {
            PortKind::ShippingPort | PortKind::FishingPortSmall | PortKind::FishingPortLarge => crate::zoning::LandUse::Industrial,
            PortKind::MarinaSmall | PortKind::MarinaLarge => crate::zoning::LandUse::Commercial,
            PortKind::FerryTerminal => crate::zoning::LandUse::Civic,
        }
    }

    /// Footprint along the shore and back from it, in metres.
    pub fn size(self) -> (f32, f32) {
        match self {
            PortKind::ShippingPort => (96.0, 56.0),
            PortKind::FerryTerminal => (48.0, 30.0),
            PortKind::MarinaSmall => (44.0, 30.0),
            PortKind::MarinaLarge => (84.0, 46.0),
            PortKind::FishingPortSmall => (46.0, 32.0),
            PortKind::FishingPortLarge => (84.0, 50.0),
        }
    }

    /// Length and width of the pier.
    pub fn pier(self) -> (f32, f32) {
        match self {
            PortKind::ShippingPort => (46.0, 14.0),
            PortKind::FerryTerminal => (30.0, 6.0),
            // A marina's main pontoon; the finger berths hang off it.
            PortKind::MarinaSmall => (26.0, 3.0),
            PortKind::MarinaLarge => (42.0, 4.0),
            PortKind::FishingPortSmall => (24.0, 6.0),
            PortKind::FishingPortLarge => (38.0, 9.0),
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            PortKind::ShippingPort => 45_000.0,
            PortKind::FerryTerminal => 18_000.0,
            PortKind::MarinaSmall => 9_000.0,
            PortKind::MarinaLarge => 26_000.0,
            PortKind::FishingPortSmall => 8_000.0,
            PortKind::FishingPortLarge => 22_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            PortKind::ShippingPort => 500.0,
            PortKind::FerryTerminal => 250.0,
            // The city keeps up the breakwater and the quay; the operator
            // pays its own staff.
            PortKind::MarinaSmall | PortKind::FishingPortSmall => 40.0,
            PortKind::MarinaLarge | PortKind::FishingPortLarge => 90.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            PortKind::ShippingPort => 30,
            PortKind::FerryTerminal => 8,
            PortKind::MarinaSmall => 3,
            PortKind::MarinaLarge => 9,
            PortKind::FishingPortSmall => 10,
            PortKind::FishingPortLarge => 24,
        }
    }

    /// Narrowest river it can use. Lakes and the sea are always wide enough.
    pub fn min_width(self) -> f32 {
        match self {
            PortKind::ShippingPort => 40.0,
            PortKind::FerryTerminal => 16.0,
            PortKind::MarinaSmall | PortKind::FishingPortSmall => 18.0,
            PortKind::MarinaLarge | PortKind::FishingPortLarge => 26.0,
        }
    }
}

/// A dock: a footprint on the shore, a street along its landward edge, a
/// pier into the water and the berth where ships tie up.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port {
    pub kind: PortKind,
    pub center: Vec2,
    /// Unit vector along the shore.
    pub along: Vec2,
    /// Unit vector from the land into the water.
    pub seaward: Vec2,
    /// The pier: root on the shore, tip out in the water.
    pub pier: [Vec2; 2],
    pub berth: Vec2,
    pub water: WaterKind,
    pub building: Option<BuildingId>,
    #[serde(default)]
    pub lot: Option<LotId>,
    /// The street side reaches the city's roads (streets alone: a lorry
    /// can get there).
    #[serde(default)]
    pub road_connected: bool,
    /// [sweep-sim] People can walk to it from the city: the street side
    /// reaches it over streets or paths. A ferry terminal needs no more.
    #[serde(default)]
    pub walk_connected: bool,
    /// Ships can reach the berth from off the map.
    #[serde(default)]
    pub reaches_elsewhere: bool,
    /// Last tick a ship was sent here.
    #[serde(default)]
    pub last_sailing: u64,
    /// [ferries] Terminals in towns off the map this ferry terminal runs
    /// to (one or two; none when its water does not reach the edge).
    /// Given when the terminal is built, or on the first refresh of an
    /// older save.
    #[serde(default)]
    pub remote: Vec<RemoteTerminal>,
    /// [ferries] Sailings made, to take turns between destinations.
    #[serde(default)]
    pub sailings: u32,
    /// [dock-access] The access road laid with the dock: from its gate
    /// (`[0]`) inland to a free end (`[1]`) that roads are drawn onto to
    /// join it to the network. None when the dock fronted a street already
    /// or came from a save that laid the old street along its landward side.
    #[serde(default)]
    pub stub: Option<[Vec2; 2]>,
    /// [pt-visual] A marina's moorings as the last midnight let them: berths
    /// taken by the town's boat owners, and billionaires' superyachts
    /// lying at its outer pontoon (`World::waterfront_daily`). `None` until
    /// the first count (an older save): every berth drawn taken.
    #[serde(default)]
    pub moored: Option<Moored>,
}

/// [pt-visual] See [`Port::moored`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Moored {
    pub berths_let: u16,
    pub superyachts: u8,
}

/// [pt-visual] Superyachts one large marina takes at its outer pontoon.
pub const MAX_SUPERYACHTS: u8 = 3;

impl Port {
    /// [dock-access] Ships sail from it: a ferry terminal runs its boats on
    /// the water alone (carrying no one until people can reach it); a
    /// port, marina or fishing port needs the roads for what it lands.
    pub fn sails(&self) -> bool {
        match self.kind {
            PortKind::FerryTerminal => true,
            _ => self.works(),
        }
    }

    /// Works when the street reaches the city and ships can get to it.
    /// [sweep-sim] A ferry terminal carries people, so a path from the city
    /// to its gate is enough; a dock that lands goods needs a road.
    pub fn works(&self) -> bool {
        // [economy] Marinas and fishing ports work any water they are on.
        let reached = self.road_connected || (self.kind == PortKind::FerryTerminal && self.walk_connected);
        reached && (self.reaches_elsewhere || self.kind != PortKind::ShippingPort)
    }

    /// [sweep-sim] Working, but reached on foot only (no road yet).
    pub fn walk_only(&self) -> bool {
        self.works() && !self.road_connected
    }

    /// Towards the water, like `Facility::inward`.
    pub fn inward(&self) -> Vec2 {
        self.seaward
    }

    /// Front-left, front-right, back-right, back-left; the front is the
    /// street side, the back the water.
    pub fn corners(&self) -> [Vec2; 4] {
        let (w, d) = self.kind.size();
        let a = self.along;
        let n = self.seaward;
        let c = self.center;
        [c - a * (w * 0.5) - n * (d * 0.5), c + a * (w * 0.5) - n * (d * 0.5), c + a * (w * 0.5) + n * (d * 0.5), c - a * (w * 0.5) + n * (d * 0.5)]
    }

    pub fn footprint(&self) -> Vec<Vec2> {
        self.corners().to_vec()
    }

    pub fn front_mid(&self) -> Vec2 {
        self.center - self.seaward * (self.kind.size().1 * 0.5)
    }

    /// [dock-access] The dock's street stub: its access road from the gate
    /// inland (`[1]` the free end), or for a dock from an older save the
    /// street along the landward side, reaching past both ends.
    pub fn road_line(&self) -> [Vec2; 2] {
        if let Some(stub) = self.stub {
            return stub;
        }
        self.front_line()
    }

    /// The street along the landward side, reaching past both ends (what a
    /// dock laid before it had an access road of its own).
    pub fn front_line(&self) -> [Vec2; 2] {
        let (w, _) = self.kind.size();
        let mid = self.front_mid() - self.seaward * crate::rail::ROAD_GAP;
        [mid - self.along * (w * 0.5 + crate::rail::STUB_LEAD), mid + self.along * (w * 0.5 + crate::rail::STUB_LEAD)]
    }

    /// Where a dock of `kind` would go for a click at `p`: on the nearest
    /// shore within reach, square to it, set back from the waterline, with
    /// its pier. `Err` says why not.
    pub fn plan(kind: PortKind, p: Vec2, land: &Landscape) -> Result<Port, String> {
        let shore = land.nearest_shore(p, SHORE_REACH).ok_or_else(|| format!("a {} must be built at the water's edge", kind.label()))?;
        let w = shore.water;
        match (kind, w.kind) {
            (PortKind::ShippingPort, WaterKind::Lake) => return Err("ships cannot reach a lake: build the port on a river or the sea".into()),
            (PortKind::ShippingPort, WaterKind::Stream) => return Err("a stream is too narrow for freighters".into()),
            _ => {}
        }
        if matches!(w.kind, WaterKind::River | WaterKind::Stream) && w.width < kind.min_width() {
            return Err(format!("the {} is {:.0} m wide; a {} needs {:.0} m", w.kind.label(), w.width, kind.label(), kind.min_width()));
        }
        let seaward = shore.seaward;
        let along = Vec2::new(seaward.y, -seaward.x);
        let (_, d) = kind.size();
        // Set back far enough that the whole yard is on dry land where the
        // bank curves away under its corners.
        let mut center = shore.point - seaward * (d * 0.5 + SETBACK);
        for k in 0..12 {
            let c = shore.point - seaward * (d * 0.5 + SETBACK + k as f32 * 4.0);
            let (w2, d2) = (kind.size().0 * 0.5, d * 0.5);
            let corners = [c - along * w2 - seaward * d2, c + along * w2 - seaward * d2, c + along * w2 + seaward * d2, c - along * w2 + seaward * d2];
            center = c;
            if !corners.iter().any(|q| land.is_water(*q)) {
                break;
            }
        }
        let back = (shore.point - center).dot(seaward) - d * 0.5;
        let (len, _) = kind.pier();
        let reach = if w.width.is_finite() { (w.width * 0.38).min(len) } else { len };
        let root = shore.point - seaward * (back + 1.0);
        let tip = shore.point + seaward * reach;
        let berth = shore.point + seaward * (reach + if matches!(kind, PortKind::ShippingPort | PortKind::FishingPortLarge) { 9.0 } else { 5.0 }).min(if w.width.is_finite() { w.width * 0.5 } else { f32::MAX });
        let port = Port { kind, center, along, seaward, pier: [root, tip], berth, water: w.kind, building: None, lot: None, road_connected: false, walk_connected: false, reaches_elsewhere: false, last_sailing: 0, remote: Vec::new(), sailings: 0, stub: None, moored: None };
        for c in port.corners().iter().chain(std::iter::once(&center)) {
            if land.is_water(*c) {
                return Err("the shore is too ragged here: part of the dock would stand in the water".into());
            }
        }
        if land.slope_over(&port.footprint()) > crate::landscape::MAX_LOT_SLOPE {
            return Err("the bank is too steep here".into());
        }
        if !land.is_water(tip) {
            return Err("the pier would not reach open water".into());
        }
        Ok(port)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShipKind {
    Freighter,
    Ferry,
    /// [economy] A trawler out of a fishing port.
    FishingBoat,
    /// [economy] A motor yacht out of a large marina.
    Yacht,
    /// [economy] A small sailing or motor boat out of a marina.
    Sailboat,
}

/// A ship under way: along `path`, standing `dwell` minutes at each end
/// that is a berth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ship {
    pub kind: ShipKind,
    pub path: Vec<Vec2>,
    /// Distance along the path now and last tick.
    pub s: f32,
    pub prev_s: f32,
    /// Metres per tick.
    pub speed: f32,
    /// Minutes left standing at the current berth.
    pub dwell: u32,
    /// The port it calls at (freighter) or sails from (ferry).
    pub port: BuildingId,
    /// The port at the far end, for a ferry between two terminals.
    pub to: Option<BuildingId>,
    /// Ships for the berth that are waiting on board.
    pub cargo: Vec<Cargo>,
    pub passengers: u32,
    /// Docked at the berth halfway through a there-and-back sailing.
    pub docked: bool,
    /// Index into the path where the berth is.
    pub berth_s: f32,
    pub color: u8,
    /// [ferries] A run to the remote terminal with this index in the
    /// port's `remote`: out over the edge and back to its own pier.
    #[serde(default)]
    pub remote: Option<u8>,
    /// [ferries] Back at its own pier after the run, letting people off.
    #[serde(default)]
    pub home: bool,
}

impl Ship {
    pub fn length(&self) -> f32 {
        match self.kind {
            ShipKind::Freighter => 70.0,
            ShipKind::Ferry => 28.0,
            ShipKind::FishingBoat => 16.0,
            ShipKind::Yacht => 18.0,
            ShipKind::Sailboat => 9.0,
        }
    }

    /// Position and heading at distance `s`.
    pub fn pose_at(&self, s: f32) -> (Vec2, Vec2) {
        crate::geom::point_along(&self.path, s)
    }

    pub fn pose(&self) -> (Vec2, Vec2) {
        self.pose_at(self.s)
    }
}

/// Port figures for the panel and the report.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortStats {
    pub ports: u32,
    pub terminals: u32,
    pub ships_out: u32,
    pub freight_landed_today: u32,
    pub ferry_riders_today: u32,
    pub ferry_fares_today: f32,
    /// Goods waiting at ports for the next freighter.
    pub waiting_cargo: u32,
    /// [ferries] Runs to towns off the map today, and the people they
    /// brought in and took out: visitors, commuters, residents on day trips.
    #[serde(default)]
    pub remote_sailings_today: u32,
    #[serde(default)]
    pub visitors_in_today: u32,
    #[serde(default)]
    pub visitors_out_today: u32,
    #[serde(default)]
    pub commuters_in_today: u32,
    #[serde(default)]
    pub commuters_out_today: u32,
    #[serde(default)]
    pub day_trips_today: u32,
    #[serde(default)]
    pub day_trippers_back_today: u32,
}

/// Every dock and the ships on the water.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ports {
    pub ports: slotmap::SlotMap<PortId, Port>,
    #[serde(skip)]
    pub ships: slotmap::SlotMap<ShipId, Ship>,
    /// Imports waiting at a port for the next freighter: (port building, load).
    #[serde(default)]
    pub inbound: Vec<(BuildingId, Cargo)>,
    #[serde(default)]
    pub stats: PortStats,
    /// [ferries] People waiting at a terminal for the next boat out of
    /// town (visitors going home, commuters after their shift): terminal →
    /// count, taken aboard by the next run to a town off the map.
    #[serde(default)]
    pub boarding: std::collections::BTreeMap<BuildingId, u32>,
    /// [review] Of `boarding`, the day-trippers who paid both legs at the
    /// terminal: carried without a fare.
    #[serde(default)]
    pub prepaid: std::collections::BTreeMap<BuildingId, u32>,
    /// [ferries] Residents on their way to a terminal for a day trip.
    /// On arrival they board.
    #[serde(default)]
    pub trippers: Vec<DayTrip>,
    /// [ferries] Per terminal today: commuters brought in and day trips
    /// set off, against the daily caps.
    #[serde(default)]
    pub commuters_today: std::collections::BTreeMap<BuildingId, u32>,
    #[serde(default)]
    pub trips_today: std::collections::BTreeMap<BuildingId, u32>,
    /// [ferry-routes] People carried today on each ferry route, by the
    /// terminal a run sailed from and where it went (`world/ferry_routes.rs`).
    #[serde(default)]
    pub line_riders: Vec<LineRiders>,
}

/// [ferry-routes] Riders today on the runs from `from` to another
/// terminal (`to`) or to a town off the map (`town`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineRiders {
    pub from: BuildingId,
    pub to: Option<BuildingId>,
    pub town: Option<u8>,
    pub riders: u32,
}

/// [ferries] A resident heading for a terminal to take the ferry to a
/// town off the map for the day (`town`: index into the port's `remote`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DayTrip {
    pub citizen: crate::citizens::CitizenId,
    pub terminal: BuildingId,
    pub town: u8,
    /// Tick they set off; a trip that has not reached the pier in four
    /// hours is forgotten.
    pub since: u64,
}

impl Ports {
    /// [ferry-routes] Count `n` riders on a run from `from` to `to` (a
    /// terminal) or `town` (off the map).
    pub fn count_line_riders(&mut self, from: BuildingId, to: Option<BuildingId>, town: Option<u8>, n: u32) {
        match self.line_riders.iter_mut().find(|l| l.from == from && l.to == to && l.town == town) {
            Some(l) => l.riders += n,
            None => self.line_riders.push(LineRiders { from, to, town, riders: n }),
        }
    }

    pub fn port_of(&self, b: BuildingId) -> Option<&Port> {
        self.ports.values().find(|p| p.building == Some(b))
    }

    pub fn kind_of(&self, b: BuildingId) -> Option<PortKind> {
        self.port_of(b).map(|p| p.kind)
    }

    /// [dock-access] Ferry terminals whose boats run (on the water alone).
    pub fn sailing(&self, kind: PortKind) -> Vec<BuildingId> {
        self.ports.values().filter(|p| p.kind == kind && p.sails()).filter_map(|p| p.building).collect()
    }

    /// Working shipping ports' buildings.
    pub fn working(&self, kind: PortKind) -> Vec<BuildingId> {
        self.ports.values().filter(|p| p.kind == kind && p.works()).filter_map(|p| p.building).collect()
    }

    /// [ferries] Working ferry terminals with a ferry to a town off the
    /// map: the way in for visitors and commuters by water.
    pub fn remote_terminals(&self) -> Vec<BuildingId> {
        self.ports.values().filter(|p| p.kind == PortKind::FerryTerminal && p.works() && !p.remote.is_empty()).filter_map(|p| p.building).collect()
    }
}
