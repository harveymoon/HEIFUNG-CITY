//! [malls] Strip malls, malls and mega-malls: buildings whose shops are let
//! as units (`world/tenants.rs` does the letting, the rent and the signs).
//!
//! - *Strip malls* (LA style): two or three storeys on a commercial road
//!   plot wide and deep enough for a row of shopfronts behind a small car
//!   park for 10 to 20 cars (`Building::recompute_capacity`). Common and
//!   cheap: a new commercial building on such a plot is one by a hash of its
//!   seed, more often on a main street (two lanes or more), never at the
//!   high tier; its own business has one shopfront (or the floor above),
//!   the rest are let.
//! - *Malls*: one building of many shops round its own car park. A shop
//!   that has grown over the plots next door (G1's chains) rebuilds as one
//!   once its site is big enough ([`MALL_MIN_AREA`], `ShopGrowth::Mall`),
//!   and a new commercial building on a plot that big may be one; either
//!   way a local company owns it and its store is an anchor.
//! - *Mega-malls*: a mall its company owns, let at least
//!   `1 - MEGA_MALL_MAX_VACANT`, buys the commercial plots round it (the land
//!   market, `plots_next_to`) and rebuilds as a mega-mall: two anchor stores,
//!   a food court, a parking structure, and shops that draw people from
//!   across town ([`World::shop_reach`]).
//!
//! District stories lean on it (`Story::mall_bias`): the suburb's parades
//! and the neon strip have the most, the old town's charter none.
//!
//! No draws from the world's stream: every pick is a hash of the
//! building's seed and the day.

use super::*;
use crate::buildings::{Occupant, Unit, UnitRole};
use crate::estate::{merged_lot_within, Form, OwnerKind};
use crate::hash::hash01;

/// Street front a strip-mall shop takes, m, and the shops in a row.
pub const STRIP_SHOP_WIDTH: f32 = 7.0;
pub const STRIP_MIN_SHOPS: usize = 3;
pub const STRIP_MAX_SHOPS: usize = 8;
/// A strip mall needs this much street front and depth (the car park in
/// front of the shops), m.
pub const STRIP_MALL_MIN_FRONTAGE: f32 = 20.0;
pub const STRIP_MALL_MIN_DEPTH: f32 = 22.0;
/// Strip malls and malls go up once the town has the custom for a row of
/// shops: this many people. (A new town's first shops are its own; in the
/// demo district a row of strip malls let at once broke too many.)
pub const MALL_MIN_POP: usize = 3000;
/// Chance a new commercial building on such a plot is a strip mall: on a
/// main street (two lanes or more), elsewhere.
pub const STRIP_MALL_CHANCE_MAIN: f32 = 0.45;
pub const STRIP_MALL_CHANCE: f32 = 0.15;
/// In a town already built, an owner rebuilds an old low shop building on
/// a plot that size as a strip mall: chance a day (one a day at most) and
/// how old the building must be.
pub const STRIP_REBUILD_CHANCE: f32 = 0.5;
pub const STRIP_REBUILD_MIN_DAYS: u64 = 20;
/// Share of the plot's built area a strip mall's block takes (the car park
/// is in front of it).
pub const STRIP_MALL_COVER: f32 = 0.6;
/// A site at least this big (m²) may be a mall.
pub const MALL_MIN_AREA: f32 = 1100.0;
/// Chance a new commercial building on a plot that big is a mall.
pub const MALL_LOT_CHANCE: f32 = 0.5;
/// Floor area of a mall shop, m², and the shops in a mall.
pub const MALL_SHOP_M2: f32 = 160.0;
pub const MALL_MIN_SHOPS: usize = 6;
pub const MALL_MAX_SHOPS: usize = 16;
/// A mall's site may be this much less compact than a merge (its car park
/// fills the corners).
pub const MALL_HULL_SLACK: f32 = 1.12;
/// A mega-mall's site: at least this big (m²), at most this many plots.
pub const MEGA_MALL_MIN_AREA: f32 = 3000.0;
pub const MEGA_MALL_MAX_LOTS: usize = 8;
/// [p2-look] A mega-mall's site must be compact: its short side at least
/// this share of its long side, and at least this long (m), measured along
/// and across its road plot's front. (A forced one on a bike shop's long
/// narrow forecourt came out a cramped strip of small anchors.)
pub const MEGA_MALL_MIN_ASPECT: f32 = 0.45;
pub const MEGA_MALL_MIN_SIDE: f32 = 36.0;
/// [p2-look] How loose the block may be while a mega-mall is still buying
/// it (an L on the way to a square); the finished block is joined at
/// [`MALL_HULL_SLACK`].
pub const MEGA_MALL_GROWING_SLACK: f32 = 1.35;
/// A mall grows only while at most this share of its shops are to let...
pub const MEGA_MALL_MAX_VACANT: f32 = 0.25;
/// ... it has traded this many days...
pub const MEGA_MALL_MIN_DAYS: u64 = 7;
/// ... and then on a day with this chance (one a day across the city).
pub const MEGA_MALL_CHANCE: f32 = 0.35;
/// Shops in a mega-mall besides its two anchors and food court.
pub const MEGA_MIN_SHOPS: usize = 12;
pub const MEGA_MAX_SHOPS: usize = 28;
pub const FOOD_COURT_COUNTERS: usize = 4;
/// Mall units let across the city in a day (on top of `LETS_PER_DAY`), and
/// how much more readily than a shopfront on the street.
pub const MALL_LETS_PER_DAY: usize = 2;
pub const MALL_LET_PULL: f32 = 1.0;
/// How far people come for a shop in a mall and in a mega-mall: the
/// distance they weigh is divided by this.
pub const MALL_REACH: f32 = 1.6;
pub const MEGA_MALL_REACH: f32 = 3.0;

fn unit(lo: u8, hi: u8, share: f32, area: f32, role: UnitRole) -> Unit {
    Unit { lo, hi, share, area, shopfront: true, jobs: 0, occupant: Occupant::Vacant, vacant_since: 0, role }
}

/// The units of a strip mall, mall or mega-mall of `floors`, street front
/// `frontage` (m) and storey `plate` (m²), all to let.
pub fn mall_layout(form: Form, floors: u8, frontage: f32, plate: f32) -> Vec<Unit> {
    let mut v = Vec::new();
    match form {
        Form::StripMall => {
            // A row of shopfronts on the ground floor, offices over them.
            let plate = plate * STRIP_MALL_COVER;
            let n = ((frontage / STRIP_SHOP_WIDTH).floor() as usize).clamp(STRIP_MIN_SHOPS, STRIP_MAX_SHOPS);
            let share = 1.0 / n as f32;
            for _ in 0..n {
                v.push(unit(0, 0, share, plate * share, UnitRole::Shop));
            }
            for f in 1..floors {
                v.push(Unit { shopfront: false, ..unit(f, f, 1.0, plate, UnitRole::Shop) });
            }
        }
        Form::Mall => {
            // Shops on two levels round the court (a seventh of each floor
            // is the court and the halls).
            let n = ((plate * 2.0 / MALL_SHOP_M2).round() as usize).clamp(MALL_MIN_SHOPS, MALL_MAX_SHOPS);
            let below = n.div_ceil(2);
            for (f, k) in [(0u8, below), (1u8, n - below)] {
                if f >= floors.max(2) {
                    continue;
                }
                let share = 0.85 / k.max(1) as f32;
                for _ in 0..k {
                    v.push(unit(f, f, share, plate * share, UnitRole::Shop));
                }
            }
        }
        Form::MegaMall => {
            // Two anchors over both levels at the ends, a food court
            // upstairs, shops along the halls.
            for _ in 0..2 {
                v.push(unit(0, 1, 0.15, plate * 0.15 * 2.0, UnitRole::Anchor));
            }
            let n = ((plate * 2.0 * 0.55 / MALL_SHOP_M2).round() as usize).clamp(MEGA_MIN_SHOPS, MEGA_MAX_SHOPS);
            let below = n.div_ceil(2);
            let shops_share = [0.55f32, 0.55 - 0.03 * FOOD_COURT_COUNTERS as f32];
            for (f, k) in [(0u8, below), (1u8, n - below)] {
                let share = shops_share[f as usize] / k.max(1) as f32;
                for _ in 0..k {
                    v.push(unit(f, f, share, plate * share, UnitRole::Shop));
                }
            }
            for _ in 0..FOOD_COURT_COUNTERS {
                v.push(unit(1, 1, 0.03, plate * 0.03, UnitRole::FoodCourt));
            }
        }
        _ => {}
    }
    v
}

/// A mall's own business takes one unit: a shop an anchor (or the first
/// shopfront), anything else the floors over the shops (or a shopfront in
/// a mall that has none); with `staff`, more until they all have work.
pub fn assign_mall_main(b: &mut Building, kind: BusinessKind, staff: u16) {
    let shop = super::tenants::fits_shopfront(kind);
    let first = b
        .units
        .iter()
        .position(|u| shop && u.role == UnitRole::Anchor)
        .or_else(|| b.units.iter().position(|u| u.shopfront == shop && u.role != UnitRole::FoodCourt))
        .or_else(|| b.units.iter().position(|u| u.role != UnitRole::FoodCourt));
    let Some(first) = first else { return };
    b.units[first].occupant = Occupant::Main;
    let mut own = b.units[first].jobs;
    for i in 0..b.units.len() {
        if own >= staff {
            break;
        }
        if b.units[i].vacant() && b.units[i].role != UnitRole::FoodCourt {
            b.units[i].occupant = Occupant::Main;
            own += b.units[i].jobs;
        }
    }
}

/// [malls] Malls in town, for the probes and the market panel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MallStats {
    pub strip_malls: usize,
    pub malls: usize,
    pub mega_malls: usize,
    /// Of those, being built or rebuilt.
    pub building: usize,
    /// Shop units in them, let (to a tenant or the owner's store), to let.
    pub units: usize,
    pub let_units: usize,
    pub vacant: usize,
    /// Tenants renting in them.
    pub tenants: usize,
}

impl World {
    /// [malls] How readily malls go up on `lot`: its district's story
    /// (`Story::mall_bias`), 1 elsewhere.
    pub fn mall_bias(&self, lot: LotId) -> f32 {
        self.district_of_lot(lot).and_then(|d| d.story).map_or(1.0, |s| s.mall_bias())
    }

    /// [malls] Whether a new commercial building on `lot_id` at `tier`
    /// (seed `seed`) is a strip mall or a mall, and which.
    pub(super) fn mall_form_for(&self, lot_id: LotId, tier: Tier, seed: u64) -> Option<Form> {
        let l = self.lots.get(lot_id)?;
        if l.landlocked || !l.car_access() || tier == Tier::High || l.land_use != LandUse::Commercial || self.citizens.len() < MALL_MIN_POP {
            return None;
        }
        let bias = self.mall_bias(lot_id);
        if bias <= 0.0 {
            return None;
        }
        if l.area() >= MALL_MIN_AREA {
            return (hash01(seed, 0x3A11_0001, 0) < MALL_LOT_CHANCE * bias).then_some(Form::Mall);
        }
        let frontage = l.front_edge[0].distance(l.front_edge[1]);
        let depth = l.corners[0].distance(l.corners[3]);
        if frontage < STRIP_MALL_MIN_FRONTAGE || depth < STRIP_MALL_MIN_DEPTH {
            return None;
        }
        let main = l.car_segment.is_some_and(|s| self.roads.try_segment(s).is_some_and(|x| x.lanes() >= 2));
        let chance = if main { STRIP_MALL_CHANCE_MAIN } else { STRIP_MALL_CHANCE } * bias;
        (hash01(seed, 0x57A1_7A11, 0) < chance).then_some(Form::StripMall)
    }

    /// [malls] Is `lot_id` a plot a strip mall fits: a commercial road plot
    /// wide and deep enough, in a district that has them?
    fn strip_mall_plot(&self, lot_id: LotId) -> bool {
        let Some(l) = self.lots.get(lot_id) else { return false };
        if l.landlocked || !l.car_access() || l.land_use != LandUse::Commercial || self.mall_bias(lot_id) <= 0.0 || self.district_frozen(lot_id) {
            return false;
        }
        l.front_edge[0].distance(l.front_edge[1]) >= STRIP_MALL_MIN_FRONTAGE && l.corners[0].distance(l.corners[3]) >= STRIP_MALL_MIN_DEPTH
    }

    /// [malls] What an old low shop building on `lot` could be rebuilt as:
    /// a mall on a plot big enough (its shop the anchor), else a strip mall
    /// (a shop or an office keeps one unit), else nothing.
    fn rebuild_form(&self, b: &Building) -> Option<Form> {
        if !self.strip_mall_plot(b.lot) && !self.lots.get(b.lot).is_some_and(|l| l.area() >= MALL_MIN_AREA && !l.landlocked && l.car_access() && l.land_use == LandUse::Commercial && self.mall_bias(b.lot) > 0.0 && !self.district_frozen(b.lot)) {
            return None;
        }
        let kind = b.business.as_ref().filter(|z| !z.closed).map(|z| z.kind);
        let shop = kind.is_some_and(super::tenants::fits_shopfront);
        if shop && self.lots.get(b.lot).is_some_and(|l| l.area() >= MALL_MIN_AREA) {
            return Some(Form::Mall);
        }
        (self.strip_mall_plot(b.lot) && (shop || kind.is_none_or(|k| k == BusinessKind::Office))).then_some(Form::StripMall)
    }

    /// [malls] An old low commercial building (at most three floors, not
    /// the high tier, standing [`STRIP_REBUILD_MIN_DAYS`]) nobody rents in,
    /// that the market may buy and that could be rebuilt as a mall or
    /// strip mall (`rebuild_form`).
    pub fn mall_rebuild_candidate(&self, id: BuildingId, b: &Building) -> bool {
        b.land_use == LandUse::Commercial
            && !b.is_special()
            && b.plot.form.is_none()
            && !b.plot.merged()
            && b.plot.tower.is_none()
            && b.operating()
            && b.floors <= 3
            && b.tier != Tier::High
            && self.clock.tick.saturating_sub(b.built_tick) >= STRIP_REBUILD_MIN_DAYS * 1440
            && self.tenants_of(id).is_empty()
            && self.buyable(id)
            && self.rebuild_form(b).is_some()
    }

    /// [malls] Once a day (by hash, [`STRIP_REBUILD_CHANCE`]): in a town
    /// already built, the owner of an old low shop or office building (or
    /// an empty one) on a strip-mall plot, with nobody renting in it,
    /// rebuilds it as a strip mall, its own business keeping one unit and
    /// the rest let; a shop on a plot big enough for a mall rebuilds as one
    /// (its company's, its store the anchor). True if one began.
    pub(super) fn rebuild_strip_malls(&mut self) -> bool {
        let day = self.clock.day();
        if hash01(self.config.seed, day, 0x57C0) >= STRIP_REBUILD_CHANCE || self.citizens.len() < MALL_MIN_POP {
            return false;
        }
        let tick = self.clock.tick;
        let pick = self
            .buildings
            .iter()
            .filter(|(id, b)| self.mall_rebuild_candidate(*id, b))
            .map(|(id, b)| (hash01(b.seed, day, 0x57C1), id))
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let Some((_, bid)) = pick else { return false };
        let b = &self.buildings[bid];
        let Some(form) = self.rebuild_form(b) else { return false };
        let (name, lot, seed) = (b.name.clone(), b.lot, b.seed);
        let keep = b.business.as_ref().is_some_and(|z| !z.closed);
        let tier = b.tier.min(Tier::Mid);
        let value = self.lot_value(lot);
        self.buildings[bid].plot.form = Some(form);
        let floors = if form == Form::Mall { 2 } else { 2 + (crate::hash::mix(seed, 0x57A1) % 2) as u8 };
        if form == Form::Mall {
            let _ = self.firm_of(bid);
            self.estate.stats.malls_total += 1;
        } else {
            self.estate.stats.strip_malls_rebuilt_total += 1;
        }
        self.redevelop_with(bid, tier, floors, value, format!("{name} is coming down for a {}.", form.label()), keep);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Redevelopment);
        let pos = self.lots.get(lot).map_or(Vec2::ZERO, |l| l.center);
        self.skyline.push_news(tick, format!("{name} is being rebuilt as a {}", form.label()), pos, Some(bid));
        true
    }

    /// [malls] How far people will come for the shop at `bid`: a shop in a
    /// mall draws from further, one in a mega-mall from across town (the
    /// distance they weigh is divided by this).
    pub fn shop_reach(&self, bid: BuildingId) -> f32 {
        match self.buildings.get(self.host_of(bid)).and_then(|h| h.plot.form) {
            Some(Form::MegaMall) => MEGA_MALL_REACH,
            Some(Form::Mall) => MALL_REACH,
            _ => 1.0,
        }
    }

    /// [p2-look] The distance (m) over which people weigh coming to the
    /// shop at `bid` (`choose_shop` divides the walk or drive by it): a
    /// street shop's, times its mall's pull (`shop_reach`).
    pub fn shopper_reach_m(&self, bid: BuildingId) -> f32 {
        super::places::REACH_SHOP * self.shop_reach(bid)
    }

    /// [malls] A shop chain's store whose site is big enough to rebuild as a
    /// mall (`ShopGrowth::Mall`): commercial, not one already, on a road
    /// plot, its plots at least [`MALL_MIN_AREA`] together, in a district
    /// that has malls.
    pub fn mall_site_ok(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        let Some(l) = self.lots.get(b.lot) else { return false };
        if l.land_use != LandUse::Commercial || l.landlocked || b.plot.form.is_some_and(|f| f.is_mall()) || b.plot.tower.is_some() || !b.plot.pending_lots.is_empty() {
            return false;
        }
        if self.mall_bias(b.lot) <= 0.0 || self.district_frozen(b.lot) {
            return false;
        }
        let area: f32 = b.plot.lots(b.lot).iter().filter_map(|x| self.lots.get(*x)).map(|x| x.area()).sum();
        area >= MALL_MIN_AREA
    }

    /// [malls] The store at `bid` rebuilds as a mall over its plots: its
    /// own store an anchor, the rest of the shops let; its company owns it.
    /// True if it began.
    pub fn build_mall(&mut self, bid: BuildingId) -> bool {
        if !self.mall_site_ok(bid) || self.buildings[bid].construction.is_some() {
            return false;
        }
        // Its company owns it (made now for a store that had none); the
        // owner's history says so where the growth step is taken.
        let _ = self.firm_of(bid);
        let b = &self.buildings[bid];
        let (name, lot) = (b.name.clone(), b.lot);
        let tier = b.tier.max(Tier::for_land_value(self.lot_value(lot))).min(Tier::Mid);
        let value = self.lot_value(lot);
        self.buildings[bid].plot.form = Some(Form::Mall);
        self.redevelop_with(bid, tier, 2, value, format!("{name} is closed while it rebuilds as a mall."), true);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Expansion);
        let tick = self.clock.tick;
        let pos = self.lots.get(lot).map_or(Vec2::ZERO, |l| l.center);
        self.skyline.push_news(tick, format!("{name} is rebuilding as a mall"), pos, Some(bid));
        self.estate.stats.malls_total += 1;
        true
    }

    /// [malls] A mall doing well enough to grow: owned by a local company,
    /// open at least [`MEGA_MALL_MIN_DAYS`], at most
    /// [`MEGA_MALL_MAX_VACANT`] of its shops to let.
    pub fn mall_thriving(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        if b.plot.form != Some(Form::Mall) || !b.operating() || b.is_derelict() || !b.plot.pending_lots.is_empty() {
            return false;
        }
        if self.clock.tick.saturating_sub(b.built_tick) < MEGA_MALL_MIN_DAYS * 1440 {
            return false;
        }
        let local = b.plot.owner.and_then(|o| self.estate.owners.get(o)).is_some_and(|o| matches!(o.kind, OwnerKind::Firm { .. } | OwnerKind::Person { .. } | OwnerKind::Company { .. } | OwnerKind::Builder { .. }));
        let shops = b.units.iter().filter(|u| u.shopfront).count();
        let vacant = b.units.iter().filter(|u| u.shopfront && u.vacant()).count();
        local && shops >= 4 && (vacant as f32) <= shops as f32 * MEGA_MALL_MAX_VACANT
    }

    /// [malls] Once a day: one thriving mall (by hash) buys the land round
    /// it and grows into a mega-mall.
    pub(super) fn grow_malls(&mut self) {
        let day = self.clock.day();
        let malls: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.plot.form == Some(Form::Mall)).map(|(id, _)| id).collect();
        for bid in malls {
            let seed = self.buildings[bid].seed;
            if hash01(seed, day, 0x3E6A) >= MEGA_MALL_CHANCE || !self.mall_thriving(bid) {
                continue;
            }
            if self.grow_mega_mall(bid) {
                break;
            }
        }
    }

    /// [p2-look] A site's extent along its road plot's front and across it
    /// (m): the box its plots fill, turned with the front.
    pub fn site_extent(&self, primary: LotId, lots: &[LotId]) -> (f32, f32) {
        let Some(p) = self.lots.get(primary) else { return (0.0, 0.0) };
        let along = Vec2::new(-p.front_dir.y, p.front_dir.x);
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for l in std::iter::once(&primary).chain(lots.iter()).filter_map(|l| self.lots.get(*l)) {
            for q in &l.polygon {
                let d = Vec2::new((*q - p.center).dot(along), (*q - p.center).dot(p.front_dir));
                lo = lo.min(d);
                hi = hi.max(d);
            }
        }
        let size = (hi - lo).max(Vec2::ZERO);
        (size.x, size.y)
    }

    /// [p2-look] Whether a mega-mall fits a site of these plots: compact
    /// enough ([`MEGA_MALL_MIN_ASPECT`], [`MEGA_MALL_MIN_SIDE`]) for its
    /// anchors, concourse and parking deck round a car park.
    pub fn mega_mall_shape_ok(&self, primary: LotId, lots: &[LotId]) -> bool {
        let (w, d) = self.site_extent(primary, lots);
        w.min(d) >= MEGA_MALL_MIN_SIDE && w.min(d) >= w.max(d) * MEGA_MALL_MIN_ASPECT
    }

    /// [malls] The plots round the building at `bid` that would make its
    /// site a mega-mall's, or None if they cannot reach
    /// [`MEGA_MALL_MIN_AREA`] on a compact site (`mega_mall_shape_ok`).
    fn mega_mall_plots(&self, bid: BuildingId, relaxed: bool) -> Option<Vec<LotId>> {
        // [p2-look] Two ways of buying the block, the first that gives a
        // compact one drawn whole: towards a square (on small plots: taking
        // the squarest step each turn runs along the street, a road plot
        // beside being squarer than one behind, into a strip), else the
        // squarest step each turn (on big plots, where a square's L on the
        // way would not join).
        self.mega_mall_plots_by(bid, relaxed, true).or_else(|| self.mega_mall_plots_by(bid, relaxed, false))
    }

    /// [p2-look] `mega_mall_plots` one way: `square`, each plot the one
    /// that brings the block's sides nearest a square of the size wanted
    /// (the block may be an L on the way, `MEGA_MALL_GROWING_SLACK`); else
    /// the one that leaves it most compact (joined at `MALL_HULL_SLACK`
    /// every step). The cheapest of those near as good either way. The
    /// finished block must join at `MALL_HULL_SLACK`, the slack
    /// `World::footprint` draws it at (a forced mega-mall bought at a looser
    /// one fell back to its road plot alone, a narrow forecourt, and came
    /// out a cramped box), and pass `mega_mall_shape_ok`.
    fn mega_mall_plots_by(&self, bid: BuildingId, relaxed: bool, square: bool) -> Option<Vec<LotId>> {
        let b = self.buildings.get(bid)?;
        let ours = b.plot.lots(b.lot);
        let owner = b.plot.owner;
        let lu = self.lots.get(b.lot).map_or(b.land_use, |l| l.land_use);
        let primary = self.lots.get(b.lot)?;
        let mut area: f32 = ours.iter().filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum();
        let mut picked: Vec<LotId> = Vec::new();
        let floors = if relaxed { 12 } else { 4 };
        let slack = if square { MEGA_MALL_GROWING_SLACK } else { MALL_HULL_SLACK };
        let side = (MEGA_MALL_MIN_AREA * 1.2).sqrt();
        while area < MEGA_MALL_MIN_AREA && ours.len() + picked.len() < MEGA_MALL_MAX_LOTS {
            let mut cur = ours.clone();
            cur.extend(picked.iter().copied());
            // (score, lot): lower is better.
            let fits: Vec<(f32, LotId)> = self
                .plots_next_to(&cur, lu, floors, owner)
                .into_iter()
                .map(|(_, l)| l)
                .filter(|l| {
                    let all: Vec<&Lot> = cur.iter().skip(1).chain(std::iter::once(l)).filter_map(|x| self.lots.get(*x)).collect();
                    merged_lot_within(primary, &all, slack).is_some()
                })
                .map(|l| {
                    let mut with: Vec<LotId> = cur[1..].to_vec();
                    with.push(l);
                    let (w, d) = self.site_extent(b.lot, &with);
                    let score = if square { (w - side).abs() + (d - side).abs() } else { -(w.min(d) / w.max(d).max(1e-3)) * 50.0 };
                    (score, l)
                })
                .collect();
            let best = fits.iter().map(|x| x.0).fold(f32::MAX, f32::min);
            let next = fits.iter().find(|x| x.0 <= best + 5.0).map(|x| x.1);
            let Some(l) = next else { break };
            area += self.lots[l].area();
            picked.push(l);
        }
        let mut site: Vec<LotId> = ours[1..].to_vec();
        site.extend(picked.iter().copied());
        let all: Vec<&Lot> = site.iter().filter_map(|x| self.lots.get(*x)).collect();
        let drawn = merged_lot_within(primary, &all, MALL_HULL_SLACK).is_some();
        (area >= MEGA_MALL_MIN_AREA && !picked.is_empty() && drawn && self.mega_mall_shape_ok(b.lot, &site)).then_some(picked)
    }

    /// [malls] The mall at `bid` buys the commercial plots round it (built
    /// ones the market may buy, landlocked backland too: it touches the
    /// mall) until its site reaches [`MEGA_MALL_MIN_AREA`], and rebuilds as
    /// a mega-mall. True if it bought; nothing changes if it cannot reach
    /// the size. (Public so a probe can force one.)
    pub fn grow_mega_mall(&mut self, bid: BuildingId) -> bool {
        self.grow_mega_mall_with(bid, false)
    }

    /// [malls] `grow_mega_mall`; `relaxed` (a forced one, for screenshots)
    /// buys taller neighbours.
    fn grow_mega_mall_with(&mut self, bid: BuildingId, relaxed: bool) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        if b.plot.form != Some(Form::Mall) || !b.operating() || b.is_tenant() || !b.plot.pending_lots.is_empty() {
            return false;
        }
        let owner = b.plot.owner;
        let Some(picked) = self.mega_mall_plots(bid, relaxed) else { return false };
        let price = self.plots_price(&picked);
        let b = &self.buildings[bid];
        let (name, lot) = (b.name.clone(), b.lot);
        let tier = b.tier.max(Tier::Mid);
        let value = self.lot_value(lot);
        self.buildings[bid].plot.form = Some(Form::MegaMall);
        let memory = format!("{name} bought the block to grow into a mega-mall. Our building is coming down.");
        self.take_plots(bid, &picked, &memory);
        self.redevelop_with(bid, tier, 2, value, format!("{name} is closed while it grows into a mega-mall."), true);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Expansion);
        if let Some(o) = owner.filter(|o| self.estate.owners.contains_key(*o)) {
            self.estate.owners[o].spent += price;
            self.note_owner(o, format!("bought {} plots to grow {name} into a mega-mall", picked.len()));
        }
        // [ledger] The agent's fee, from the anchor store's till.
        if let Some(z) = self.buildings.get(bid).and_then(|b| b.business.as_ref()) {
            let fee = (price as f32 * 0.01).min(z.money.max(0.0));
            self.transfer(Account::Business(bid), Account::Outside, fee, Kind::PropertySale);
        }
        let tick = self.clock.tick;
        let pos = self.lots.get(lot).map_or(Vec2::ZERO, |l| l.center);
        self.skyline.push_news(tick, format!("{name} buys up the block to become a mega-mall"), pos, Some(bid));
        self.estate.stats.mega_malls_total += 1;
        true
    }

    /// [malls] For probes and screenshots: the best-let mall (or, with none,
    /// one made at once of the biggest shop's site) grows into a mega-mall
    /// at once, opened. Its id, or None if no mall could grow.
    #[doc(hidden)]
    pub fn force_mega_mall(&mut self) -> Option<BuildingId> {
        let mut malls: Vec<(usize, BuildingId)> = self.buildings.iter().filter(|(id, b)| b.plot.form == Some(Form::Mall) && !b.is_tenant() && self.mega_mall_plots(*id, true).is_some()).map(|(id, b)| (b.vacant_units(), id)).collect();
        malls.sort();
        let bid = match malls.first() {
            Some(&(_, b)) => b,
            None => self.force_mall(None, true)?,
        };
        if self.buildings[bid].construction.is_some() {
            self.finish_construction(bid);
        }
        if !self.grow_mega_mall_with(bid, true) {
            return None;
        }
        self.finish_construction(bid);
        Some(bid)
    }

    /// [malls] For probes and screenshots: the shop with the biggest site
    /// (not `except`) rebuilds as a mall at once, opened.
    #[doc(hidden)]
    pub fn force_mall(&mut self, except: Option<BuildingId>, room_to_grow: bool) -> Option<BuildingId> {
        let shop = |b: &Building| b.land_use == LandUse::Commercial && b.operating() && !b.is_special() && b.plot.tower.is_none() && b.plot.form.is_none() && b.business.as_ref().is_some_and(|z| super::tenants::fits_shopfront(z.kind));
        let c = self
            .buildings
            .iter()
            .filter(|(id, b)| Some(*id) != except && shop(b) && self.lots.get(b.lot).is_some_and(|l| !l.landlocked && l.car_access()) && (!room_to_grow || self.mega_mall_plots(*id, true).is_some()))
            .map(|(id, _)| (self.footprint(id).map_or(0.0, |l| l.area()), id))
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))?
            .1;
        self.buildings[c].plot.form = Some(Form::Mall);
        let _ = self.firm_of(c);
        let (tier, value, name) = (self.buildings[c].tier.min(Tier::Mid), self.lot_value(self.buildings[c].lot), self.buildings[c].name.clone());
        self.redevelop_with(c, tier, 2, value, format!("{name} is closed while it rebuilds as a mall."), true);
        self.finish_construction(c);
        Some(c)
    }

    /// [malls] Strip malls, malls and mega-malls in town, their shop units
    /// and tenants.
    pub fn mall_stats(&self) -> MallStats {
        let mut s = MallStats::default();
        for (id, b) in self.buildings.iter() {
            let Some(form) = b.plot.form.filter(|f| f.is_mall()) else { continue };
            if b.is_tenant() {
                continue;
            }
            match form {
                Form::StripMall => s.strip_malls += 1,
                Form::Mall => s.malls += 1,
                _ => s.mega_malls += 1,
            }
            if !b.operating() {
                s.building += 1;
            }
            let shops = b.units.iter().filter(|u| u.shopfront);
            for u in shops {
                s.units += 1;
                if u.vacant() {
                    s.vacant += 1;
                } else {
                    s.let_units += 1;
                }
            }
            s.tenants += self.tenants_of(id).len();
        }
        s
    }
}
