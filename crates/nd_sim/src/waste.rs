//! [r5-trash] Where the trash goes. The depots' trucks (`crate::trash`)
//! used to unload at the depot and the trash was simply gone. Now every
//! kilogram has a destination and a price:
//!
//! - **Landfill** (Buildings > Services): trucks within `DUMP_REACH` of their
//!   depot tip here. It fills by the tonne (`LANDFILL_T` by tier) and a
//!   full one turns trucks away. It stinks for the neighbours (dirt
//!   exposure, `World::refresh_dirty`: mood, land value, fewer homes built
//!   next door), its leachate soaks the ground round it (the contamination
//!   raster), rats from it make people ill, gulls wheel over it, and at
//!   night people sleeping rough pick through it for a little money.
//! - **Incinerator** (waste-to-energy): burns `BURN_T_DAY` a day with its
//!   furnace crews, putting `MWH_PER_T` on the grid for each tonne
//!   (`World::waste_mw`, counted in the plants' supply); dearer per tonne,
//!   smoke from its stack, fallout on the ground downwind, and ash (a
//!   fifth of the weight) to the landfill or out of town.
//! - **Recycling plant**: sorting lines (many jobs) take `SORT_T_DAY`; metal
//!   goes to the scrap yards in town, plastics to the works that use them,
//!   the rest (and paper and glass) is sold out of town. What cannot be
//!   recovered goes on to a landfill, an incinerator or out of town.
//! - **Transfer station**: compacts what it is given and ships it out of
//!   town by rail or ship when the city has a freight yard or a shipping
//!   port (half the price of lorries), else in trailers by road.
//! - **No site in reach** (the default): the load is unloaded at the depot's
//!   yard and a contractor hauls it out of town by lorry at the dearest
//!   price (`COST_ROAD`). The yard holds `YARD_T`; the contractor takes
//!   `HAUL_T_DAY` a day (half while the treasury is in the red). When the
//!   yard is full the trucks cannot unload, so they stop collecting and
//!   the bins overflow: litter, smell, complaints, rats.
//!
//! Every city (an old save on load, a new city on its first hour) starts
//! with a starter contract: `STARTER_DAYS` days of the contractor at
//! `STARTER_DISCOUNT` and with no daily limit, so nobody drowns in trash
//! the day this arrives.
//!
//! The costs are booked once a day under `Flow::WasteDisposal` (per site,
//! and the lorries out of town); what the recyclers sell under
//! `Flow::RecyclingSales`. Nothing here draws a random number but the
//! rats and the scavengers' luck, once a day, from the society stream.

use crate::buildings::{BuildingId, Tier};
use crate::economy::{Flow, Item};
use crate::market::{BusinessKind, Resource};
use crate::roads::SegmentId;
use crate::services::ServiceKind;
use crate::World;
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TONNE: f32 = 1000.0;

// ── balance ────────────────────────────────────────────────────────────────

/// Tonnes a landfill holds, by tier.
pub const LANDFILL_T: [f32; 3] = [900.0, 2600.0, 6500.0];
/// Tonnes a day an incinerator burns, a recycling plant sorts and a
/// transfer station ships at full staff, by tier.
pub const BURN_T_DAY: [f32; 3] = [20.0, 50.0, 120.0];
pub const SORT_T_DAY: [f32; 3] = [10.0, 28.0, 70.0];
pub const TRANSFER_T_DAY: [f32; 3] = [50.0, 140.0, 350.0];
/// A site that sorts, burns or ships holds this many days of its
/// throughput waiting before it turns trucks away.
pub const BACKLOG_DAYS: f32 = 2.0;
/// Electricity a tonne burnt makes, MWh.
pub const MWH_PER_T: f32 = 0.7;
/// Share of what burns that is left as ash.
pub const ASH_SHARE: f32 = 0.2;
/// What a recycling plant recovers of every tonne it sorts.
pub const METAL_SHARE: f32 = 0.06;
pub const PLASTIC_SHARE: f32 = 0.12;
pub const PAPER_GLASS_SHARE: f32 = 0.24;
/// Paper and glass, a tonne, sold out of town.
pub const PAPER_GLASS_PRICE: f32 = 25.0;
/// Market units of metal or plastics in a tonne recovered.
pub const UNITS_PER_T: f32 = 1.0;
/// What a scrap yard or a works pays for a recovered unit, as a share of
/// the base price (out of town it fetches the export discount).
pub const LOCAL_SHARE: f32 = 0.4;
/// Cost per tonne: tipped, burnt, sorted, shipped by rail or ship from a
/// transfer station, by road from a transfer station, and hauled out of
/// town by lorry from a depot's yard (or residue with nowhere to go).
pub const COST_LANDFILL: f32 = 14.0;
pub const COST_BURN: f32 = 50.0;
pub const COST_SORT: f32 = 40.0;
pub const COST_SHIP: f32 = 45.0;
pub const COST_TRANSFER_ROAD: f32 = 80.0;
pub const COST_ROAD: f32 = 120.0;
/// The starter contract: days, and the share of the road price paid.
pub const STARTER_DAYS: u64 = 14;
pub const STARTER_DISCOUNT: f32 = 0.5;
/// A depot's yard, tonnes, and what the contractor takes from it a day.
pub const YARD_T: [f32; 3] = [30.0, 60.0, 120.0];
pub const HAUL_T_DAY: [f32; 3] = [40.0, 80.0, 150.0];
/// The contractor's hours (lorries out of town).
pub const HAUL_HOURS: (u64, u64) = (6, 20);
/// A city truck takes its load to a site this far from its depot.
pub const DUMP_REACH: f32 = 3000.0;
/// Hourly leachate a landfill soaks into the ground: (base radius,
/// amount at empty, more at full).
pub const LEACHATE: (f32, f32, f32) = (70.0, 0.0006, 0.0024);
/// Hourly fallout downwind of a burning incinerator: (radius, amount at
/// full burn, metres downwind).
pub const FALLOUT: (f32, f32, f32) = (150.0, 0.0018, 160.0);
/// Homes this near a landfill say it stinks (a few a day); rats from it
/// reach this far.
pub const LANDFILL_SMELL: f32 = 280.0;
pub const LANDFILL_RATS: f32 = 150.0;
/// Daily chance a resident by a landfill falls ill (rats), at full.
pub const LANDFILL_RAT_ILLNESS: f32 = 0.004;
/// People sleeping rough within this of a landfill scavenge at night: a
/// little money, at most this many households a landfill, and a chance
/// of getting hurt.
pub const SCAVENGE_REACH: f32 = 1500.0;
pub const SCAVENGE_PAY: f32 = 4.0;
pub const SCAVENGERS: usize = 8;
pub const SCAVENGE_HURT: f32 = 0.02;
/// Street litter (`0..1` per street piece): what an overflowing bin
/// throws onto its street a day per level of overflow, and what is left
/// after a day with and without a depot's street sweepers in reach.
pub const LITTER_PER_OVERFLOW: f32 = 0.22;
pub const LITTER_KEPT: f32 = 0.88;
pub const LITTER_SWEPT: f32 = 0.5;
/// Land value a fully littered street takes off round it.
pub const LITTER_LV: f32 = 0.03;
pub const LITTER_LV_RADIUS: f32 = 45.0;

/// Where a tonne of trash ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dest {
    /// Tipped into a landfill (ash and residue too).
    Landfill,
    /// Burnt in an incinerator (less the ash).
    Burned,
    /// Recovered by a recycling plant.
    Recycled,
    /// Shipped out of town by rail or ship from a transfer station.
    Shipped,
    /// Out of town by lorry: a depot's contractor, residue and ash with
    /// nowhere to go, or a transfer station with no rail or port.
    Road,
}

impl Dest {
    pub const COUNT: usize = 5;
    pub const ALL: [Dest; Self::COUNT] = [Dest::Landfill, Dest::Burned, Dest::Recycled, Dest::Shipped, Dest::Road];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            Dest::Landfill => "landfill",
            Dest::Burned => "burned",
            Dest::Recycled => "recycled",
            Dest::Shipped => "shipped",
            Dest::Road => "road",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dest::Landfill => "landfill",
            Dest::Burned => "burned",
            Dest::Recycled => "recycled",
            Dest::Shipped => "out by rail or ship",
            Dest::Road => "out of town by lorry",
        }
    }
}

/// One disposal site's state (a landfill, incinerator, recycling plant or
/// transfer station), keyed by its building.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WasteSite {
    /// A landfill: tonnes in the ground.
    pub filled_t: f32,
    /// The others: tonnes tipped and waiting to be burnt, sorted or shipped.
    pub backlog_t: f32,
    /// Tonnes tipped (or passed on to it) today and yesterday.
    pub in_today_t: f32,
    pub in_yesterday_t: f32,
    /// Tonnes burnt, sorted or shipped today and yesterday.
    pub done_today_t: f32,
    pub done_yesterday_t: f32,
    /// A recycling plant: tonnes recovered.
    pub recovered_today_t: f32,
    pub recovered_yesterday_t: f32,
    /// An incinerator: what it put on the grid the last hour, and the day's.
    pub mw: f32,
    pub mwh_today: f32,
    pub mwh_yesterday: f32,
    /// Running costs by the tonne (booked at midnight), and sales.
    pub cost_today: f32,
    pub cost_yesterday: f32,
    pub sales_today: f32,
    pub sales_yesterday: f32,
    /// Trucks turned away (full) today and yesterday.
    pub turned_away_today: u32,
    pub turned_away_yesterday: u32,
    /// The fill step last drawn (a landfill's mound), for the redraw.
    pub shown: u8,
}

/// The city's disposal: the sites, the depots' yards, the starter
/// contract and the day's figures by destination.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Disposal {
    pub sites: BTreeMap<BuildingId, WasteSite>,
    /// Tonnes unloaded at each depot's yard, waiting for the contractor.
    pub yards: BTreeMap<BuildingId, f32>,
    /// The contractor's lorry on its way out of town from each depot this
    /// hour (shown as a truck; the money is the hourly haul).
    pub hauled_today_t: f32,
    /// Set the first hour the city runs with disposal (old saves on load,
    /// new cities at once): the starter contract runs to `starter_until`.
    pub started: bool,
    pub starter_until: u64,
    /// Tonnes by destination, today, yesterday, ever.
    #[serde(default, deserialize_with = "crate::economy::padded")]
    pub t_today: [f32; Dest::COUNT],
    #[serde(default, deserialize_with = "crate::economy::padded")]
    pub t_yesterday: [f32; Dest::COUNT],
    #[serde(default, deserialize_with = "crate::economy::padded")]
    pub t_total: [f32; Dest::COUNT],
    /// What each destination cost, today and yesterday.
    #[serde(default, deserialize_with = "crate::economy::padded")]
    pub cost_today: [f32; Dest::COUNT],
    #[serde(default, deserialize_with = "crate::economy::padded")]
    pub cost_yesterday: [f32; Dest::COUNT],
    pub cost_total: f32,
    /// Lorries out of town: the cost not yet booked (booked at midnight).
    pub road_cost_due: f32,
    pub sales_today: f32,
    pub sales_yesterday: f32,
    pub sales_total: f32,
    /// Loads a truck could not unload (the depot's yard full), today and
    /// yesterday; days some yard stood full.
    pub blocked_today: u32,
    pub blocked_yesterday: u32,
    pub yard_full_days: u32,
    /// Scavengers last night and their takings.
    pub scavengers_yesterday: u32,
    pub scavenged_total: f32,
    pub landfill_ill_total: u32,
}

impl Disposal {
    pub fn held_t(&self) -> f32 {
        self.yards.values().sum()
    }

    /// Tonnes that left the depots yesterday, all destinations.
    pub fn yesterday_t(&self) -> f32 {
        self.t_yesterday.iter().sum()
    }

    pub fn starter_on(&self, day: u64) -> bool {
        !self.started || day < self.starter_until
    }
}

/// What a disposal site can take, for the trucks and the inspector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiteInfo {
    pub id: BuildingId,
    pub kind: ServiceKind,
    pub tier: Tier,
    pub pos: Vec2,
    /// Share of its jobs filled, `0..1`.
    pub staffing: f32,
}

/// What a site of `kind` handles a day at full staff, tonnes (a landfill:
/// what it holds).
pub fn site_capacity(kind: ServiceKind, tier: Tier) -> f32 {
    let t = tier.index();
    match kind {
        ServiceKind::Landfill => LANDFILL_T[t],
        ServiceKind::Incinerator => BURN_T_DAY[t],
        ServiceKind::RecyclingPlant => SORT_T_DAY[t],
        ServiceKind::TransferStation => TRANSFER_T_DAY[t],
        _ => 0.0,
    }
}

/// The share of the ground's trash price a truck's tip at `kind` saves
/// over the lorries (lower is the trucks' first pick), as metres of extra
/// drive it is worth: recycling first, then burning, the tip, the shed.
fn preference(kind: ServiceKind) -> f32 {
    match kind {
        ServiceKind::RecyclingPlant => -500.0,
        ServiceKind::Incinerator => -200.0,
        ServiceKind::Landfill => 0.0,
        _ => 250.0,
    }
}

impl World {
    /// Every finished disposal site: kind, tier, where, how staffed.
    pub fn waste_sites(&self) -> Vec<SiteInfo> {
        self.buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind.is_waste()).map(|s| (id, b, s)))
            .filter(|(_, b, _)| b.operating())
            .filter_map(|(id, b, s)| {
                let staffing = if b.is_derelict() || b.jobs == 0 { 0.0 } else { (b.workers.len() as f32 / b.jobs as f32).clamp(0.0, 1.0) };
                self.lots.get(b.lot).map(|l| SiteInfo { id, kind: s.kind, tier: s.tier, pos: l.center, staffing })
            })
            .collect()
    }

    /// A landfill's share full, `0..1` (`None` for anything else).
    pub fn landfill_fill(&self, id: BuildingId) -> Option<f32> {
        let s = self.buildings.get(id)?.service.filter(|s| s.kind == ServiceKind::Landfill)?;
        let filled = self.society.trash.disposal.sites.get(&id).map_or(0.0, |x| x.filled_t);
        Some((filled / site_capacity(s.kind, s.tier)).clamp(0.0, 1.0))
    }

    /// Tonnes more a site will take now (0 when it is full, unstaffed or
    /// shut).
    pub fn site_room_t(&self, info: &SiteInfo) -> f32 {
        if info.staffing <= 0.0 {
            return 0.0;
        }
        let st = self.society.trash.disposal.sites.get(&info.id);
        let cap = site_capacity(info.kind, info.tier);
        match info.kind {
            ServiceKind::Landfill => (cap - st.map_or(0.0, |s| s.filled_t)).max(0.0),
            _ => (cap * BACKLOG_DAYS - st.map_or(0.0, |s| s.backlog_t)).max(0.0),
        }
    }

    /// Where a city truck from `depot` standing at `from` takes its load:
    /// the best site with room within `DUMP_REACH` of the depot (recycling
    /// first, then burning, the tip, the transfer shed; nearer first), or
    /// `None` for the depot's own yard.
    pub(crate) fn dump_for(&self, depot: BuildingId, from: Vec2, load_kg: f32) -> Option<BuildingId> {
        let home = self.building_lot(depot).map(|l| l.center)?;
        let mut best: Option<(f32, BuildingId)> = None;
        for s in self.waste_sites() {
            if s.pos.distance(home) > DUMP_REACH || self.site_room_t(&s) * TONNE < load_kg.min(2_000.0) {
                continue;
            }
            let score = s.pos.distance(from) + preference(s.kind);
            if best.is_none_or(|(b, id)| score < b || (score == b && s.id < id)) {
                best = Some((score, s.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// A truck tips `kg` at a disposal site: what it took, kg.
    pub(crate) fn tip_at_site(&mut self, site: BuildingId, kg: f32) -> f32 {
        let Some(info) = self.waste_sites().into_iter().find(|s| s.id == site) else { return 0.0 };
        let take_t = (kg / TONNE).min(self.site_room_t(&info)).max(0.0);
        let d = &mut self.society.trash.disposal;
        let st = d.sites.entry(site).or_default();
        if take_t * TONNE + 1.0 < kg {
            st.turned_away_today += 1;
        }
        st.in_today_t += take_t;
        if info.kind == ServiceKind::Landfill {
            st.filled_t += take_t;
            st.cost_today += take_t * COST_LANDFILL;
            d.t_today[Dest::Landfill.index()] += take_t;
            d.t_total[Dest::Landfill.index()] += take_t;
            d.cost_today[Dest::Landfill.index()] += take_t * COST_LANDFILL;
        } else {
            st.backlog_t += take_t;
        }
        take_t * TONNE
    }

    /// A truck unloads `kg` at its depot's yard: what the yard took, kg
    /// (all of it while the starter contract runs).
    pub(crate) fn tip_at_depot(&mut self, depot: BuildingId, kg: f32) -> f32 {
        let tier = self.buildings.get(depot).and_then(|b| b.service).map_or(Tier::Mid, |s| s.tier);
        let day = self.clock.day();
        let d = &mut self.society.trash.disposal;
        let have = d.yards.get(&depot).copied().unwrap_or(0.0);
        let room = if d.starter_on(day) { f32::MAX } else { (YARD_T[tier.index()] - have).max(0.0) };
        let take_t = (kg / TONNE).min(room);
        *d.yards.entry(depot).or_default() += take_t;
        if take_t * TONNE + 1.0 < kg {
            d.blocked_today += 1;
        }
        take_t * TONNE
    }

    /// The road price per tonne today (the starter contract's discount).
    pub fn road_price(&self) -> f32 {
        if self.society.trash.disposal.starter_on(self.clock.day()) {
            COST_ROAD * STARTER_DISCOUNT
        } else {
            COST_ROAD
        }
    }

    /// Whether a transfer station can ship by rail or sea: a working
    /// freight yard or shipping port in town.
    pub fn waste_ships_by_rail(&self) -> bool {
        !self.ports.working(crate::ports::PortKind::ShippingPort).is_empty()
            || self.buildings.values().any(|b| b.rail.as_ref().is_some_and(|l| l.kind == crate::rail::RailFacility::FreightYard) && b.operating() && !b.is_derelict())
    }

    /// Electricity the incinerators put on the grid now, MW.
    pub fn waste_mw(&self) -> f32 {
        self.society.trash.disposal.sites.values().map(|s| s.mw).sum::<f32>() + 0.0
    }

    /// Residue or ash (`t` tonnes) from `from` goes on: residue to an
    /// incinerator with room first (the trucks' own order: burning before
    /// the tip), then to the landfill with room nearest it, else out of
    /// town by lorry; ash to a landfill or the lorries.
    ///
    /// [r6-perfect-city] Residue used to go to the landfill first, so a
    /// city with a recycling plant, an incinerator and a landfill tipped
    /// 58% of its trash and the incinerator stood idle.
    fn send_on(&mut self, t: f32, from: Vec2, may_burn: bool, sites: &[SiteInfo]) {
        if t <= 0.0 {
            return;
        }
        let mut left = t;
        let mut order: Vec<&SiteInfo> = sites.iter().filter(|s| s.kind == ServiceKind::Landfill || (may_burn && s.kind == ServiceKind::Incinerator)).collect();
        order.sort_by(|a, b| (a.kind != ServiceKind::Incinerator).cmp(&(b.kind != ServiceKind::Incinerator)).then(a.pos.distance(from).total_cmp(&b.pos.distance(from))).then(a.id.cmp(&b.id)));
        for s in order {
            if left <= 0.0 {
                break;
            }
            let room = self.site_room_t(s);
            let take = left.min(room);
            if take <= 0.0 {
                continue;
            }
            left -= take;
            let d = &mut self.society.trash.disposal;
            let st = d.sites.entry(s.id).or_default();
            st.in_today_t += take;
            if s.kind == ServiceKind::Landfill {
                st.filled_t += take;
                st.cost_today += take * COST_LANDFILL;
                d.t_today[Dest::Landfill.index()] += take;
                d.t_total[Dest::Landfill.index()] += take;
                d.cost_today[Dest::Landfill.index()] += take * COST_LANDFILL;
            } else {
                st.backlog_t += take;
            }
        }
        if left > 0.0 {
            self.haul_by_road(left);
        }
    }

    /// `t` tonnes out of town by lorry, at today's road price.
    fn haul_by_road(&mut self, t: f32) {
        let cost = t * self.road_price();
        let d = &mut self.society.trash.disposal;
        d.t_today[Dest::Road.index()] += t;
        d.t_total[Dest::Road.index()] += t;
        d.cost_today[Dest::Road.index()] += cost;
        d.road_cost_due += cost;
    }

    /// Sell `units` of recovered `r` from `site`: to the scrap yards (metal)
    /// or works that use it (plastics) in town that are short, nearest
    /// first, then out of town. Returns the takings.
    fn sell_recovered(&mut self, site: BuildingId, at: Vec2, r: Resource, units: f32) -> f32 {
        if units <= 0.0 {
            return 0.0;
        }
        let mut buyers: Vec<(f32, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && !b.is_derelict())
            .filter_map(|(id, b)| {
                let z = b.business.as_ref().filter(|z| !z.closed)?;
                let wants = match r {
                    Resource::Metal => z.kind == BusinessKind::ScrapYard && z.stock_of(r) < 80.0,
                    _ => z.kind.recipe().inputs.iter().any(|(x, _)| *x == r) && z.stock_of(r) < 30.0,
                };
                wants.then(|| (self.lots.get(b.lot).map_or(f32::MAX, |l| l.center.distance(at)), id))
            })
            .collect();
        buyers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut left = units;
        let mut takings = 0.0;
        for (_, bid) in buyers.into_iter().take(4) {
            if left <= 0.0 {
                break;
            }
            let room = match r {
                Resource::Metal => 80.0,
                _ => 30.0,
            } - self.buildings[bid].business.as_ref().map_or(0.0, |z| z.stock_of(r));
            let n = left.min(room.max(0.0));
            if n <= 0.0 {
                continue;
            }
            let price = n * r.base_price() * LOCAL_SHARE;
            let paid = self.pay_city(crate::bank::Account::Business(bid), Flow::RecyclingSales, price, Some(Item::Building(site)));
            if paid <= 0.0 {
                continue;
            }
            let got = n * (paid / price).clamp(0.0, 1.0);
            if let Some(z) = self.buildings[bid].business.as_mut() {
                z.add(r, got);
                z.spend_today += paid;
            }
            takings += paid;
            left -= got;
        }
        if left > 0.0 {
            let income = left * r.base_price() * crate::market::EXPORT_DISCOUNT;
            self.economy.credit_item(Flow::RecyclingSales, income, Item::Building(site));
            takings += income;
        }
        takings
    }

    // ── the hourly round ───────────────────────────────────────────────────

    /// Every hour (with the bins): the sites burn, sort and ship what they
    /// hold; the contractor hauls from the depots' yards; leachate and
    /// fallout; landfills redrawn as they fill.
    pub(crate) fn hourly_waste(&mut self) {
        crate::prof_scope!("hourly_waste");
        let day = self.clock.day();
        let hour = self.clock.hour();
        {
            let d = &mut self.society.trash.disposal;
            if !d.started {
                d.started = true;
                d.starter_until = day + STARTER_DAYS;
            }
        }
        let sites = self.waste_sites();
        // Sites that are gone, or are not disposal sites any more, go;
        // a site under construction keeps nothing yet.
        let alive: Vec<BuildingId> = sites.iter().map(|s| s.id).collect();
        let stale: Vec<BuildingId> = self.society.trash.disposal.sites.keys().copied().filter(|id| !alive.contains(id) && !self.buildings.get(*id).is_some_and(|b| b.service.is_some_and(|s| s.kind.is_waste()))).collect();
        for id in stale {
            self.society.trash.disposal.sites.remove(&id);
        }
        let yards: Vec<BuildingId> = self.society.trash.disposal.yards.keys().copied().filter(|id| !self.buildings.get(*id).is_some_and(|b| b.service.is_some_and(|s| s.kind == ServiceKind::TrashDepot))).collect();
        for id in yards {
            // A depot pulled down: what lay in its yard goes out by lorry.
            let t = self.society.trash.disposal.yards.remove(&id).unwrap_or(0.0);
            self.haul_by_road(t);
        }
        let wind = crate::weather::wind_dir(self.clock.tick);
        let by_rail = self.waste_ships_by_rail();
        for s in &sites {
            let open = self.buildings.get(s.id).is_some_and(|b| b.service.is_some_and(|x| x.kind.hours().contains(hour as u8)));
            let rate = site_capacity(s.kind, s.tier) / 24.0 * s.staffing;
            let st = self.society.trash.disposal.sites.entry(s.id).or_default().clone();
            match s.kind {
                ServiceKind::Landfill => {
                    let fill = (st.filled_t / site_capacity(s.kind, s.tier)).clamp(0.0, 1.0);
                    let (r, a0, a1) = LEACHATE;
                    if st.filled_t > 0.0 {
                        self.contamination.add(s.pos, r + 60.0 * fill, a0 + a1 * fill);
                    }
                    // The mound is redrawn in twentieths.
                    let step = (fill * 20.0).floor() as u8;
                    if step != st.shown {
                        self.society.trash.disposal.sites.get_mut(&s.id).unwrap().shown = step;
                        self.events.push(crate::SimEvent::BuildingChanged(s.id));
                    }
                }
                ServiceKind::Incinerator => {
                    // [r5-trash] The furnace takes the day's rate round the clock.
                    let burn = st.backlog_t.min(rate);
                    let ash = burn * ASH_SHARE;
                    {
                        let d = &mut self.society.trash.disposal;
                        let x = d.sites.get_mut(&s.id).unwrap();
                        x.backlog_t -= burn;
                        x.done_today_t += burn;
                        x.mw = burn * MWH_PER_T;
                        x.mwh_today += burn * MWH_PER_T;
                        x.cost_today += burn * COST_BURN;
                        d.t_today[Dest::Burned.index()] += burn - ash;
                        d.t_total[Dest::Burned.index()] += burn - ash;
                        d.cost_today[Dest::Burned.index()] += burn * COST_BURN;
                    }
                    if burn > 0.0 {
                        let (r, a, off) = FALLOUT;
                        let share = (burn / BURN_T_DAY[s.tier.index()] * 24.0).clamp(0.0, 1.5);
                        self.contamination.add(s.pos + wind.normalize_or_zero() * off, r, a * share);
                    }
                    self.send_on(ash, s.pos, false, &sites);
                }
                ServiceKind::RecyclingPlant => {
                    // Two shifts: the day's rate over its open hours.
                    let sort = if open { st.backlog_t.min(rate * 24.0 / 16.0) } else { 0.0 };
                    if sort <= 0.0 {
                        continue;
                    }
                    let metal = sort * METAL_SHARE;
                    let plastic = sort * PLASTIC_SHARE;
                    let paper = sort * PAPER_GLASS_SHARE;
                    let recovered = metal + plastic + paper;
                    let mut takings = self.sell_recovered(s.id, s.pos, Resource::Metal, metal * UNITS_PER_T);
                    takings += self.sell_recovered(s.id, s.pos, Resource::Plastics, plastic * UNITS_PER_T);
                    let pg = paper * PAPER_GLASS_PRICE;
                    self.economy.credit_item(Flow::RecyclingSales, pg, Item::Building(s.id));
                    takings += pg;
                    {
                        let d = &mut self.society.trash.disposal;
                        let x = d.sites.get_mut(&s.id).unwrap();
                        x.backlog_t -= sort;
                        x.done_today_t += sort;
                        x.recovered_today_t += recovered;
                        x.cost_today += sort * COST_SORT;
                        x.sales_today += takings;
                        d.t_today[Dest::Recycled.index()] += recovered;
                        d.t_total[Dest::Recycled.index()] += recovered;
                        d.cost_today[Dest::Recycled.index()] += sort * COST_SORT;
                        d.sales_today += takings;
                        d.sales_total += takings;
                    }
                    self.send_on(sort - recovered, s.pos, true, &sites);
                }
                ServiceKind::TransferStation => {
                    let ship = if open { st.backlog_t.min(rate * 24.0 / 15.0) } else { 0.0 };
                    if ship <= 0.0 {
                        continue;
                    }
                    let (dest, per_t) = if by_rail { (Dest::Shipped, COST_SHIP) } else { (Dest::Road, COST_TRANSFER_ROAD) };
                    let d = &mut self.society.trash.disposal;
                    let x = d.sites.get_mut(&s.id).unwrap();
                    x.backlog_t -= ship;
                    x.done_today_t += ship;
                    x.cost_today += ship * per_t;
                    d.t_today[dest.index()] += ship;
                    d.t_total[dest.index()] += ship;
                    d.cost_today[dest.index()] += ship * per_t;
                }
                _ => {}
            }
        }
        // The contractor's lorries take the depots' yards out of town.
        if (HAUL_HOURS.0..HAUL_HOURS.1).contains(&hour) {
            let starter = self.society.trash.disposal.starter_on(day);
            let broke = self.economy.treasury < 0.0;
            let hours = (HAUL_HOURS.1 - HAUL_HOURS.0) as f32;
            let depots: Vec<(BuildingId, f32)> = self.society.trash.disposal.yards.iter().filter(|(_, t)| **t > 0.0).map(|(id, t)| (*id, *t)).collect();
            for (depot, have) in depots {
                let tier = self.buildings.get(depot).and_then(|b| b.service).map_or(Tier::Mid, |s| s.tier);
                let rate = if starter { f32::MAX } else { HAUL_T_DAY[tier.index()] / hours * if broke { 0.5 } else { 1.0 } };
                let t = have.min(rate);
                *self.society.trash.disposal.yards.get_mut(&depot).unwrap() -= t;
                self.society.trash.disposal.hauled_today_t += t;
                self.haul_by_road(t);
                if t >= 3.0 {
                    self.send_contractor(depot);
                }
            }
        }
    }

    // ── midnight ───────────────────────────────────────────────────────────

    /// The day's books (one posting a site, one for the lorries), the
    /// landfills' neighbours (complaints, rats), the scavengers, and the
    /// day's counters roll.
    pub(crate) fn daily_waste(&mut self) {
        crate::prof_scope!("daily_waste");
        let tick = self.clock.tick;
        let posts: Vec<(BuildingId, f32)> = self.society.trash.disposal.sites.iter().filter(|(_, s)| s.cost_today > 0.0).map(|(id, s)| (*id, s.cost_today)).collect();
        for (id, c) in posts {
            self.economy.debit_item(Flow::WasteDisposal, c, Item::Building(id));
        }
        let road = std::mem::take(&mut self.society.trash.disposal.road_cost_due);
        if road > 0.0 {
            self.economy.debit_item(Flow::WasteDisposal, road, Item::Named("lorries out of town".to_string()));
        }
        let sites = self.waste_sites();
        let landfills: Vec<(BuildingId, Vec2, f32, String)> = sites
            .iter()
            .filter(|s| s.kind == ServiceKind::Landfill)
            .filter_map(|s| Some((s.id, s.pos, self.landfill_fill(s.id)?, self.buildings.get(s.id)?.name.clone())))
            .filter(|(_, _, f, _)| *f > 0.0)
            .collect();
        if !landfills.is_empty() {
            // Neighbours: a few say it stinks; rats make some ill.
            let homes: Vec<(BuildingId, Vec2)> = self.buildings.iter().filter(|(_, b)| !b.residents.is_empty()).filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center))).collect();
            for (lf, at, fill, name) in &landfills {
                let mut voices = 0;
                for (hb, hp) in &homes {
                    let d = hp.distance(*at);
                    if d > LANDFILL_SMELL {
                        continue;
                    }
                    let residents = self.buildings[*hb].residents.clone();
                    for cid in residents {
                        if voices < 6 && (slotmap::Key::data(&cid).as_ffi() ^ tick / crate::clock::MINUTES_PER_DAY) % 5 == 0 {
                            if let Some(c) = self.citizens.get_mut(cid) {
                                c.remember(tick, format!("The stink from {name} reaches the flat again. Gulls everywhere."));
                                self.concerns.heard_at(cid, c, crate::concerns::Concern::Trash, 0.3 + 0.3 * fill, Some((*lf, *at)));
                                voices += 1;
                            }
                        }
                        if d <= LANDFILL_RATS && self.config.emergencies && !self.is_patient(cid) && self.society_rng().random::<f32>() < LANDFILL_RAT_ILLNESS * (0.3 + fill) && self.fall_ill(cid).is_some() {
                            self.society.trash.disposal.landfill_ill_total += 1;
                            if let Some(c) = self.citizens.get_mut(cid) {
                                c.remember(tick, format!("Rats from {name} got into the building. Now I'm sick."));
                            }
                        }
                    }
                }
            }
            // Scavengers: people sleeping rough pick through the tip at
            // night for a little money; some get hurt.
            let rough: Vec<(crate::citizens::HouseholdId, Vec2)> = self
                .society
                .homeless
                .since
                .iter()
                .filter_map(|(h, _)| self.households.get(*h).and_then(|hh| hh.members.first()).and_then(|m| self.citizens.get(*m)).map(|c| (*h, c.pos)))
                .collect();
            let mut n = 0u32;
            for (lf, at, _, name) in &landfills {
                let mut near: Vec<&(crate::citizens::HouseholdId, Vec2)> = rough.iter().filter(|(_, p)| p.distance(*at) <= SCAVENGE_REACH).collect();
                near.sort_by(|a, b| a.1.distance(*at).total_cmp(&b.1.distance(*at)).then(a.0.cmp(&b.0)));
                for (h, _) in near.into_iter().take(SCAVENGERS) {
                    let paid = self.transfer(crate::bank::Account::Outside, crate::bank::Account::Household(*h), SCAVENGE_PAY, crate::bank::Kind::Wages);
                    self.society.trash.disposal.scavenged_total += paid;
                    n += 1;
                    let Some(m) = self.households.get(*h).and_then(|hh| hh.members.first().copied()) else { continue };
                    if let Some(c) = self.citizens.get_mut(m) {
                        c.remember(tick, format!("Picked through {name} all night. Copper wire, a few cans. Enough for a meal."));
                    }
                    if self.config.emergencies && !self.is_patient(m) && self.society_rng().random::<f32>() < SCAVENGE_HURT {
                        let _ = self.fall_ill(m);
                    }
                }
                let _ = lf;
            }
            self.society.trash.disposal.scavengers_yesterday = n;
        } else {
            self.society.trash.disposal.scavengers_yesterday = 0;
        }
        // Some yard stood full: the trucks went home loaded.
        let full = self.society.trash.disposal.yards.iter().any(|(id, t)| self.buildings.get(*id).and_then(|b| b.service).is_some_and(|s| *t >= YARD_T[s.tier.index()] - 0.5));
        let d = &mut self.society.trash.disposal;
        if full {
            d.yard_full_days += 1;
        }
        for s in d.sites.values_mut() {
            s.in_yesterday_t = std::mem::take(&mut s.in_today_t);
            s.done_yesterday_t = std::mem::take(&mut s.done_today_t);
            s.recovered_yesterday_t = std::mem::take(&mut s.recovered_today_t);
            s.mwh_yesterday = std::mem::take(&mut s.mwh_today);
            s.cost_yesterday = std::mem::take(&mut s.cost_today);
            s.sales_yesterday = std::mem::take(&mut s.sales_today);
            s.turned_away_yesterday = std::mem::take(&mut s.turned_away_today);
        }
        d.cost_total += d.cost_today.iter().sum::<f32>();
        d.t_yesterday = std::mem::take(&mut d.t_today);
        d.cost_yesterday = std::mem::take(&mut d.cost_today);
        d.sales_yesterday = std::mem::take(&mut d.sales_today);
        d.blocked_yesterday = std::mem::take(&mut d.blocked_today);
        d.hauled_today_t = 0.0;
    }

    // ── litter on the streets ──────────────────────────────────────────────

    /// Midnight: overflowing bins throw litter onto their street; the
    /// depots' sweepers clear the streets in their reach faster.
    pub(crate) fn daily_litter(&mut self) {
        let mut add: BTreeMap<SegmentId, f32> = BTreeMap::new();
        for (bid, bin) in &self.society.trash.bins {
            let o = bin.overflow();
            if o <= 0.0 {
                continue;
            }
            let Some(l) = self.building_lot(*bid) else { continue };
            *add.entry(l.front_segment).or_default() += LITTER_PER_OVERFLOW * (0.4 + o).min(2.0);
        }
        let depots = self.trash_depots();
        let litter = &mut self.society.trash.litter;
        let segs: Vec<SegmentId> = litter.keys().copied().chain(add.keys().copied()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        for seg in segs {
            let Some(_) = self.roads.try_segment(seg) else {
                litter.remove(&seg);
                continue;
            };
            let (a, b) = self.roads.segment_points(seg);
            let mid = (a + b) * 0.5;
            let swept = depots.iter().any(|d| d.3 > 0 && d.1.distance(mid) <= d.2);
            let v = litter.get(&seg).copied().unwrap_or(0.0) * if swept { LITTER_SWEPT } else { LITTER_KEPT } + add.get(&seg).copied().unwrap_or(0.0);
            if v < 0.02 {
                litter.remove(&seg);
            } else {
                litter.insert(seg, v.min(1.0));
            }
        }
    }

    /// Street litter at a street piece, `0..1`.
    pub fn street_litter(&self, seg: SegmentId) -> f32 {
        self.society.trash.litter.get(&seg).copied().unwrap_or(0.0)
    }

    /// Land value: littered streets and the landfills' fill.
    pub(crate) fn splat_waste(&self, b: &mut crate::land_value::LandValueBuilder) {
        for (seg, l) in &self.society.trash.litter {
            if self.roads.try_segment(*seg).is_none() {
                continue;
            }
            let (a, c) = self.roads.segment_points(*seg);
            b.splat((a + c) * 0.5, LITTER_LV_RADIUS, -LITTER_LV * l);
        }
        for s in self.waste_sites().into_iter().filter(|s| s.kind == ServiceKind::Landfill) {
            if let Some(f) = self.landfill_fill(s.id) {
                b.splat(s.pos, 300.0, -0.08 * f);
            }
        }
    }

    /// [r5-trash] The disposal sites as dirty neighbours (`World::dirty`):
    /// the tip stinks (more as it fills), the furnace smokes, the sheds are
    /// loud. Mood, land value and building next door follow from there.
    pub(crate) fn waste_dirt(&self) -> Vec<crate::industry::DirtySource> {
        use crate::industry::IndustryKind;
        self.waste_sites()
            .into_iter()
            .filter_map(|s| {
                let activity = 0.4 + 0.6 * s.staffing;
                let (as_kind, level, reach) = match s.kind {
                    ServiceKind::Landfill => {
                        let f = self.landfill_fill(s.id).unwrap_or(0.0);
                        if f <= 0.0 {
                            return None;
                        }
                        (IndustryKind::FactoryFarming, 0.45 + 0.4 * f, 260.0 + 140.0 * f)
                    }
                    ServiceKind::Incinerator => (IndustryKind::Refinery, 0.6 * activity, 320.0),
                    ServiceKind::RecyclingPlant => (IndustryKind::Mining, 0.3 * activity, 170.0),
                    _ => (IndustryKind::Mining, 0.3 * activity, 180.0),
                };
                Some(crate::industry::DirtySource { building: s.id, pos: s.pos, as_kind, level, reach })
            })
            .collect()
    }
}
