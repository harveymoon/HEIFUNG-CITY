//! [r12-yards] Yards and fronts that say who lives there.
//!
//! The clutter round a home comes from its household, not from dice: a
//! child's bike and a ball where there are children, pots on the step and
//! a chair out front where the old live, the washing out on a dry, bright
//! day (and in when it rains or at night), and whatever the grown-ups'
//! trades and leanings bring home: a motorbike half taken apart, a
//! workbench, a grill, a surfboard by the door near the beach.
//!
//! This module only decides *what* a household puts out and whose it is
//! ([`home_wishes`]): every choice is a hash of the members' own seeds, so
//! the same household always shows the same things, and the things change
//! when the household does (a child born or grown up, someone moving in or
//! out, a new job, a bike bought). Nothing is saved and nothing draws from
//! the world's random stream. Where each thing stands is the drawing's
//! business (`nd_procgen::yards` finds the free ground and the windows of
//! the drawn building), and only round the camera.

use crate::buildings::BuildingId;
use crate::citizens::{traits_of, Citizen, CitizenId, HouseholdId};
use crate::hash::mix;
use crate::landscape::Climate;
use crate::market::BusinessKind;
use crate::weather::{Spell, Weather};
use crate::world::World;

/// Cloud cover at which it rains (or snows) on the maps that have weather:
/// the same line the renderer starts its rain at (`render/climate.rs`).
pub const RAIN_CLOUD: f32 = 0.6;
/// Cloud under which a day is bright enough to hang the washing out.
pub const DRY_CLOUD: f32 = 0.55;
/// The hours the washing may be out, 08:00 to 19:00.
pub const WASHING_HOURS: std::ops::Range<u64> = 8..19;
/// Most things one household puts out.
pub const MAX_PER_HOUSEHOLD: usize = 6;

/// One thing out in front of (or behind) a home.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PropKind {
    /// A grown-up's or a teenager's bike against the wall.
    Bike,
    /// A small child's bike with stabilisers, dropped on its side.
    KidBike,
    /// A toddler's plastic trike.
    Trike,
    /// A football left in the grass.
    Ball,
    /// A scatter of toys: bucket, blocks, a toy truck.
    Toys,
    /// Chalk drawings on the paving (a hopscotch grid).
    Chalk,
    /// A potted plant (on the step, or a window box on a block).
    Plant,
    /// A worn chair out front.
    Chair,
    /// Washing on a line or an airer (on a pole from the window on a block).
    Laundry,
    /// A motorbike on a stand, half taken apart, an oil tray under it.
    ProjectBike,
    /// A workbench with tools, a sawhorse.
    Tools,
    /// A kettle grill on legs.
    Grill,
    /// A surfboard leaning on the wall.
    Surfboard,
    /// [r13-detail] A garden trampoline (a house with children, room behind).
    Trampoline,
}

impl PropKind {
    pub const ALL: [PropKind; 14] = [
        PropKind::Bike,
        PropKind::KidBike,
        PropKind::Trike,
        PropKind::Ball,
        PropKind::Toys,
        PropKind::Chalk,
        PropKind::Plant,
        PropKind::Chair,
        PropKind::Laundry,
        PropKind::ProjectBike,
        PropKind::Tools,
        PropKind::Grill,
        PropKind::Surfboard,
        PropKind::Trampoline,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// "bike", for "Kai's bike".
    pub fn label(self) -> &'static str {
        match self {
            PropKind::Bike => "bike",
            PropKind::KidBike => "little bike",
            PropKind::Trike => "trike",
            PropKind::Ball => "ball",
            PropKind::Toys => "toys",
            PropKind::Chalk => "chalk drawings",
            PropKind::Plant => "pot plants",
            PropKind::Chair => "chair out front",
            PropKind::Laundry => "washing",
            PropKind::ProjectBike => "motorbike project",
            PropKind::Tools => "workbench",
            PropKind::Grill => "grill",
            PropKind::Surfboard => "surfboard",
            PropKind::Trampoline => "trampoline",
        }
    }

    /// The remote API's key.
    pub fn key(self) -> &'static str {
        match self {
            PropKind::Bike => "bike",
            PropKind::KidBike => "kid_bike",
            PropKind::Trike => "trike",
            PropKind::Ball => "ball",
            PropKind::Toys => "toys",
            PropKind::Chalk => "chalk",
            PropKind::Plant => "plant",
            PropKind::Chair => "chair",
            PropKind::Laundry => "laundry",
            PropKind::ProjectBike => "project_bike",
            PropKind::Tools => "tools",
            PropKind::Grill => "grill",
            PropKind::Surfboard => "surfboard",
            PropKind::Trampoline => "trampoline",
        }
    }
}

/// Where round the home a thing goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// Against the front wall by the door (or on the pavement against it
    /// where the house stands at the street).
    Step,
    /// Out on the front garden, the drive or the forecourt.
    Front,
    /// Behind the house.
    Yard,
    /// At the household's own window or balcony, on a block of flats.
    Window,
}

/// One thing a household puts out: what, whose, where, and a seed for its
/// look (colour, which way it lies).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Wish {
    pub kind: PropKind,
    pub owner: CitizenId,
    pub household: HouseholdId,
    /// The household's place among the building's (`Building::households`):
    /// on a block, which flat's window.
    pub flat: u16,
    pub place: Place,
    pub seed: u64,
}

/// What is round a home besides its people: near a beach or not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HomeCtx {
    /// The home is on the water or a short walk from a public beach.
    pub near_beach: bool,
}

/// Whether it is raining (or snowing) now: heavy cloud on a map that has
/// rain or snow, or a storm.
pub fn wet(weather: &Weather) -> bool {
    weather.spell == Spell::Storm || (weather.climate != Climate::Desert && weather.cloud >= RAIN_CLOUD)
}

/// Whether the washing is out at `hour` (0..24): by day, dry and bright, not
/// blowing a gale. In when it rains, at night and in a storm.
pub fn laundry_out(weather: &Weather, hour: u64) -> bool {
    WASHING_HOURS.contains(&hour) && !wet(weather) && weather.cloud < DRY_CLOUD && weather.wind < 0.85
}

/// A home drawn as a block of flats (things go to the windows) rather than
/// a house with a front and a yard.
pub fn is_block(homes: u16, floors: u8) -> bool {
    homes > 4 || floors > 3
}

/// `0..1` from a seed and a salt.
fn h01(seed: u64, salt: u64) -> f32 {
    (mix(seed, salt) >> 40) as f32 / (1u64 << 24) as f32
}

/// The trades that bring an engine home to work on.
fn engine_trade(k: BusinessKind) -> bool {
    matches!(k, BusinessKind::Mechanic | BusinessKind::TowYard | BusinessKind::ScrapYard | BusinessKind::BikeShop | BusinessKind::CarDealer | BusinessKind::TaxiCompany)
}

/// The trades of people who keep a bench of tools at home.
fn tool_trade(k: BusinessKind) -> bool {
    matches!(
        k,
        BusinessKind::Fab | BusinessKind::Assembly | BusinessKind::Mill | BusinessKind::Smelter | BusinessKind::Refinery | BusinessKind::ChemicalWorks | BusinessKind::OilWell | BusinessKind::StripMine | BusinessKind::ArtStudio | BusinessKind::ContainerYard
    )
}

/// The trades of cooks, who grill at home too.
fn cook_trade(k: BusinessKind) -> bool {
    matches!(
        k,
        BusinessKind::Diner | BusinessKind::NoodleBar | BusinessKind::PizzaPlace | BusinessKind::FastFood | BusinessKind::FoodHall | BusinessKind::StreetFood | BusinessKind::FineDining | BusinessKind::SeafoodRestaurant | BusinessKind::PierRestaurant | BusinessKind::Bakery
    )
}

/// The beach trades.
fn beach_trade(k: BusinessKind) -> bool {
    matches!(k, BusinessKind::BeachResort | BusinessKind::BeachBar | BusinessKind::BoatRental | BusinessKind::Marina)
}

fn trade_of(world: &World, c: &Citizen) -> Option<BusinessKind> {
    c.work.and_then(|w| world.buildings.get(w)).and_then(|b| b.business.as_ref()).map(|z| z.kind)
}

/// What one household puts out round its home, `block` saying whether the
/// home is a block of flats. Deterministic: hashes of its members' seeds,
/// their ages, bikes, vehicles and trades, the household's place in the
/// building (`flat`), and whether the washing is out (`laundry`).
pub fn household_wishes(world: &World, hid: HouseholdId, flat: u16, block: bool, laundry: bool, ctx: HomeCtx) -> Vec<Wish> {
    let Some(h) = world.households.get(hid) else { return Vec::new() };
    let mut members: Vec<(CitizenId, &Citizen)> = h.members.iter().filter_map(|&id| world.citizens.get(id).map(|c| (id, c))).collect();
    if members.is_empty() {
        return Vec::new();
    }
    // Eldest first, ties by seed: the order never depends on the slot map.
    members.sort_by(|a, b| b.1.age.cmp(&a.1.age).then(a.1.seed.cmp(&b.1.seed)));
    let hseed = members.iter().fold(0x7a2d_u64, |s, (_, c)| mix(s, c.seed));
    let mut out: Vec<Wish> = Vec::new();
    let mut put = |kind: PropKind, owner: CitizenId, place: Place, seed: u64| {
        if !out.iter().any(|w| w.kind == kind && w.owner == owner) {
            out.push(Wish { kind, owner, household: hid, flat, place, seed });
        }
    };
    let front = if block { Place::Window } else { Place::Front };
    let step = if block { Place::Window } else { Place::Step };
    let yard = if block { Place::Window } else { Place::Yard };
    // The washing first: it is the household's, hung by a grown-up.
    if laundry && h01(hseed, 1) < 0.3 + 0.12 * members.len() as f32 {
        let hanger = members.iter().filter(|(_, c)| c.age >= crate::citizens::ADULT_AGE).min_by_key(|(_, c)| mix(c.seed, 2)).or(members.first());
        if let Some((id, c)) = hanger {
            put(PropKind::Laundry, *id, yard, mix(c.seed, 3));
        }
    }
    // The old: pots on the step and a chair out front.
    if let Some((id, c)) = members.iter().find(|(_, c)| c.age >= crate::citizens::RETIRE_AGE) {
        put(PropKind::Plant, *id, step, mix(c.seed, 4));
        if !block {
            put(PropKind::Chair, *id, Place::Step, mix(c.seed, 5));
        }
    }
    for (id, c) in &members {
        let (id, s) = (*id, c.seed);
        let t = traits_of(s);
        let (sociable, hedonist, cautious, curious) = (t[0], t[2], t[3], t[4]);
        match c.age {
            // Children: a block's are only seen by the bike on the balcony.
            0..=4 if !block => {
                put(if h01(s, 6) < 0.6 { PropKind::Trike } else { PropKind::Toys }, id, Place::Front, mix(s, 7));
            }
            5..=11 => {
                if block {
                    if h01(s, 8) < 0.5 {
                        put(PropKind::KidBike, id, Place::Window, mix(s, 9));
                    }
                } else {
                    put(PropKind::KidBike, id, Place::Front, mix(s, 9));
                    put(if h01(s, 10) < 0.55 { PropKind::Ball } else { PropKind::Chalk }, id, Place::Front, mix(s, 11));
                    // [r13-detail] Some have a trampoline (the styles no
                    // longer bake one into the garden).
                    if h01(s, 26) < 0.3 {
                        put(PropKind::Trampoline, id, Place::Yard, mix(s, 27));
                    }
                }
            }
            12..=17 => {
                if (c.owns_bike && h01(s, 12) < 0.7) || h01(s, 12) < 0.35 {
                    put(PropKind::Bike, id, front, mix(s, 13));
                }
                if !block && h01(s, 14) < 0.35 {
                    put(PropKind::Ball, id, Place::Yard, mix(s, 15));
                }
            }
            _ => {}
        }
        if c.age < crate::citizens::ADULT_AGE {
            continue;
        }
        let trade = trade_of(world, c);
        // A grown-up's own bike, kept by the door or on the balcony.
        if c.owns_bike && h01(s, 16) < 0.45 {
            put(PropKind::Bike, id, front, mix(s, 17));
        }
        // A motorbike half taken apart: an engine trade, or a rider who
        // tinkers.
        let rides = c.vehicle.is_some_and(|v| v.body.two_wheeler());
        if !block && (trade.is_some_and(engine_trade) || (rides && curious > 0.55)) {
            put(PropKind::ProjectBike, id, Place::Front, mix(s, 18));
        }
        // A workbench: a trade with tools, or a careful, driven tinkerer.
        if !block && (trade.is_some_and(tool_trade) || (cautious > 0.65 && curious > 0.6 && h01(s, 19) < 0.5)) {
            put(PropKind::Tools, id, Place::Yard, mix(s, 20));
        }
        // A grill: cooks, and the sociable who like a good time.
        if !block && (trade.is_some_and(cook_trade) || (sociable > 0.62 && hedonist > 0.5)) {
            put(PropKind::Grill, id, Place::Yard, mix(s, 21));
        }
        // A surfboard near the beach: beach trades, and the young at heart.
        if ctx.near_beach && (trade.is_some_and(beach_trade) || (c.age < 45 && hedonist > 0.45 && h01(s, 22) < 0.6)) {
            put(PropKind::Surfboard, id, step, mix(s, 23));
        }
        // Gardeners keep pots too, at any age.
        if cautious > 0.6 && curious > 0.45 && h01(s, 24) < 0.6 {
            put(PropKind::Plant, id, step, mix(s, 25));
        }
    }
    // One grill, one workbench a home.
    let mut seen: Vec<PropKind> = Vec::new();
    out.retain(|w| {
        if matches!(w.kind, PropKind::Grill | PropKind::Tools | PropKind::Plant | PropKind::Chair | PropKind::Trampoline) {
            if seen.contains(&w.kind) {
                return false;
            }
            seen.push(w.kind);
        }
        true
    });
    out.truncate(MAX_PER_HOUSEHOLD);
    out
}

/// Everything a home's households put out, household by household in the
/// building's order. `laundry`: whether the washing is out now
/// ([`laundry_out`]).
pub fn home_wishes(world: &World, bid: BuildingId, laundry: bool, ctx: HomeCtx) -> Vec<Wish> {
    let Some(b) = world.buildings.get(bid) else { return Vec::new() };
    if b.homes == 0 || b.construction.is_some() || b.demolition.is_some() {
        return Vec::new();
    }
    let block = is_block(b.homes, b.floors);
    let mut all: Vec<Wish> = b.households.iter().enumerate().flat_map(|(i, &h)| household_wishes(world, h, i as u16, block, laundry, ctx)).collect();
    // A house shared by several households still has one grill, one bench,
    // one bike in pieces, one chair out front and two lines of washing.
    if !block {
        let mut count: Vec<(PropKind, usize)> = Vec::new();
        all.retain(|w| {
            let cap = match w.kind {
                PropKind::Grill | PropKind::Tools | PropKind::ProjectBike | PropKind::Chair | PropKind::Trampoline => 1,
                PropKind::Laundry | PropKind::Plant => 2,
                _ => usize::MAX,
            };
            let n = match count.iter_mut().find(|(k, _)| *k == w.kind) {
                Some(e) => {
                    e.1 += 1;
                    e.1
                }
                None => {
                    count.push((w.kind, 1));
                    1
                }
            };
            n <= cap
        });
    }
    all
}

/// A signature of what [`home_wishes`] reads for a home: its households'
/// members, ages, bikes, vehicles and work. The renderer compares it to
/// know when a home's things need placing again, without placing them.
pub fn home_signature(world: &World, bid: BuildingId) -> u64 {
    let Some(b) = world.buildings.get(bid) else { return 0 };
    let mut s = mix(b.homes as u64, b.floors as u64);
    for &h in &b.households {
        s = mix(s, 0x4855);
        for &m in world.households.get(h).map_or(&[][..], |h| &h.members[..]) {
            if let Some(c) = world.citizens.get(m) {
                let work = c.work.and_then(|w| world.buildings.get(w)).and_then(|b| b.business.as_ref()).map_or(0, |z| z.kind as u64 + 1);
                let ride = c.vehicle.map_or(0, |v| v.body as u64 + 1);
                s = mix(s, c.seed ^ ((c.age as u64) << 8) ^ ((c.owns_bike as u64) << 16) ^ (work << 20) ^ (ride << 32));
            }
        }
    }
    s
}

/// How far a home may be from a public beach for its people to keep a
/// surfboard, metres.
pub const BEACH_WALK: f32 = 600.0;

/// The middles of the public beaches (`LotShore::beach`).
pub fn beaches(world: &World) -> Vec<glam::Vec2> {
    world.lots.values().filter(|l| l.shore.beach).map(|l| l.center).collect()
}

/// What is round a home: on the water, or within [`BEACH_WALK`] of one of
/// `beaches`.
pub fn home_ctx(world: &World, bid: BuildingId, beaches: &[glam::Vec2]) -> HomeCtx {
    let Some(lot) = world.buildings.get(bid).and_then(|b| world.lots.get(b.lot)) else { return HomeCtx::default() };
    HomeCtx { near_beach: lot.shore.is_waterfront() || beaches.iter().any(|c| c.distance(lot.center) < BEACH_WALK) }
}

/// "Kai's bike".
pub fn line(world: &World, w: &Wish) -> String {
    let who = world.citizens.get(w.owner).map_or("Someone", |c| c.name.split_whitespace().next().unwrap_or(&c.name));
    format!("{who}'s {}", w.kind.label())
}

#[cfg(test)]
#[path = "yards_tests.rs"]
mod tests;
