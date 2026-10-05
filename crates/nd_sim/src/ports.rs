//! Docks: shipping ports and ferry terminals. Both stand on the shoreline
//! with a pier built out into the water and a street along their landward
//! side. A shipping port brings freight to and from "elsewhere" by ship,
//! like the rail yard does by train: imports are cheaper and wait at the port
//! for the next freighter, which hands them to lorries; exports earn more.
//! A ferry terminal runs a ferry to every other terminal on connected water,
//! or out to "elsewhere" when it is the only one, carrying passengers for a
//! fare. Ships are drawn moving along their water route; they are not saved
//! (the next sailing brings fresh ones).

use crate::buildings::BuildingId;
use crate::landscape::{Landscape, WaterKind};
use crate::traffic::Cargo;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct PortId;
    pub struct ShipId;
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortKind {
    ShippingPort,
    FerryTerminal,
}

impl PortKind {
    pub const ALL: [PortKind; 2] = [PortKind::ShippingPort, PortKind::FerryTerminal];

    pub fn label(self) -> &'static str {
        match self {
            PortKind::ShippingPort => "shipping port",
            PortKind::FerryTerminal => "ferry terminal",
        }
    }

    /// Footprint along the shore and back from it, in metres.
    pub fn size(self) -> (f32, f32) {
        match self {
            PortKind::ShippingPort => (96.0, 56.0),
            PortKind::FerryTerminal => (48.0, 30.0),
        }
    }

    /// Length and width of the pier.
    pub fn pier(self) -> (f32, f32) {
        match self {
            PortKind::ShippingPort => (46.0, 14.0),
            PortKind::FerryTerminal => (30.0, 6.0),
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            PortKind::ShippingPort => 45_000.0,
            PortKind::FerryTerminal => 18_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            PortKind::ShippingPort => 500.0,
            PortKind::FerryTerminal => 250.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            PortKind::ShippingPort => 30,
            PortKind::FerryTerminal => 8,
        }
    }

    /// Narrowest river it can use. Lakes and the sea are always wide enough.
    pub fn min_width(self) -> f32 {
        match self {
            PortKind::ShippingPort => 40.0,
            PortKind::FerryTerminal => 16.0,
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
    /// The street side reaches the city's roads.
    #[serde(default)]
    pub road_connected: bool,
    /// Ships can reach the berth from off the map.
    #[serde(default)]
    pub reaches_elsewhere: bool,
    /// Last tick a ship was sent here.
    #[serde(default)]
    pub last_sailing: u64,
}

impl Port {
    /// Works when the street reaches the city and ships can get to it.
    pub fn works(&self) -> bool {
        self.road_connected && (self.reaches_elsewhere || self.kind == PortKind::FerryTerminal)
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

    /// The street stub along the landward side, reaching past both ends.
    pub fn road_line(&self) -> [Vec2; 2] {
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
        let center = shore.point - seaward * (d * 0.5 + SETBACK);
        let (len, _) = kind.pier();
        let reach = if w.width.is_finite() { (w.width * 0.38).min(len) } else { len };
        let root = shore.point - seaward * 2.0;
        let tip = shore.point + seaward * reach;
        let berth = shore.point + seaward * (reach + if kind == PortKind::ShippingPort { 9.0 } else { 5.0 }).min(if w.width.is_finite() { w.width * 0.5 } else { f32::MAX });
        let port = Port { kind, center, along, seaward, pier: [root, tip], berth, water: w.kind, building: None, lot: None, road_connected: false, reaches_elsewhere: false, last_sailing: 0 };
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
}

impl Ship {
    pub fn length(&self) -> f32 {
        match self.kind {
            ShipKind::Freighter => 70.0,
            ShipKind::Ferry => 28.0,
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
}

impl Ports {
    pub fn port_of(&self, b: BuildingId) -> Option<&Port> {
        self.ports.values().find(|p| p.building == Some(b))
    }

    pub fn kind_of(&self, b: BuildingId) -> Option<PortKind> {
        self.port_of(b).map(|p| p.kind)
    }

    /// Working shipping ports' buildings.
    pub fn working(&self, kind: PortKind) -> Vec<BuildingId> {
        self.ports.values().filter(|p| p.kind == kind && p.works()).filter_map(|p| p.building).collect()
    }
}
