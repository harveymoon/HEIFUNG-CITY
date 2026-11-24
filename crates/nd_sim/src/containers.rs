//! [K34] Shipping containers as the boxes they are: each one a record with
//! its own number, line, origin, age and wear, and a short history of
//! where it has been (systems in `world/containers.rs`).
//!
//! A box comes into town full: on a freight train, off a freighter at the
//! port, or on a semi from out of town. It is set down at a freight yard,
//! the port or a cargo hub, then trucked to the business its goods are for
//! (a hub unpacks it and sends the goods on in vans). Emptied, it is the
//! owner's: its shipping line, or a container yard that has bought it. The
//! owner stores it, sends it back out of town (on an outgoing train or
//! ship, or by road), sells it to a stack developer or an out-of-town
//! buyer, or has it cut up at the scrap yard, whose steel is sold on. A
//! stack developer building a Stacks home buys boxes off the lorry that
//! has just emptied one, or from a yard; the home is drawn from those very
//! boxes. Records cost a hundred bytes or so each; nothing is done per
//! tick for a box standing still (an hourly and a daily pass, and the
//! moments a train, ship or lorry moves one).

use crate::buildings::BuildingId;
use crate::hash::{mix, unit};
use crate::market::Resource;
use crate::rail::BoxKind;
use crate::traffic::CarId;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::BTreeMap;

new_key_type! {
    /// One shipping container.
    pub struct BoxId;
}

/// A shipping line (all fictional) and its owner code (the first three
/// letters of every box number it owns; the fourth, `U`, says freight
/// container). In the order of `nd_procgen::SHIPPING_LINES`, whose
/// colours they wear.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub name: &'static str,
    pub code: &'static str,
}

pub const LINES: [Line; 10] = [
    Line { name: "Kestrel Ocean Lines", code: "KOL" },
    Line { name: "Ardent Maritime", code: "ADM" },
    Line { name: "Tidewell Freight", code: "TWF" },
    Line { name: "Hanzo Kaiun", code: "HZK" },
    Line { name: "Meridian Box Co.", code: "MBX" },
    Line { name: "Corvid Logistics", code: "CVD" },
    Line { name: "Sable Line", code: "SBL" },
    Line { name: "Lumen Pacific", code: "LMP" },
    Line { name: "Oro Leasing", code: "ORO" },
    Line { name: "Brine & Basalt", code: "BNB" },
];

/// The ports boxes start their trips from (real ports; the lines and
/// companies are made up).
pub const PORTS: [&str; 28] = [
    "Shanghai", "Singapore", "Ningbo", "Shenzhen", "Busan", "Hong Kong", "Rotterdam", "Jebel Ali", "Port Klang", "Antwerp", "Kaohsiung", "Hamburg", "Los Angeles", "Tanjung Pelepas", "Laem Chabang", "Colombo", "Nhava Sheva", "Santos", "Piraeus", "Valencia", "Tokyo", "Felixstowe", "Durban", "Lagos", "Melbourne", "Vancouver", "Callao", "Algeciras",
];

/// History entries a box keeps (the oldest drop off).
pub const LOG_LEN: usize = 12;
/// Goods (units) in a twenty-foot box: a load takes as many boxes as this
/// many units go into (as the freight trains box them, `rail::TEU_UNITS`).
pub const BOX_UNITS: f32 = crate::rail::TEU_UNITS;
/// The most boxes one load comes in.
pub const MAX_BOXES_PER_LOAD: usize = 4;
/// Wear from which a box is fit only for scrap, and the age (years) at
/// which its owner scraps it anyway.
pub const SCRAP_WEAR: f32 = 0.78;
pub const SCRAP_AGE: f32 = 20.0;
/// Steel (units of `Resource::Metal`) cut from a twenty- and a forty-foot box.
pub const STEEL_20: f32 = 2.2;
pub const STEEL_40: f32 = 3.8;
/// What a sound twenty-foot box sells for (a forty-footer half as much
/// again), scaled down by its wear; scrap pays per unit of steel.
pub const BOX_PRICE: f32 = 1800.0;
pub const SCRAP_PRICE_PER_STEEL: f32 = 60.0;
/// Share of low-rent home sites (in the districts that build them) that a
/// stack developer takes on: built from boxes.
pub const BOX_HOME_SHARE: f32 = 0.25;
/// How far a stack developer looks for a box (metres): off a lorry, or at
/// a yard.
pub const DEVELOPER_REACH: f32 = 4000.0;
/// Boxes a stack developer has trucked over from a yard each hour, per site.
pub const HAULS_PER_HOUR: usize = 2;
/// Share of a depot's room its owners keep stocked: a stack developer buys
/// from a yard only what is over it.
pub const DEPOT_RESERVE: f32 = 0.4;
/// Days an old box waits for a scrap yard in town before it is sold for
/// scrap out of town.
pub const SCRAP_WAIT_DAYS: u64 = 3;
/// Old boxes a day that keep one scrap yard busy (the pressure).
pub const BOXES_PER_SCRAP_YARD: f32 = 12.0;
/// Days a line leaves an empty with a container yard before the yard buys
/// the older ones off it.
pub const YARD_BUYS_AFTER_DAYS: u64 = 8;

/// ISO 6346 letter values: A is 10, and the multiples of 11 are skipped.
fn letter_value(c: u8) -> u32 {
    let mut v = 10u32;
    for l in b'A'..c {
        let _ = l;
        v += 1;
        if v % 11 == 0 {
            v += 1;
        }
    }
    v
}

/// [K34] The ISO 6346 check digit of a box number: owner code (three
/// letters), category letter and six-digit serial. Each of the ten
/// characters' values (digits as they are, letters from 10 skipping 11, 22
/// and 33) weighted by 2^position, summed, mod 11, mod 10.
pub fn check_digit(prefix: &[u8; 4], serial: u32) -> u8 {
    let digits = format!("{:06}", serial % 1_000_000);
    let mut sum = 0u32;
    for (i, c) in prefix.iter().chain(digits.as_bytes()).enumerate() {
        let v = if c.is_ascii_digit() { (c - b'0') as u32 } else { letter_value(c.to_ascii_uppercase()) };
        sum += v << i;
    }
    ((sum % 11) % 10) as u8
}

/// The eleven characters of a box number: `KOLU4829137`.
pub fn number_chars(line: u8, serial: u32) -> [u8; 11] {
    let code = LINES[line as usize % LINES.len()].code.as_bytes();
    let prefix = [code[0], code[1], code[2], b'U'];
    let mut out = [b'0'; 11];
    out[..4].copy_from_slice(&prefix);
    let digits = format!("{:06}", serial % 1_000_000);
    out[4..10].copy_from_slice(digits.as_bytes());
    out[10] = b'0' + check_digit(&prefix, serial);
    out
}

/// The number as printed: `KOLU 482913 7`.
pub fn number_text(line: u8, serial: u32) -> String {
    let n = number_chars(line, serial);
    let s = |r: std::ops::Range<usize>| String::from_utf8_lossy(&n[r]).into_owned();
    format!("{} {} {}", s(0..4), s(4..10), s(10..11))
}

/// [K34] Who a box belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Owner {
    /// Its shipping line (out of town).
    #[default]
    Line,
    /// A container yard in town that bought it off the line.
    Yard(BuildingId),
    /// A stack developer, for the home it is building (and then the home).
    Home(BuildingId),
    /// A scrap yard, to cut up.
    Scrap(BuildingId),
    /// [r8] A freight forwarder in town that leased it to fill with the
    /// town's exports.
    Forwarder(BuildingId),
}

/// [K34] Where a box is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Where {
    /// On a freight train (coming in, or going out as an export).
    Train,
    /// On a lorry on the road.
    Truck(CarId),
    /// Stacked at a freight yard, the port, a cargo hub or a container yard.
    Stored(BuildingId),
    /// On a stack developer's building site.
    Site(BuildingId),
    /// Part of a Stacks home.
    Home(BuildingId),
    /// At the scrap yard, waiting to be cut up.
    Scrap(BuildingId),
    /// [r8] Hanging from a crane or a straddle carrier at a depot (the
    /// port's quay crane, a yard's carrier), between the stack and a ship,
    /// a lorry's chassis or a wagon.
    Lifting(BuildingId),
    /// [r8] Aboard a freighter (coming in, or going out).
    Ship(crate::ports::ShipId),
}

impl Where {
    /// [r10-port-boxes] The depot whose stack the box stands in (stored, or
    /// on a scrap yard's pile).
    pub fn stack(self) -> Option<BuildingId> {
        match self {
            Where::Stored(b) | Where::Scrap(b) => Some(b),
            _ => None,
        }
    }
}

/// [K34] What a box is full of, and who it is for (a null buyer: whoever
/// at its yard ordered that).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Goods {
    pub resource: Resource,
    pub buyer: BuildingId,
    /// [r14-materials] How much of it is in the box, in its unit (0 in
    /// older saves: not known).
    #[serde(default)]
    pub amount: f32,
}

impl Goods {
    /// [r14-materials] "14 t bricks and blocks": what and how much.
    pub fn describe(&self) -> String {
        let row = self.resource.row();
        if self.amount > 0.0 {
            format!("{:.0} {} {}", self.amount.max(1.0), row.unit.label(), row.label)
        } else {
            row.label.to_string()
        }
    }
}

/// [r14-materials] Units a full box holds (a forty holds two TEU).
pub fn full_load(forty: bool) -> f32 {
    BOX_UNITS * if forty { 2.0 } else { 1.0 }
}

/// [K34] Things that happen to a box, for its history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// Came in by rail, by sea or by road (place: where it was set down).
    ArrivedRail,
    ArrivedSea,
    ArrivedRoad,
    /// Lifted off at a yard or the port, full.
    Unloaded,
    /// Put on a lorry (place: where to).
    OnLorry,
    /// Its goods delivered (place: the business).
    Delivered,
    /// Unpacked at a cargo hub.
    Unpacked,
    /// Set down empty in storage.
    Stored,
    /// Bought off the lorry by a stack developer (place: the home site).
    BoughtOffTruck,
    /// Sold by a yard or its line to a stack developer (place: the site).
    SoldToDeveloper,
    /// Built into a home.
    BuiltIntoHome,
    /// Bought out of town by a stack developer short of boxes.
    BoughtOutOfTown,
    /// Bought off its line by a container yard.
    BoughtByYard,
    /// Sent back out of town empty (by rail, sea or road).
    SentOut,
    /// Sold to a buyer out of town.
    SoldOutOfTown,
    /// Taken to the scrap yard.
    ToScrap,
    /// Cut up for steel.
    CutUp,
    /// Already in town when the records began (an older save).
    Counted,
    /// Salvaged from a home pulled down.
    Salvaged,
    /// [r8] Lifted onto a lorry's chassis by the yard's carrier.
    CraneOn,
    /// [r8] Lifted off a lorry's chassis into the stack.
    CraneOff,
    /// [r8] Craned aboard a freighter at the quay.
    LoadedOnShip,
    /// [r8] Leased by a freight forwarder (place: the forwarder).
    Leased,
    /// [r8] A firm's export load packed in (place: the firm).
    Packed,
    /// [r8] Left town full of the town's exports.
    Exported,
    /// [r8] Sold for scrap out of town (no scrap yard in town to take it).
    SoldForScrap,
}

impl Event {
    pub fn label(self) -> &'static str {
        match self {
            Event::ArrivedRail => "arrived by rail",
            Event::ArrivedSea => "landed off a freighter",
            Event::ArrivedRoad => "came in by road",
            Event::Unloaded => "lifted off",
            Event::OnLorry => "on a lorry",
            Event::Delivered => "goods delivered",
            Event::Unpacked => "unpacked",
            Event::Stored => "stored empty",
            Event::BoughtOffTruck => "bought off the lorry by a stack developer",
            Event::SoldToDeveloper => "sold to a stack developer",
            Event::BuiltIntoHome => "built into a home",
            Event::BoughtOutOfTown => "bought out of town by a stack developer",
            Event::BoughtByYard => "bought off its line by a container yard",
            Event::SentOut => "sent back out of town",
            Event::SoldOutOfTown => "sold out of town",
            Event::ToScrap => "taken for scrap",
            Event::CutUp => "cut up for steel",
            Event::Counted => "in town when the records began",
            Event::Salvaged => "salvaged from a home pulled down",
            Event::CraneOn => "lifted onto a lorry's chassis",
            Event::CraneOff => "lifted off a lorry into the stack",
            Event::LoadedOnShip => "craned aboard a freighter",
            Event::Leased => "leased by a freight forwarder",
            Event::Packed => "export goods packed in",
            Event::Exported => "left town full of exports",
            Event::SoldForScrap => "sold for scrap out of town",
        }
    }
}

/// [K34] One entry of a box's history: when, what and where.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop(pub u64, pub Event, pub BuildingId);

/// [K34] One shipping container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxRec {
    /// Its line (index into [`LINES`]): the owner code and the livery.
    pub line: u8,
    pub serial: u32,
    pub forty: bool,
    pub kind: BoxKind,
    /// Where its trip began (index into [`PORTS`]).
    pub origin: u8,
    /// The day it was built (the city's first day is 0; before, negative).
    pub built: i32,
    /// Knocks taken: every lift, trip and season wears it a little.
    pub knocks: u16,
    pub owner: Owner,
    pub at: Where,
    #[serde(default)]
    pub goods: Option<Goods>,
    /// Tick it got where it is.
    pub since: u64,
    /// Times it has come through town.
    #[serde(default)]
    pub visits: u16,
    #[serde(default)]
    pub log: Vec<Stop>,
    /// [r8] Export loads packed in by a freight forwarder, firm by firm:
    /// the box is full of the town's exports, bound out of town.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export: Vec<ExportLoad>,
    /// [r8] An empty its owners want gone from town (the surplus over a
    /// depot's room): the next ship or train calling takes it, or a lorry
    /// out by road.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bound_out: bool,
    /// [r10-port-boxes] Its place in its depot's stack while it stands
    /// there, in twenty-foot cells (see [`Containers::seat`]): a
    /// forty-footer takes an even cell and the one after it. The renderer
    /// stands it in that place for as long as it stays, so no two boxes
    /// share one and nothing moves when another box leaves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u16>,
    /// [r11-box-stock] Kept at a container yard for its line, on a storage
    /// contract, as the overflow of this terminal (the port, a freight yard,
    /// a cargo hub): the line pays the yard [`STORAGE_FEE`] a day for it
    /// until the terminal calls it back or the line sends it home. Set as
    /// it is trucked over; gone once it stands anywhere but a yard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_for: Option<BuildingId>,
}

/// [r8] One firm's export goods in a forwarder's box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportLoad {
    pub firm: BuildingId,
    pub resource: Resource,
    pub amount: f32,
}

/// [K34] The marks of a box's life on its paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    /// Panels patched in not-quite-matching paint (0..=3).
    pub patches: u8,
    /// Dents (0..=3).
    pub dents: u8,
    /// The number scuffed half away.
    pub scuffed: bool,
    /// The number painted over and stencilled again on a fresh patch.
    pub repainted: bool,
}

/// [K34] What the renderer needs to draw a box as it is: its line, build,
/// wear and marks, its number, and a seed for the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxFace {
    pub id: BoxId,
    pub line: u8,
    pub forty: bool,
    pub kind: BoxKind,
    pub wear: f32,
    pub marks: Marks,
    pub number: [u8; 11],
    pub seed: u64,
}

impl BoxRec {
    pub fn number(&self) -> String {
        number_text(self.line, self.serial)
    }

    pub fn seed(&self) -> u64 {
        mix(self.serial as u64, 0xB0C5 ^ self.line as u64)
    }

    /// Its age in years on `day`.
    pub fn age_years(&self, day: i64) -> f32 {
        (day - self.built as i64).max(0) as f32 / 365.0
    }

    /// How worn it is, 0 (new) to 1 (fit for nothing but scrap): the
    /// years, the knocks, and its own luck.
    pub fn wear(&self, day: i64) -> f32 {
        let luck = unit(mix(self.seed(), 1)) * 0.14;
        (self.age_years(day) / 24.0 * 0.78 + self.knocks as f32 * 0.0035 + luck).clamp(0.0, 1.0)
    }

    pub fn condition_label(&self, day: i64) -> &'static str {
        match self.wear(day) {
            w if w < 0.15 => "as new",
            w if w < 0.35 => "good",
            w if w < 0.55 => "fair, faded",
            w if w < SCRAP_WEAR => "worn, rusting",
            _ => "fit for scrap",
        }
    }

    pub fn marks(&self, day: i64) -> Marks {
        let w = self.wear(day);
        let h = self.seed();
        let patches = if w < 0.2 { 0 } else { ((w * 3.2 + unit(mix(h, 2)) * 1.2) as u8).min(3) };
        let dents = ((w * 3.6 + unit(mix(h, 3)) * 1.1) as u8).min(3);
        let scuffed = w > 0.45 && unit(mix(h, 4)) < w * 0.8;
        // A box resold or long in service has its number painted over and
        // stencilled again more often than not.
        let resold = matches!(self.owner, Owner::Yard(_) | Owner::Home(_));
        let repainted = (resold || self.age_years(day) > 9.0) && unit(mix(h, 5)) < 0.35;
        Marks { patches, dents, scuffed, repainted }
    }

    pub fn face(&self, id: BoxId, day: i64) -> BoxFace {
        BoxFace { id, line: self.line, forty: self.forty && self.kind != BoxKind::Tank, kind: self.kind, wear: self.wear(day), marks: self.marks(day), number: number_chars(self.line, self.serial), seed: self.seed() }
    }

    /// Fit only for scrap (worn through or too old).
    pub fn for_scrap(&self, day: i64) -> bool {
        self.wear(day) >= SCRAP_WEAR || self.age_years(day) >= SCRAP_AGE
    }

    /// Steel it is cut into.
    pub fn steel(&self) -> f32 {
        if self.forty { STEEL_40 } else { STEEL_20 }
    }

    /// What it sells for on `day`.
    pub fn price(&self, day: i64) -> f32 {
        BOX_PRICE * if self.forty { 1.5 } else { 1.0 } * (1.0 - self.wear(day)).max(0.1)
    }

    pub fn note(&mut self, tick: u64, what: Event, place: BuildingId) {
        self.log.push(Stop(tick, what, place));
        if self.log.len() > LOG_LEN {
            let cut = self.log.len() - LOG_LEN;
            // The first entry (how it first came) is kept.
            self.log.drain(1..1 + cut);
        }
    }
}

/// [K34] A Stacks home built from boxes: how many it wants and those it
/// has (on site, then built in).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BoxHome {
    pub want: u16,
    pub boxes: Vec<BoxId>,
    /// Finished: its boxes are the home.
    #[serde(default)]
    pub done: bool,
}

/// [K34] Boxes on a lorry, and where they are bound (a home site that
/// bought one off it; `None`: back to storage).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TruckBoxes {
    pub boxes: Vec<BoxId>,
    #[serde(default)]
    pub to: Option<BuildingId>,
}

/// [K34] The counts, today and since records began.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxStats {
    /// Boxes made so far (salts each new box's number).
    pub made: u64,
    /// Arrivals by rail, sea and road.
    pub arrived_total: [u32; 3],
    pub arrived_today: [u32; 3],
    pub delivered_total: u32,
    pub delivered_today: u32,
    pub sold_off_truck_total: u32,
    pub sold_to_stacks_total: u32,
    pub bought_out_of_town_for_homes: u32,
    pub homes_built_total: u32,
    pub bought_by_yards_total: u32,
    pub sent_out_total: u32,
    pub sold_out_of_town_total: u32,
    pub scrapped_total: u32,
    pub scrapped_today: u32,
    pub steel_total: f32,
    /// Old boxes waiting for a scrap yard (daily, for the pressure).
    pub awaiting_scrap: u32,
    /// The same counted into the pressure: due for scrap today.
    pub due_scrap_today: u32,
    /// [r8] Boxes that left town full of exports, and export units
    /// through the forwarders today and yesterday (the pressure).
    pub exported_total: u32,
    pub export_units_today: f32,
    pub export_units_yday: f32,
    /// [r8] Boxes sold for scrap out of town (no yard in town to take them).
    pub sold_for_scrap_total: u32,
    /// [r8] Crane and carrier moves of real boxes, all told.
    pub crane_moves_total: u32,
    /// [r8] Surplus empties sent out by their lines.
    pub surplus_out_total: u32,
    /// [r11-box-stock] Boxes trucked from a full terminal to a container
    /// yard to be kept there, called back from one, and sent home by road
    /// from a terminal (or a yard) with nowhere in town to keep them.
    pub to_storage_total: u32,
    pub called_back_total: u32,
    pub cleared_by_road_total: u32,
    /// [r15-freight] Empties the lines took off a depot over its room and
    /// back to sea or rail with their next sailing or train (no lorry).
    pub repositioned_total: u32,
    /// [r11-box-stock] Boxes over the terminals' room no yard had room
    /// for, today and yesterday (the pressure for container storage).
    pub overflow_today: u32,
    pub overflow_yday: u32,
    /// [r11-box-stock] Storage fees the lines paid the yards, all told.
    pub storage_fees_total: f32,
}

/// [r11-box-stock] A container yard's storage books: the boxes it keeps
/// for the lines (the terminals' overflow), and the fees.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageBook {
    pub taken_in_total: u32,
    pub returned_total: u32,
    pub fees_today: f32,
    pub fees_yday: f32,
    pub fees_total: f32,
}

/// [K34] Every box in town and the Stacks homes built from them.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Containers {
    #[serde(default)]
    pub boxes: SlotMap<BoxId, BoxRec>,
    #[serde(default)]
    pub homes: BTreeMap<BuildingId, BoxHome>,
    #[serde(default)]
    pub trucks: BTreeMap<CarId, TruckBoxes>,
    #[serde(default)]
    pub stats: BoxStats,
    /// Records are kept (a new city, or an older save given its stock).
    #[serde(default)]
    pub live: bool,
    /// [r8] The lorry bays, gate queues and yard carriers of the depots.
    #[serde(default)]
    pub depots: BTreeMap<BuildingId, DepotYard>,
    /// [r8] What each container lorry is about (a box to fetch, a round).
    #[serde(default)]
    pub lorries: BTreeMap<CarId, LorryJob>,
    /// [r8] The boxes each freighter brings and takes, in crane order.
    #[serde(default)]
    pub ships: BTreeMap<crate::ports::ShipId, ShipBoxes>,
    /// [r8] The freight forwarders' books.
    #[serde(default)]
    pub forwarders: BTreeMap<BuildingId, Forwarder>,
    /// [r11-box-stock] Boxes each depot's stacks hold
    /// (`nd_procgen::buildings::parts::stack_room`): a depot keeps no more
    /// than that, so every box it holds is drawn. [r16-tidy] Taken by the
    /// sim from the yard's own layout (`World::refresh_yard_rooms`, through
    /// the layout hook `nd_procgen::install_sim_hooks` sets), no longer
    /// handed over by the renderer, so a headless run with the hook keeps
    /// the same stock as the game. Saved (old saves call it `drawn_room`):
    /// without the hook (the sim crate's own tests and probes) the saved
    /// room stands; unknown, its ground alone says (`World::box_capacity`).
    #[serde(default, alias = "drawn_room")]
    pub layout_room: BTreeMap<BuildingId, u16>,
    /// [r16-tidy] What each depot's `layout_room` was worked out from (a
    /// digest of its lot and look), so it is laid out again only when that
    /// changes. Not saved: after a load the first round lays them all out.
    #[serde(skip)]
    pub layout_keys: BTreeMap<BuildingId, u64>,
    /// [r11-box-stock] The container yards' storage books.
    #[serde(default)]
    pub storage: BTreeMap<BuildingId, StorageBook>,
    /// [r11-box-stock] Lorries hired to clear a terminal's backlog: on the
    /// road beside the town's own (not counted against the trade's lorries,
    /// at most [`BACKLOG_LORRIES`] at once).
    #[serde(default)]
    pub haulers: std::collections::BTreeSet<CarId>,
    /// [r8] Lorries taken off the road lately, and where they went: the
    /// follow camera says so instead of "they are gone".
    #[serde(skip)]
    pub lorry_notes: BTreeMap<CarId, (u64, String)>,
    /// Bumped whenever a box moves: the renderer redraws what changed.
    #[serde(skip)]
    pub version: u64,
}

/// [r8] Minutes the yard's carrier takes over one box between the stack
/// and a lorry's chassis (drive over, lift, carry, set down).
pub const CARRIER_MINUTES: u64 = 4;
/// [r8] Lorries that queue at a depot's gate beyond its bays.
pub const GATE_QUEUE: usize = 6;
/// [r8] Minutes a lorry waits in a bay for the box it came for (still
/// aboard a ship, on a train) before it sets off with the goods anyway.
pub const BOX_WAIT_MINUTES: u64 = 8 * 60;
/// [r8] Minutes an idle lorry stands in a bay before it leaves for home
/// or out of town (when nobody else is waiting for the bay, longer).
pub const IDLE_MINUTES: u64 = 10 * 60;
/// [r8] Seconds a forwarder's lorry stands at a firm while its goods are
/// packed into the box.
pub const PACK_SECONDS: f32 = 240.0;
/// [r8] Firms a forwarder's lorry calls at on one round, at most.
pub const ROUND_STOPS: usize = 4;
/// [r8] A forwarder's fee per firm's load, and per unit of goods on top.
pub const FORWARDER_FEE: f32 = 40.0;
pub const FORWARDER_FEE_PER_UNIT: f32 = 0.6;
/// [r8] What a forwarder pays a line to lease an empty for a trip.
pub const LEASE_PRICE: f32 = 120.0;
/// [r8] Export units a day that keep one forwarder busy (the pressure).
pub const UNITS_PER_FORWARDER: f32 = 600.0;
/// [r8] How far a firm may be from a forwarder for it to take its goods.
pub const FORWARDER_REACH: f32 = 3000.0;
/// [r8] Wear below which an empty is sound enough for exports, and from
/// which a worn one goes to the stack developers first.
pub const SOUND_WEAR: f32 = 0.45;
/// [r8] Share of a depot's room empties may fill before the surplus is
/// sent out (imports outrunning exports pile up a little, no more), and
/// the most a depot keeps at all.
pub const SURPLUS_SHARE: f32 = 0.7;
pub const SURPLUS_MAX: usize = 60;
/// [r11-box-stock] Share of its room a terminal (the port, a freight yard,
/// a cargo hub) fills with empties before the overflow is trucked to a
/// container yard to be kept: the rest is headroom for the next ship or
/// train to land into.
pub const TERMINAL_FILL: f32 = 0.75;
/// [r11-box-stock] Under this share of its room a terminal calls back the
/// boxes kept for it at the yards (empties for the town's exports).
pub const CALL_BACK_FILL: f32 = 0.4;
/// [r11-box-stock] Share of its room a container yard lets out to the
/// lines for storage (the rest for its own boxes and the comings and goings).
pub const STORAGE_FILL: f32 = 0.85;
/// [r11-box-stock] What a line pays a container yard a day for each box
/// kept for it.
pub const STORAGE_FEE: f32 = 9.0;
/// [r11-box-stock] Days a line keeps a box at a yard before it sends it
/// home (back to its terminal to ship out when there is room, else by road).
pub const STORAGE_DAYS: u64 = 12;
/// [r11-box-stock] How far a terminal's overflow is trucked to a yard, m.
pub const STORAGE_REACH: f32 = 6000.0;
/// [r11-box-stock] Boxes a day over the terminals' room with no yard to
/// keep them that make work for one more container yard (the pressure).
pub const BOXES_PER_STORAGE_YARD: f32 = 40.0;
/// [r11-box-stock] Lorries a terminal sends each hour with its overflow
/// through the bays (the carrier lifts each box), and lorries loaded at the
/// back gate by hired reach stackers while it is over its room (an older
/// save's backlog, a burst of landings).
pub const SHIFT_LORRIES_PER_HOUR: usize = 2;
pub const BACKLOG_LORRIES_PER_HOUR: usize = 24;
/// [r11-box-stock] Hours a full box stands at a terminal over its room,
/// nobody fetching it, before it is unpacked there (two days otherwise).
/// [r15-freight] Two: Harvey's port held 779 full boxes over a room of 60
/// for a day (a cap on what a terminal holds, the goods going on loose).
pub const UNPACK_AFTER_HOURS: u64 = 2;
/// [r11-box-stock] Hired backlog lorries on the road at once, all told
/// (shared out evenly between the terminals over their room).
pub const BACKLOG_LORRIES: usize = 110;

/// [r8] A depot's lorry bays, the queue at its gate, and its carrier's work.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DepotYard {
    /// Each bay's lorry (drawn in the bay; still on the road's books at
    /// the kerb, off the carriageway).
    #[serde(default)]
    pub bays: Vec<Option<CarId>>,
    /// Lorries queued at the gate, first come first in.
    #[serde(default)]
    pub queue: Vec<CarId>,
    /// The carrier's move under way.
    #[serde(default)]
    pub crane: Option<CraneJob>,
    /// Moves done (for the inspector).
    #[serde(default)]
    pub moves: u32,
}

/// [r8] One move of a yard's carrier: box `boxid` from the stack onto
/// the chassis of lorry `car` in bay `bay` (`onto_truck`), or back.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraneJob {
    pub car: CarId,
    pub boxid: BoxId,
    pub onto_truck: bool,
    pub bay: u8,
    pub start: u64,
}

impl CraneJob {
    /// How far through the move it is at `tick` + `frac`, 0..1.
    pub fn progress(&self, tick: u64, frac: f32) -> f32 {
        ((tick.saturating_sub(self.start)) as f32 + frac) / CARRIER_MINUTES as f32
    }
}

/// [r8] A container lorry's business at a depot: the load it fetches
/// (goods for a buyer, or boxes to haul), and a forwarder's round.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LorryJob {
    /// The depot it is bound for or standing at.
    pub depot: BuildingId,
    /// The paid load it fetches (delivered at `cargo.to`).
    #[serde(default)]
    pub cargo: Option<crate::traffic::Cargo>,
    /// Boxes it is to take on (a haul to a home site, the scrap yard, out
    /// of town, or the empty a forwarder leased), and where to.
    #[serde(default)]
    pub take: Vec<BoxId>,
    #[serde(default)]
    pub take_to: Option<BuildingId>,
    /// Tick it got to the depot (0: not yet).
    #[serde(default)]
    pub since: u64,
    /// [r8] A forwarder's round: the firms still to call at, and the depot
    /// the filled box goes to.
    #[serde(default)]
    pub round: Option<Round>,
}

/// [r8] A forwarder's lorry going round the firms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub forwarder: BuildingId,
    pub stops: Vec<ExportLoad>,
    pub to: BuildingId,
}

/// [r8] A freighter's boxes: those it lands (in order) and those it
/// loads, and how many crane moves are done.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShipBoxes {
    pub port: BuildingId,
    #[serde(default)]
    pub inbound: Vec<BoxId>,
    #[serde(default)]
    pub outbound: Vec<BoxId>,
    #[serde(default)]
    pub done: u32,
}

/// [r8] A freight forwarder's books: loads waiting to be collected, and
/// what it has done.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Forwarder {
    pub pending: Vec<ExportLoad>,
    pub boxes_out: u32,
    pub loads_total: u32,
    pub units_total: f32,
    pub firms: std::collections::BTreeSet<BuildingId>,
    pub fees_total: f32,
    pub fees_today: f32,
    pub rounds: u32,
    /// Tick the oldest load waiting came in.
    pub since: u64,
}

impl Containers {
    /// Boxes stored at `b`, in the order they came (oldest first).
    pub fn stored_at(&self, b: BuildingId) -> Vec<BoxId> {
        let mut v: Vec<(u64, BoxId)> = self.boxes.iter().filter(|(_, r)| matches!(r.at, Where::Stored(x) | Where::Scrap(x) if x == b)).map(|(id, r)| (r.since, id)).collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// [r10-port-boxes] The cells of depot `b`'s stack taken, but box
    /// `but`'s.
    fn cells_taken(&self, b: BuildingId, but: Option<BoxId>) -> std::collections::BTreeSet<u16> {
        let mut taken = std::collections::BTreeSet::new();
        for (id, r) in self.boxes.iter() {
            if Some(id) == but || r.at.stack() != Some(b) {
                continue;
            }
            if let Some(s) = r.slot {
                taken.insert(s);
                if r.forty {
                    taken.insert(s + 1);
                }
            }
        }
        taken
    }

    /// [r10-port-boxes] The place a box of size `forty` would take in depot
    /// `b`'s stack now: the first free cell (a forty-footer the first free
    /// even pair), so a stack fills from its first places and a box set
    /// down fills the gap the last one lifted out left.
    pub fn free_slot(&self, b: BuildingId, forty: bool) -> u16 {
        Self::first_free(&self.cells_taken(b, None), forty)
    }

    fn first_free(taken: &std::collections::BTreeSet<u16>, forty: bool) -> u16 {
        let mut s = 0u16;
        loop {
            if forty {
                if !taken.contains(&s) && !taken.contains(&(s + 1)) {
                    return s;
                }
                s += 2;
            } else {
                if !taken.contains(&s) {
                    return s;
                }
                s += 1;
            }
        }
    }

    /// [r10-port-boxes] Give box `id` its place in the stack it now stands
    /// in (keeping the one it has there if that is still its own), or take
    /// its place away when it is not in a stack. Every way into a stack
    /// (a crane off a ship, a lorry's drop, an empty returned, a forwarder's
    /// box, a salvaged home, a depot's first stock) goes through here.
    pub fn seat(&mut self, id: BoxId, keep: bool) {
        let Some(r) = self.boxes.get(id) else { return };
        let Some(b) = r.at.stack() else {
            self.boxes[id].slot = None;
            return;
        };
        let forty = r.forty;
        let taken = self.cells_taken(b, Some(id));
        let fits = |s: u16| !taken.contains(&s) && (!forty || (s % 2 == 0 && !taken.contains(&(s + 1))));
        let s = match r.slot.filter(|s| keep && fits(*s)) {
            Some(s) => s,
            None => Self::first_free(&taken, forty),
        };
        self.boxes[id].slot = Some(s);
    }

    /// [r10-port-boxes] Every stack's boxes in places of their own: a box
    /// without a place (an older save) or sharing one with a box that came
    /// before it is re-seated in the first free place. In the order the
    /// boxes came (oldest first, then by id), so the same city always comes
    /// out the same. Returns how many were moved.
    pub fn reseat_all(&mut self) -> usize {
        let mut by_stack: BTreeMap<BuildingId, Vec<(u64, BoxId)>> = BTreeMap::new();
        for (id, r) in self.boxes.iter() {
            if let Some(b) = r.at.stack() {
                by_stack.entry(b).or_default().push((r.since, id));
            }
        }
        let mut moved = 0;
        for (_, mut v) in by_stack {
            v.sort();
            let mut taken = std::collections::BTreeSet::new();
            let mut later = Vec::new();
            for &(_, id) in &v {
                let r = &self.boxes[id];
                let ok = r.slot.is_some_and(|s| !taken.contains(&s) && (!r.forty || (s % 2 == 0 && !taken.contains(&(s + 1)))));
                if ok {
                    let s = r.slot.expect("ok: it has a slot");
                    taken.insert(s);
                    if r.forty {
                        taken.insert(s + 1);
                    }
                } else {
                    later.push(id);
                }
            }
            for id in later {
                let forty = self.boxes[id].forty;
                let s = Self::first_free(&taken, forty);
                taken.insert(s);
                if forty {
                    taken.insert(s + 1);
                }
                self.boxes[id].slot = Some(s);
                moved += 1;
            }
        }
        for r in self.boxes.values_mut().filter(|r| r.at.stack().is_none()) {
            r.slot = None;
        }
        if moved > 0 {
            self.touch();
        }
        moved
    }

    /// [r10-port-boxes] Pairs of boxes in one stack whose places overlap
    /// (none, unless something went wrong): for the tests and the probes.
    pub fn slot_clashes(&self) -> usize {
        let mut cells: BTreeMap<(BuildingId, u16), u32> = BTreeMap::new();
        let mut clashes = 0;
        for r in self.boxes.values() {
            let (Some(b), Some(s)) = (r.at.stack(), r.slot) else { continue };
            for c in if r.forty { vec![s, s + 1] } else { vec![s] } {
                let n = cells.entry((b, c)).or_default();
                clashes += *n as usize;
                *n += 1;
            }
            if r.forty && s % 2 == 1 {
                clashes += 1;
            }
        }
        clashes
    }

    /// The faces of the boxes stored at `b`, for drawing.
    pub fn faces_at(&self, b: BuildingId, day: i64) -> Vec<BoxFace> {
        self.stored_at(b).into_iter().filter_map(|id| self.boxes.get(id).map(|r| r.face(id, day))).collect()
    }

    /// The faces of a finished Stacks home's boxes, in building order.
    pub fn home_faces(&self, b: BuildingId, day: i64) -> Vec<BoxFace> {
        let Some(h) = self.homes.get(&b).filter(|h| h.done) else { return Vec::new() };
        h.boxes.iter().filter_map(|id| self.boxes.get(*id).map(|r| r.face(*id, day))).collect()
    }

    /// A box by its number, spaces or not (`KOLU 482913 7`, `kolu4829137`).
    pub fn by_number(&self, text: &str) -> Option<BoxId> {
        let want: String = text.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
        self.boxes.iter().find(|(_, r)| String::from_utf8_lossy(&number_chars(r.line, r.serial)) == want).map(|(id, _)| id)
    }

    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

/// [K34] How many boxes a Stacks home on a lot of `area` m² and `floors`
/// floors is built from: about as many as the Stacks style piles there
/// (`nd_procgen` places them level by level, each level about three
/// quarters of the one below).
pub fn boxes_for_home(area: f32, floors: u8) -> u16 {
    let ground = (area / 40.0).clamp(2.0, 10.0);
    let levels = ((floors as f32 * 1.3) as i32).clamp(2, 8);
    let total: f32 = (0..levels).map(|l| ground * 0.75f32.powi(l)).sum();
    (total.round() as u16).clamp(3, 24)
}

/// [K34] Boxes a load of `amount` units comes in.
pub fn boxes_for_load(amount: f32) -> usize {
    ((amount / BOX_UNITS).ceil() as usize).clamp(1, MAX_BOXES_PER_LOAD)
}

/// [K34] A box's line, serial, origin and age (years) for a new arrival
/// from hash `h`: mixed lines, from all over, mostly a few years old with a
/// long tail of old ones and the odd brand-new box.
pub fn arrival(h: u64) -> (u8, u32, u8, f32) {
    let line = (mix(h, 11) % LINES.len() as u64) as u8;
    let serial = (mix(h, 12) % 1_000_000) as u32;
    let origin = (mix(h, 13) % PORTS.len() as u64) as u8;
    let u = unit(mix(h, 14));
    let years = if u < 0.08 { unit(mix(h, 15)) * 0.8 } else { 1.0 + u * u * 18.0 + unit(mix(h, 16)) * 1.5 };
    (line, serial, origin, years)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_digits_follow_iso_6346() {
        // The standard's own example and two more worked by hand.
        assert_eq!(check_digit(b"CSQU", 305438), 3);
        assert_eq!(number_text(0, 305438).len(), 13);
        // Letter values skip the multiples of eleven.
        assert_eq!(letter_value(b'A'), 10);
        assert_eq!(letter_value(b'B'), 12);
        assert_eq!(letter_value(b'K'), 21);
        assert_eq!(letter_value(b'L'), 23);
        assert_eq!(letter_value(b'U'), 32);
        assert_eq!(letter_value(b'V'), 34);
        assert_eq!(letter_value(b'Z'), 38);
        // MSCU 123456: against the weighted sum written out.
        let prefix = *b"MSCU";
        let mut sum = 0u32;
        for (i, v) in [24u32, 30, 13, 32, 1, 2, 3, 4, 5, 6].iter().enumerate() {
            sum += v << i;
        }
        assert_eq!(check_digit(&prefix, 123456) as u32, sum % 11 % 10);
    }

    #[test]
    fn numbers_have_owner_code_u_serial_and_check_digit() {
        let n = number_text(0, 482913);
        assert!(n.starts_with("KOLU 482913 "), "{n}");
        let chars = number_chars(3, 7);
        assert_eq!(&chars[..4], b"HZKU");
        assert_eq!(&chars[4..10], b"000007");
        assert_eq!(chars[10] - b'0', check_digit(b"HZKU", 7));
        // Every line's code is three letters, and all differ.
        let mut codes: Vec<&str> = LINES.iter().map(|l| l.code).collect();
        assert!(codes.iter().all(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase())));
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), LINES.len());
    }

    #[test]
    fn arrivals_are_mixed_in_line_origin_and_age() {
        let mut lines = [0u32; 10];
        let mut old = 0;
        let mut new = 0;
        let mut origins = std::collections::BTreeSet::new();
        for i in 0..2000u64 {
            let (l, _, o, y) = arrival(mix(i, 99));
            lines[l as usize] += 1;
            origins.insert(o);
            if y > 12.0 {
                old += 1;
            }
            if y < 1.0 {
                new += 1;
            }
        }
        assert!(lines.iter().all(|&n| n > 100), "{lines:?}");
        assert!(origins.len() > 20);
        assert!(old > 100 && new > 60, "old {old} new {new}");
    }

    #[test]
    fn wear_grows_with_age_and_knocks_and_shows() {
        let r = BoxRec { line: 1, serial: 42, forty: true, kind: BoxKind::Dry, origin: 0, built: 0, knocks: 0, owner: Owner::Line, at: Where::Train, goods: None, since: 0, visits: 1, log: Vec::new(), export: Vec::new(), bound_out: false, slot: None, kept_for: None };
        let young = r.wear(365);
        let old = r.wear(365 * 18);
        assert!(old > young + 0.4, "{young} {old}");
        let knocked = BoxRec { knocks: 120, ..r.clone() };
        assert!(knocked.wear(365) > young + 0.3);
        assert!(r.marks(365 * 20).dents >= r.marks(30).dents);
        assert!(BoxRec { built: -365 * 21, ..r.clone() }.for_scrap(0));
        assert!(!r.for_scrap(100));
        assert_eq!(r.condition_label(10), "as new");
    }

    #[test]
    fn the_log_keeps_the_first_entry_and_the_latest() {
        let mut r = BoxRec { line: 0, serial: 1, forty: false, kind: BoxKind::Dry, origin: 0, built: 0, knocks: 0, owner: Owner::Line, at: Where::Train, goods: None, since: 0, visits: 1, log: Vec::new(), export: Vec::new(), bound_out: false, slot: None, kept_for: None };
        r.note(0, Event::ArrivedRail, BuildingId::default());
        for t in 1..40 {
            r.note(t, Event::Stored, BuildingId::default());
        }
        assert_eq!(r.log.len(), LOG_LEN);
        assert_eq!(r.log[0].1, Event::ArrivedRail);
        assert_eq!(r.log.last().unwrap().0, 39);
    }

    #[test]
    fn a_home_is_built_from_a_plausible_number_of_boxes() {
        assert!(boxes_for_home(200.0, 2) >= 5);
        assert!(boxes_for_home(800.0, 6) <= 24);
        assert!(boxes_for_home(800.0, 6) > boxes_for_home(200.0, 2));
        assert_eq!(boxes_for_load(10.0), 1);
        assert_eq!(boxes_for_load(60.0), 3);
        assert_eq!(boxes_for_load(1000.0), MAX_BOXES_PER_LOAD);
    }
}
