//! [economy] The World side of business pressure (`crate::pressure`):
//! reading the city's needs once a day, opening the business a pressure
//! asks for on a lot that suits it, the trades that live on jobs rather
//! than goods (repair shops, bike shops), the art scene (artists, studios,
//! galleries, openings and donations), and why each business is where it
//! is and how it is doing, for the inspector and the API.
//!
//! Hooks elsewhere (one line each): `develop.rs` asks
//! [`World::open_by_pressure`] for a business a round; `mod.rs` runs
//! [`World::daily_pressure`] after the daily update and
//! [`World::trades_hourly`] on the hour; `cars.rs` counts long walks
//! ([`World::note_long_walk`]) and sends car and bike buyers to dealers and
//! bike shops first; `market.rs` (`venue_tabs`) charges at every place to
//! go out; `places.rs` draws people to an opening.
//!
//! Deterministic: no draw from the world's RNG. Choices are by score with
//! ties to the lower id, and the few chances use hashes of ids and the day.

use super::*;
use crate::pressure::{self, assess, Opening, PressureKind, Signals, Supply};

/// What a wreck repair and a service bring a repair shop.
pub const REPAIR_FEE: f32 = 90.0;
pub const SERVICE_FEE: f32 = 30.0;
/// Bike owners per bike repair a day, and its fee.
pub const BIKE_OWNERS_PER_REPAIR: f32 = 25.0;
pub const BIKE_REPAIR_FEE: f32 = 12.0;
/// What a wealthy household pays its dealership a day: the lease, the
/// valet service.
pub const DEALER_LEASE: f32 = 2.0;
/// Shop hours over which a day's repairs are spread.
const TRADE_HOURS: f32 = 10.0;
/// Share of adults with an artist's calling.
pub const ARTIST_PER_MILLE: u64 = 45;
/// Artists who move a day towards cheap rooms near the warehouses, and
/// who take a studio job.
const ARTIST_MOVES_PER_DAY: usize = 2;
const ARTIST_HIRES_PER_DAY: usize = 6;
/// Invitations sent per opening.
const INVITES: usize = 18;
/// How far a gallery's invitations go.
const INVITE_REACH: f32 = 2500.0;
/// Within this of the shore a wealthy household might keep a boat.
pub const NEAR_WATER: f32 = 900.0;

/// A stable `u64` from a key and a salt.
fn mix(key: u64, salt: u64) -> u64 {
    let mut x = key ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 29;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^ (x >> 32)
}

fn key_of<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

/// [economy] Does this seed carry an artist's calling? About 4.5% of people.
pub fn artist_seed(seed: u64) -> bool {
    mix(seed, 0xA27_15) % 1000 < ARTIST_PER_MILLE
}

/// [economy] An adult with an artist's calling: the art scene's people.
pub fn is_artist(c: &crate::citizens::Citizen) -> bool {
    c.stage() == crate::citizens::LifeStage::Adult && artist_seed(c.seed)
}

/// Where a pressure puts its business.
struct Placement {
    uses: &'static [LandUse],
    /// Points the lot should be near; empty: anywhere.
    anchors: Vec<Vec2>,
    /// No farther than this from the nearest anchor.
    reach: f32,
    min_area: f32,
    /// Prefers cheap land (<0), dear land (>0) or does not mind (0).
    value: f32,
    needs_street: bool,
    /// [r4-hunger] What the anchors are, for the player ("a fishing port").
    near: &'static str,
}

/// [r4-hunger] The lot a pressure's business needs ([`World::lot_need`]),
/// from its placement: for the player, so a held pressure says what to zone.
#[derive(Clone, Debug, PartialEq)]
pub struct LotNeed {
    /// The land uses it takes, the one it likes best first.
    pub uses: Vec<LandUse>,
    /// Square metres; 0 for any size.
    pub min_area: f32,
    /// On a street cars can use (not a footpath).
    pub needs_street: bool,
    /// On the water, between a road and the shore.
    pub waterfront: bool,
    /// The points it should be near.
    pub anchors: Vec<Vec2>,
    /// No farther than this from the nearest anchor; None: anywhere, best
    /// near them.
    pub reach: Option<f32>,
    /// What the anchors are ("a fishing port").
    pub near: &'static str,
}

impl LotNeed {
    /// "industrial", "commercial or entertainment".
    pub fn uses_text(&self) -> String {
        self.uses.iter().map(|u| u.label().to_lowercase()).collect::<Vec<_>>().join(" or ")
    }

    /// "needs a vacant industrial lot of 500 m² or more on a street within
    /// 1.4 km of a fishing port".
    pub fn text(&self) -> String {
        let mut t = format!("needs a vacant {} lot", self.uses_text());
        if self.min_area > 0.0 {
            t += &format!(" of {:.0} m² or more", self.min_area);
        }
        t += match (self.waterfront, self.needs_street) {
            (true, true) => " on the water and on a street",
            (true, false) => " on the water",
            (false, true) => " on a street",
            (false, false) => "",
        };
        match self.reach {
            Some(r) if r >= 1000.0 => t += &format!(" within {:.1} km of {}", r / 1000.0, self.near),
            Some(r) => t += &format!(" within {:.0} m of {}", r, self.near),
            None => t += &format!(", best near {}", self.near),
        }
        t
    }
}

/// [r4-hunger] How a vacant lot measures up to a pressure's business
/// ([`World::lot_need_lots`]): it suits, or the first thing it fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LotFit {
    Suits,
    /// Built on, promised or under a yard.
    Taken,
    WrongUse,
    NotOnWater,
    NoJettyRoom,
    TooSmall,
    NoStreet,
    TooFar,
}

impl LotFit {
    pub fn key(self) -> &'static str {
        match self {
            LotFit::Suits => "suits",
            LotFit::Taken => "taken",
            LotFit::WrongUse => "wrong_use",
            LotFit::NotOnWater => "not_on_water",
            LotFit::NoJettyRoom => "no_jetty_room",
            LotFit::TooSmall => "too_small",
            LotFit::NoStreet => "no_street",
            LotFit::TooFar => "too_far",
        }
    }

    /// "too small", for "vacant industrial lots: 3 too small".
    pub fn label(self) -> &'static str {
        match self {
            LotFit::Suits => "suit",
            LotFit::Taken => "taken",
            LotFit::WrongUse => "of another land use",
            LotFit::NotOnWater => "not on the water",
            LotFit::NoJettyRoom => "with no room for a jetty",
            LotFit::TooSmall => "too small",
            LotFit::NoStreet => "on a footpath, not a street",
            LotFit::TooFar => "too far",
        }
    }
}

/// Why a business is where it is and how it is doing, for the inspector
/// and the API.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BusinessStory {
    pub why: String,
    pub doing: String,
    pub struggling: bool,
    pub wants_expand: bool,
}

impl World {
    // ── reading the city ────────────────────────────────────────────────

    /// The class of a household: the mean income of its earners.
    pub(super) fn household_class(&self, hid: crate::citizens::HouseholdId) -> Tier {
        let Some(hh) = self.households.get(hid) else { return Tier::Low };
        let (sum, n) = hh.members.iter().filter_map(|m| self.citizens.get(*m)).filter(|c| c.income > 0.0).fold((0.0f32, 0u32), |(s, n), c| (s + c.income, n + 1));
        Tier::for_income(if n == 0 { 0.0 } else { sum / n as f32 })
    }

    /// A working dock of this kind whose operator is open.
    fn working_docks(&self, f: impl Fn(crate::ports::PortKind) -> bool) -> Vec<(BuildingId, crate::ports::PortKind, Vec2)> {
        self.ports
            .ports
            .values()
            .filter(|p| f(p.kind) && p.works())
            .filter_map(|p| p.building.map(|b| (b, p.kind, p.center)))
            .filter(|(b, _, _)| self.buildings.get(*b).is_some_and(|x| x.operating() && x.business.as_ref().is_none_or(|z| !z.closed)))
            .collect()
    }

    /// A day's output of the open businesses of `kinds`, at their staffing.
    fn daily_output(&self, kinds: &[BusinessKind]) -> f32 {
        self.buildings
            .values()
            .filter(|b| b.operating())
            .filter_map(|b| b.business.as_ref().filter(|z| !z.closed && kinds.contains(&z.kind)).map(|z| (b, z)))
            .map(|(b, z)| {
                let per = z.kind.recipe().outputs.first().map_or(0.0, |o| o.1);
                per * b.workers.len() as f32 * crate::market::JOB_RATE * 24.0
            })
            .sum()
    }

    /// Today's raw signals (flows as counts since yesterday).
    fn read_signals(&mut self) -> Signals {
        let st = &self.enforcement.stats;
        let (out_total, tows_total) = (st.tows_out_of_town_total, st.tows_total);
        let crashes_total = self.driving.crashes_total;
        let ev = &self.vehicles.stats.events_total;
        let bought_in = ev.get(crate::vehicle::CarEvent::Bought.label()).copied().unwrap_or(0);
        let bought_out = ev.get(crate::vehicle::CarEvent::BoughtOutOfTown.label()).copied().unwrap_or(0);
        let p = &mut self.pressure;
        let first = !p.primed;
        let delta = |now: u32, last: u32| if first { 0.0 } else { now.saturating_sub(last) as f32 };
        let outside = delta(out_total, p.last_outside_tows);
        let all_tows = delta(tows_total, p.last_tows);
        let crashes = delta(crashes_total, p.last_crashes);
        let bought = delta(bought_in + bought_out, p.last_bought);
        let bought_outside = delta(bought_out, p.last_bought_outside);
        p.last_outside_tows = out_total;
        p.last_tows = tows_total;
        p.last_crashes = crashes_total;
        p.last_bought = bought_in + bought_out;
        p.last_bought_outside = bought_out;
        p.primed = true;
        let long_walks = std::mem::take(&mut p.long_walks_today) as f32;
        let bikes_bought = self.vehicles.stats.bikes_bought_today as f32;

        let mut rich_households = 0u32;
        let mut rich_near_water = 0u32;
        let l = &self.landscape;
        let has_water = !(l.rivers.is_empty() && l.lakes.is_empty() && l.sea.is_none());
        for (hid, hh) in self.households.iter() {
            if self.household_class(hid) != Tier::High {
                continue;
            }
            rich_households += 1;
            if has_water {
                if let Some(at) = hh.home.and_then(|h| self.building_lot(h)).map(|l| l.center) {
                    if self.landscape.water_distance(at) <= NEAR_WATER {
                        rich_near_water += 1;
                    }
                }
            }
        }
        let (mut comfortable, mut rich_adults, mut artists, mut artists_mid, mut bike_owners) = (0u32, 0u32, 0u32, 0u32, 0u32);
        for c in self.citizens.values() {
            if c.owns_bike {
                bike_owners += 1;
            }
            if !c.can_work() {
                continue;
            }
            let class = c.class();
            if class >= Tier::Mid {
                comfortable += 1;
            }
            if class == Tier::High {
                rich_adults += 1;
            }
            if is_artist(c) {
                artists += 1;
                if class >= Tier::Mid {
                    artists_mid += 1;
                }
            }
        }
        let bins = &self.society.trash.bins;
        let uncovered = bins.values().filter(|b| !b.covered).count() as f32;
        let overflowing = bins.values().filter(|b| b.overflow() > 0.0).count() as f32;
        let depots = self.trash_depots().iter().filter(|d| d.3 > 0).count() as f32;
        let fishing = self.working_docks(|k| k.is_fishing());
        let marinas = self.working_docks(|k| k.is_marina());
        Signals {
            outside_tows: outside,
            local_tows: (all_tows - outside).max(0.0),
            tow_backlog: (self.enforcement.jobs.iter().filter(|j| matches!(j.state, crate::enforcement::TowState::Waiting { .. })).count() as u32 + self.enforcement.stats.tows_declined_today) as f32,
            crashes,
            cars: self.vehicles.cars.len() as f32,
            rich_households: rich_households as f32,
            cars_bought: bought,
            cars_bought_outside: bought_outside,
            trash_uncovered: uncovered,
            trash_overflowing: overflowing,
            depots,
            bike_owners: bike_owners as f32,
            bikes_bought,
            long_walks,
            livestock: self.daily_output(&[BusinessKind::CowFarm, BusinessKind::ChickenFarm, BusinessKind::Feedlot]),
            fish: self.daily_output(&[BusinessKind::FishingPort, BusinessKind::Fishery]),
            fillets: self.daily_output(&[BusinessKind::FishGutting]),
            fishing_small: fishing.iter().filter(|f| !f.1.is_large()).count() as f32,
            fishing_large: fishing.iter().filter(|f| f.1.is_large()).count() as f32,
            marinas_small: marinas.iter().filter(|f| !f.1.is_large()).count() as f32,
            marinas_large: marinas.iter().filter(|f| f.1.is_large()).count() as f32,
            comfortable_adults: comfortable as f32,
            artists: artists as f32,
            artists_mid: artists_mid as f32,
            rich_adults: rich_adults as f32,
            rich_near_water: rich_near_water as f32,
            people: self.citizens.len() as f32,
            has_water,
            // [waterfront]
            tourists: self.tourism.tourists.len() as f32,
            beach_visits: self.shore.beaches.values().map(|b| b.visits_yesterday as f32).sum(),
            beaches_open: self.shore.beaches.values().filter(|b| b.usable()).count() as f32,
            beach_appeal: self.beach_appeal_season(),
            waterfront_sites: self.lots.values().filter(|l| l.building.is_none() && l.usable() && l.shore.is_waterfront() && !l.shore.beach && l.land_use != LandUse::Park).count() as f32,
        }
    }

    /// What stands of each kind.
    pub fn pressure_supply(&self) -> Supply {
        let mut s = Supply::default();
        for b in self.buildings.values() {
            let Some(z) = &b.business else { continue };
            if z.closed {
                continue;
            }
            let i = z.kind.index();
            if b.construction.is_some() {
                s.building[i] += 1;
            } else {
                s.open[i] += 1;
                if z.broke_days > 0 || z.losing_days(5) >= 3 {
                    s.struggling[i] += 1;
                }
            }
        }
        s
    }

    /// [economy] Open a business of `kind` on a vacant lot now, as a
    /// pressure would (for scripts, tests and the remote API): the site
    /// goes up, and with `finish` it is built at once. A kind that needs
    /// a dock is placed with the Build window instead.
    pub fn open_business(&mut self, lot: LotId, kind: BusinessKind, finish: bool) -> Result<BuildingId, String> {
        if matches!(kind, BusinessKind::Marina | BusinessKind::FishingPort) {
            return Err("marinas and fishing ports are docks: place them at the water's edge (Build > Water, or place_port)".into());
        }
        let l = self.lots.get(lot).ok_or("no such lot")?;
        if l.building.is_some() {
            return Err("the lot is built on".into());
        }
        // [waterfront] The shore's trades stand on the water.
        if kind.wants_waterfront() && !l.shore.is_waterfront() {
            return Err(format!("{} must stand on a lot that runs down to the water", crate::names::a(kind.label())));
        }
        let id = self.build_on_as(lot, Some(kind)).ok_or("nothing can be built on that lot")?;
        if let Some(z) = self.buildings[id].business.as_mut() {
            z.origin = Some(format!("Opened by hand as {}.", crate::names::a(kind.label())));
        }
        if finish {
            self.finish_construction(id);
        }
        Ok(id)
    }

    // ── the daily reading ───────────────────────────────────────────────

    /// Midnight: the trades' bills, the signals, the pressures, the
    /// waterfront's fees and the art scene.
    pub(super) fn daily_pressure(&mut self) {
        self.pressure.pad();
        self.settle_trade_bills();
        // Businesses that went under today cool their kind down: shut
        // tonight, their count of days shut not yet started
        // (`daily_businesses` closes with `broke_days` reset to 0 and counts
        // it up each night after).
        let closed: Vec<BusinessKind> = self.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| z.closed && z.broke_days == 0).map(|z| z.kind).collect();
        for k in closed {
            if let Some(p) = PressureKind::of_business(k) {
                self.pressure.cooldown[p.index()] = pressure::COOLDOWN_DAYS;
            }
        }
        let today = self.read_signals();
        self.pressure.signals.blend(&today);
        let supply = self.pressure_supply();
        let mut readings = assess(&self.pressure.signals, &supply);
        for r in readings.iter_mut() {
            let i = r.kind.index();
            let cooling = self.pressure.cooldown[i];
            if r.wants_more() && cooling > 0 && r.held.is_none() {
                r.held = Some(format!("one closed for lack of trade lately: {cooling} days before another is tried"));
            }
            let counts = r.wants_more() && !r.kind.player_placed() && cooling == 0;
            self.pressure.held_days[i] = if counts { self.pressure.held_days[i].saturating_add(1) } else { 0 };
            self.pressure.cooldown[i] = cooling.saturating_sub(1);
        }
        self.pressure.readings = readings;
        let s = &mut self.scene;
        s.repairs_today = 0.0;
        s.bike_repairs_today = 0.0;
        s.fish_landed_today = 0.0;
        s.berth_fees_today = 0.0;
        self.dealer_leases();
        self.waterfront_daily();
        self.art_scene_daily();
    }

    /// The day's repairs are paid by the households with a car (a wreck's
    /// owner, a service now and then: spread evenly), the bike repairs by
    /// those with a bike. Who cannot pay is let off.
    fn settle_trade_bills(&mut self) {
        let (car_bill, bike_bill) = (std::mem::take(&mut self.pressure.car_bill_today), std::mem::take(&mut self.pressure.bike_bill_today));
        if car_bill <= 0.0 && bike_bill <= 0.0 {
            return;
        }
        let mut with_car: Vec<crate::citizens::HouseholdId> = Vec::new();
        let mut with_bike: Vec<crate::citizens::HouseholdId> = Vec::new();
        for c in self.citizens.values() {
            let Some(h) = c.household else { continue };
            if c.car.is_some() && with_car.last() != Some(&h) {
                with_car.push(h);
            }
            if c.owns_bike && with_bike.last() != Some(&h) {
                with_bike.push(h);
            }
        }
        with_car.sort();
        with_car.dedup();
        with_bike.sort();
        with_bike.dedup();
        for (list, bill) in [(with_car, car_bill), (with_bike, bike_bill)] {
            if list.is_empty() || bill <= 0.0 {
                continue;
            }
            let each = bill / list.len() as f32;
            for h in list {
                if self.households.get(h).is_some_and(|hh| hh.money > each * 5.0) {
                    // [ledger] The shops were paid as they worked (from
                    // outside); the bill settles it.
                    self.transfer(Account::Household(h), Account::Outside, each, Kind::Fees);
                }
            }
        }
    }

    /// The wealthy lease and have their cars looked after at the nearest
    /// dealership in reach.
    fn dealer_leases(&mut self) {
        let dealers: Vec<(BuildingId, Vec2)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::CarDealer && !z.closed))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center)))
            .collect();
        if dealers.is_empty() {
            return;
        }
        let rich: Vec<(crate::citizens::HouseholdId, Vec2)> = self
            .households
            .iter()
            .filter(|(h, _)| self.household_class(*h) == Tier::High)
            .filter_map(|(h, hh)| hh.home.and_then(|b| self.building_lot(b)).map(|l| (h, l.center)))
            .collect();
        for (h, at) in rich {
            let Some(&(d, _)) = dealers.iter().filter(|(_, p)| p.distance(at) <= super::cars::CAR_DEALER_REACH).min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)).then(a.0.cmp(&b.0))) else { continue };
            let Some(hh) = self.households.get(h) else { continue };
            if hh.money < DEALER_LEASE * 20.0 {
                continue;
            }
            let to = self.business_account(d).unwrap_or(Account::Outside);
            self.transfer(Account::Household(h), to, DEALER_LEASE, Kind::Cars);
            if let Some(z) = self.buildings.get_mut(d).and_then(|b| b.business.as_mut()) {
                z.revenue_today += DEALER_LEASE;
            }
        }
    }

    // ── opening what the city wants ─────────────────────────────────────

    /// Development's pressure branch: the strongest pressure that has held
    /// for `OPEN_DAYS` opens one business on the lot that suits it. True if
    /// one opened.
    pub(super) fn open_by_pressure(&mut self, vacant: &[LotId]) -> bool {
        self.pressure.pad();
        if self.pressure.readings.iter().all(|r| !r.wants_more()) {
            return false;
        }
        let day = self.clock.day();
        let sites = self.pressure_supply().building;
        let mut order: Vec<(f32, PressureKind)> = self
            .pressure
            .readings
            .iter()
            .filter(|r| r.wants_more() && !r.kind.player_placed() && self.pressure.held_days[r.kind.index()] >= pressure::OPEN_DAYS && self.pressure.cooldown[r.kind.index()] == 0)
            .filter(|r| !self.pressure.settling(r.kind, day + 1, r.kind.business().map_or(0, |b| sites[b.index()])))
            .map(|r| (r.gap, r.kind))
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, kind) in order {
            let Some(bk) = kind.business() else { continue };
            let Some(lot) = self.pressure_lot(kind, vacant) else {
                let text = self.no_lot_text(kind);
                if let Some(r) = self.pressure.readings.iter_mut().find(|r| r.kind == kind) {
                    r.held = Some(text);
                }
                continue;
            };
            let Some(id) = self.build_on_as(lot, Some(bk)) else { continue };
            let why = self.pressure.reading(kind).map(|r| r.why.split(';').next().unwrap_or("").trim().to_string()).unwrap_or_default();
            if let Some(z) = self.buildings[id].business.as_mut() {
                z.origin = Some(format!("Opened because the city wanted {}: {why}.", crate::names::a(kind.label())));
            }
            *self.pressure.opened_total.entry(kind.key().to_string()).or_insert(0) += 1;
            self.pressure.held_days[kind.index()] = 0;
            self.pressure.last_opened_day[kind.index()] = day + 1;
            if let Some(r) = self.pressure.readings.iter_mut().find(|r| r.kind == kind) {
                r.have += 1;
                r.gap -= 1.0;
                r.strength = r.gap.clamp(0.0, 1.0);
                r.held = None;
            }
            return true;
        }
        false
    }

    /// The mean of some points, or the city's centre.
    fn centroid(&self, pts: &[Vec2]) -> Vec2 {
        if pts.is_empty() {
            return self.city_centre();
        }
        pts.iter().fold(Vec2::ZERO, |a, p| a + *p) / pts.len() as f32
    }

    /// The middle of what is built.
    pub fn city_centre(&self) -> Vec2 {
        let pts: Vec<Vec2> = self.buildings.values().filter(|b| b.land_use != LandUse::Park).filter_map(|b| self.lots.get(b.lot).map(|l| l.center)).collect();
        if pts.is_empty() {
            Vec2::ZERO
        } else {
            pts.iter().fold(Vec2::ZERO, |a, p| a + *p) / pts.len() as f32
        }
    }

    fn homes_of(&self, f: impl Fn(&crate::citizens::Citizen) -> bool) -> Vec<Vec2> {
        let mut v: Vec<Vec2> = self.citizens.values().filter(|c| f(c)).filter_map(|c| c.home.and_then(|h| self.building_lot(h)).map(|l| l.center)).collect();
        v.truncate(4000);
        v
    }

    fn sites_of(&self, kinds: &[BusinessKind]) -> Vec<Vec2> {
        self.buildings
            .values()
            .filter(|b| b.business.as_ref().is_some_and(|z| !z.closed && kinds.contains(&z.kind)))
            .filter_map(|b| self.lots.get(b.lot).map(|l| l.center))
            .collect()
    }

    fn placement(&self, kind: PressureKind) -> Option<Placement> {
        use LandUse::*;
        let crash_spots: Vec<Vec2> = self.driving.recent.iter().map(|r| r.pos).collect();
        let fishing: Vec<Vec2> = self.working_docks(|k| k.is_fishing()).into_iter().map(|d| d.2).collect();
        let marinas: Vec<Vec2> = self.working_docks(|k| k.is_marina()).into_iter().map(|d| d.2).collect();
        let marinas_for_rental = marinas.clone();
        let p = match kind {
            PressureKind::TowCompany => Placement { uses: &[Industrial, Commercial], anchors: vec![self.centroid(&crash_spots)], reach: f32::INFINITY, min_area: 500.0, value: -1.0, needs_street: true, near: "where the crashes are" },
            PressureKind::Mechanic => Placement { uses: &[Commercial, Industrial], anchors: vec![self.centroid(&crash_spots)], reach: f32::INFINITY, min_area: 350.0, value: -1.0, needs_street: true, near: "where the crashes are" },
            PressureKind::CarDealer => {
                let rich = self.homes_of(|c| c.class() == Tier::High);
                Placement { uses: &[Commercial], anchors: vec![self.centroid(&rich)], reach: f32::INFINITY, min_area: 700.0, value: 0.6, needs_street: true, near: "the well-off's homes" }
            }
            PressureKind::TrashCompany => {
                let far: Vec<Vec2> = self.society.trash.bins.iter().filter(|(_, b)| !b.covered).filter_map(|(id, _)| self.building_lot(*id).map(|l| l.center)).collect();
                Placement { uses: &[Industrial], anchors: vec![self.centroid(&far)], reach: f32::INFINITY, min_area: 600.0, value: -1.0, needs_street: true, near: "the buildings no trash company serves" }
            }
            PressureKind::BikeShop => {
                let riders = self.homes_of(|c| c.owns_bike);
                Placement { uses: &[Commercial], anchors: vec![self.centroid(&riders)], reach: f32::INFINITY, min_area: 0.0, value: 0.0, needs_street: false, near: "the bike owners' homes" }
            }
            PressureKind::Slaughterhouse => {
                let farms = self.sites_of(&[BusinessKind::CowFarm, BusinessKind::ChickenFarm, BusinessKind::Feedlot]);
                if farms.is_empty() {
                    return None;
                }
                Placement { uses: &[Industrial], anchors: farms, reach: 1500.0, min_area: 800.0, value: -1.0, needs_street: true, near: "a livestock farm" }
            }
            PressureKind::FishGutting | PressureKind::FishStickFactory => {
                if fishing.is_empty() {
                    return None;
                }
                Placement { uses: &[Industrial], anchors: fishing, reach: 1400.0, min_area: 500.0, value: -0.5, needs_street: true, near: "a fishing port" }
            }
            PressureKind::FishMarket => {
                if fishing.is_empty() {
                    return None;
                }
                Placement { uses: &[Commercial, Industrial], anchors: fishing, reach: 1200.0, min_area: 0.0, value: 0.0, needs_street: false, near: "a fishing port" }
            }
            PressureKind::SeafoodRestaurant => {
                let mut anchors = marinas;
                anchors.extend(fishing);
                if anchors.is_empty() {
                    return None;
                }
                Placement { uses: &[Commercial, Entertainment], anchors, reach: 900.0, min_area: 0.0, value: 0.5, needs_street: false, near: "a marina or fishing port" }
            }
            PressureKind::GolfRange => {
                let comfortable = self.homes_of(|c| c.class() >= Tier::Mid && c.can_work());
                Placement { uses: &[Entertainment, Commercial], anchors: vec![self.centroid(&comfortable)], reach: f32::INFINITY, min_area: 2800.0, value: 0.3, needs_street: true, near: "the comfortable homes" }
            }
            PressureKind::ArtStudio => {
                // Old warehouses and workshops: cheap industrial buildings.
                let warehouses: Vec<Vec2> = self.buildings.values().filter(|b| b.land_use == LandUse::Industrial && b.tier == Tier::Low && b.port.is_none() && !b.is_power()).filter_map(|b| self.lots.get(b.lot).map(|l| l.center)).collect();
                let near = if warehouses.is_empty() { "the city centre" } else { "old warehouses (cheap industrial buildings)" };
                let anchors = if warehouses.is_empty() { vec![self.city_centre()] } else { warehouses };
                Placement { uses: &[Commercial, Industrial], anchors, reach: 900.0, min_area: 0.0, value: -1.5, needs_street: false, near }
            }
            PressureKind::Gallery => {
                let studios = self.sites_of(&[BusinessKind::ArtStudio]);
                let near = if studios.is_empty() { "the city centre" } else { "an art studio" };
                Placement { uses: &[Commercial, Entertainment], anchors: if studios.is_empty() { vec![self.city_centre()] } else { studios }, reach: 1800.0, min_area: 0.0, value: 0.4, needs_street: false, near }
            }
            // [waterfront] On lots that run down to the water (`pressure_lot`).
            PressureKind::BeachResort => {
                let beaches = self.usable_beach_centres();
                Placement { uses: &[Commercial, Entertainment], anchors: if beaches.is_empty() { vec![self.city_centre()] } else { beaches }, reach: f32::INFINITY, min_area: 1200.0, value: 0.8, needs_street: true, near: "a beach" }
            }
            PressureKind::PierRestaurant => {
                let comfortable = self.homes_of(|c| c.class() >= Tier::Mid);
                Placement { uses: &[Commercial, Entertainment], anchors: vec![self.centroid(&comfortable)], reach: f32::INFINITY, min_area: 0.0, value: 0.6, needs_street: false, near: "the comfortable homes" }
            }
            PressureKind::BeachBar => {
                let beaches = self.usable_beach_centres();
                if beaches.is_empty() {
                    return None;
                }
                Placement { uses: &[Entertainment, Commercial], anchors: beaches, reach: 700.0, min_area: 0.0, value: 0.0, needs_street: false, near: "a beach" }
            }
            PressureKind::BoatRental => {
                let mut anchors = self.usable_beach_centres();
                anchors.extend(marinas_for_rental);
                if anchors.is_empty() {
                    anchors.push(self.city_centre());
                }
                Placement { uses: &[Commercial, Entertainment], anchors, reach: f32::INFINITY, min_area: 0.0, value: 0.2, needs_street: false, near: "a beach or marina" }
            }
            PressureKind::Marina => return None,
        };
        Some(p)
    }

    /// [r4-hunger] Whether lot `id` suits a pressure's business, or the
    /// first test it fails, in the order `pressure_lot` always asked them:
    /// free, the land use, on the water (with room for a jetty), the size,
    /// a street, the reach from the anchors.
    fn lot_fit(&self, kind: PressureKind, p: &Placement, id: LotId) -> LotFit {
        let Some(lot) = self.lots.get(id) else { return LotFit::Taken };
        if !self.lot_free_for(id, SiteRule::STREET) {
            return LotFit::Taken;
        }
        if !p.uses.contains(&lot.land_use) {
            return LotFit::WrongUse;
        }
        // [waterfront] The shore's trades only on the water; a pier or a
        // pontoon only where boats can work.
        if let Some(bk) = kind.business().filter(|b| b.wants_waterfront()) {
            if !lot.shore.is_waterfront() || lot.shore.beach {
                return LotFit::NotOnWater;
            }
            if let Some(jk) = Self::jetty_kind_for(bk) {
                if crate::shore::plan_jetty(lot, &self.landscape, jk).is_err() {
                    return LotFit::NoJettyRoom;
                }
            }
        }
        if lot.area() < p.min_area {
            return LotFit::TooSmall;
        }
        if p.needs_street && !lot.car_access() {
            return LotFit::NoStreet;
        }
        let d = p.anchors.iter().map(|a| a.distance(lot.center)).fold(f32::INFINITY, f32::min);
        if d > p.reach {
            return LotFit::TooFar;
        }
        LotFit::Suits
    }

    /// [r4-hunger] The lot a pressure's business needs, for the player
    /// (`Pressure::held`, the economy pane, the API): None for the kinds
    /// the player builds or that have nowhere to go yet.
    pub fn lot_need(&self, kind: PressureKind) -> Option<LotNeed> {
        let p = self.placement(kind)?;
        Some(LotNeed {
            uses: p.uses.to_vec(),
            min_area: p.min_area,
            needs_street: p.needs_street,
            waterfront: kind.business().is_some_and(|b| b.wants_waterfront()),
            reach: p.reach.is_finite().then_some(p.reach),
            anchors: p.anchors,
            near: p.near,
        })
    }

    /// [r4-hunger] The vacant lots of the land uses a pressure's business
    /// takes, each with whether it suits and if not why (`lot_fit`), for
    /// "show where": with a reach, only those within 400 m past it.
    pub fn lot_need_lots(&self, kind: PressureKind) -> Vec<(LotId, LotFit)> {
        let Some(p) = self.placement(kind) else { return Vec::new() };
        let margin = if p.reach.is_finite() { p.reach + 400.0 } else { f32::INFINITY };
        self.lots
            .iter()
            .filter(|(_, l)| l.building.is_none() && l.usable() && p.uses.contains(&l.land_use))
            .filter(|(_, l)| p.anchors.iter().any(|a| a.distance(l.center) <= margin))
            .map(|(id, _)| (id, self.lot_fit(kind, &p, id)))
            .collect()
    }

    /// [r4-hunger] What `Pressure::held` says when no lot suits: the lot
    /// it needs and how the vacant lots of its land uses fall short.
    fn no_lot_text(&self, kind: PressureKind) -> String {
        let Some(need) = self.lot_need(kind) else { return "no vacant lot that suits it".to_string() };
        let mut tally: std::collections::BTreeMap<LotFit, u32> = Default::default();
        for (_, fit) in self.lot_need_lots(kind) {
            *tally.entry(fit).or_default() += 1;
        }
        let uses = need.uses_text();
        let short: Vec<String> = tally.iter().filter(|(f, _)| **f != LotFit::Suits).map(|(f, n)| format!("{n} {}", f.label())).collect();
        let lots = if short.is_empty() { format!("no vacant {uses} lot nearby") } else { format!("vacant {uses} lots: {}", short.join(", ")) };
        let advice = if need.waterfront { " Zone the land between a road and the shore." } else { "" };
        format!("no vacant lot that suits it: it {} ({lots}).{advice}", need.text())
    }

    /// The vacant lot a pressure's business opens on: allowed land use,
    /// big enough, near its anchors, on the land value it likes. Ties to
    /// the lower lot id.
    pub(super) fn pressure_lot(&self, kind: PressureKind, vacant: &[LotId]) -> Option<LotId> {
        let p = self.placement(kind)?;
        let rival_yards: Vec<Vec2> = match kind.business() {
            Some(k) if crate::companies::runs_fleet(k) && matches!(k, BusinessKind::TowYard | BusinessKind::TrashCompany) => self.sites_of(&[k]),
            _ => Vec::new(),
        };
        let mut best: Option<(f32, LotId)> = None;
        for &id in vacant {
            let Some(lot) = self.lots.get(id) else { continue };
            if self.lot_fit(kind, &p, id) != LotFit::Suits {
                continue;
            }
            let Some(rank) = p.uses.iter().position(|u| *u == lot.land_use) else { continue };
            // [waterfront] The shore's trades like a long frontage.
            let view = if kind.business().is_some_and(|b| b.wants_waterfront()) { -(lot.shore.frontage / 40.0).min(1.5) } else { 0.0 };
            let d = p.anchors.iter().map(|a| a.distance(lot.center)).fold(f32::INFINITY, f32::min);
            let lv = self.land_value.at(lot.center).clamp(0.0, 1.0);
            let value = if p.value < 0.0 { -p.value * lv * 3.0 } else { p.value * (1.0 - lv) * 3.0 };
            // Bigger lots suit the yards and ranges; the land use's order says
            // which it prefers.
            let room = if p.min_area > 0.0 { -(lot.area() / p.min_area).min(3.0) * 0.3 } else { 0.0 };
            // [companies] Rival yards of the same trade start in different
            // parts of town: a site close to one is marked down.
            let rivals = rival_yards.iter().map(|r| r.distance(lot.center)).fold(f32::INFINITY, f32::min);
            let spacing = if rivals < crate::companies::RIVAL_SPACING { (crate::companies::RIVAL_SPACING - rivals) / 100.0 } else { 0.0 };
            let score = if d.is_finite() { d / 150.0 } else { 0.0 } + value + rank as f32 * 0.8 + room + view + spacing;
            if best.is_none_or(|(s, b)| score < s || (score == s && id < b)) {
                best = Some((score, id));
            }
        }
        best.map(|(_, id)| id)
    }

    // ── the trades ──────────────────────────────────────────────────────

    /// On the hour: repair shops mend cars and bike shops bikes, paid by the
    /// job; galleries hold their openings and their patrons give; the
    /// waterfront's boats.
    pub(super) fn trades_hourly(&mut self) {
        let hour = self.clock.hour() as u8;
        let sig = self.pressure.signals.clone();
        // Car repair: wrecks and services over the shop day, shared by
        // capacity; what the shops cannot take goes out of town.
        let wrecks = sig.crashes;
        let services = sig.cars / pressure::CARS_PER_SERVICE;
        let car_fee = if wrecks + services > 0.0 { (wrecks * REPAIR_FEE + services * SERVICE_FEE) / (wrecks + services) } else { 0.0 };
        let bike_work = sig.bike_owners / BIKE_OWNERS_PER_REPAIR;
        for (kind, per_day, fee) in [(BusinessKind::Mechanic, wrecks + services, car_fee), (BusinessKind::BikeShop, bike_work, BIKE_REPAIR_FEE)] {
            if per_day <= 0.0 {
                continue;
            }
            let shops: Vec<(BuildingId, f32)> = self
                .buildings
                .iter()
                .filter(|(_, b)| b.operating() && b.opening_hours().contains(hour) && b.business.as_ref().is_some_and(|z| z.kind == kind && !z.closed))
                .map(|(id, b)| {
                    let staffing = if b.jobs == 0 { 0.0 } else { b.workers.len() as f32 / b.jobs as f32 };
                    let per_shop = if kind == BusinessKind::Mechanic { pressure::REPAIRS_PER_SHOP } else { 12.0 };
                    (id, per_shop / TRADE_HOURS * staffing * (b.jobs as f32 / 3.0).clamp(0.5, 3.0))
                })
                .filter(|(_, cap)| *cap > 0.0)
                .collect();
            let cap: f32 = shops.iter().map(|s| s.1).sum();
            if cap <= 0.0 {
                continue;
            }
            let done = (per_day / TRADE_HOURS).min(cap);
            for (id, c) in shops {
                let jobs = done * c / cap;
                let pay = jobs * fee;
                if let Some(z) = self.buildings[id].business.as_mut() {
                    z.revenue_today += pay;
                    z.deals += (jobs + 0.5) as u32;
                    self.transfer(Account::Outside, Account::Business(id), pay, Kind::Fees);
                }
                if kind == BusinessKind::Mechanic {
                    self.pressure.car_bill_today += pay;
                    self.scene.repairs_today += jobs;
                } else {
                    self.pressure.bike_bill_today += pay;
                    self.scene.bike_repairs_today += jobs;
                }
            }
        }
        if hour as u64 == pressure::OPENING_START_HOUR - 1 {
            self.plan_openings();
        }
        if hour as u64 == pressure::OPENING_START_HOUR {
            self.send_invitees();
        }
        self.gallery_patrons();
    }

    // ── the art scene ───────────────────────────────────────────────────

    /// At 17:00: galleries whose turn it is hold an opening tonight and
    /// send their invitations to the comfortable and the rich around.
    fn plan_openings(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let start = tick + 60;
        let galleries: Vec<(BuildingId, Vec2, String)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Gallery && !z.closed))
            .filter(|(id, _)| mix(key_of(*id), day) % pressure::OPENING_EVERY_DAYS == 0)
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center, b.name.clone())))
            .collect();
        for (g, at, name) in galleries {
            // The guest list: grown-ups of means within reach, in a fixed
            // order from the day.
            let mut guests: Vec<(u64, CitizenId)> = self
                .citizens
                .iter()
                .filter(|(_, c)| c.can_work() && c.class() >= Tier::Mid && c.home.and_then(|h| self.building_lot(h)).is_some_and(|l| l.center.distance(at) <= INVITE_REACH))
                .map(|(id, c)| (mix(key_of(id) ^ key_of(g), day) + if c.class() == Tier::High || is_artist(c) { 0 } else { u64::MAX / 3 }, id))
                .collect();
            guests.sort();
            let invited: Vec<CitizenId> = guests.into_iter().take(INVITES).map(|(_, id)| id).collect();
            for &cid in &invited {
                if let Some(c) = self.citizens.get_mut(cid) {
                    c.remember(tick, format!("Invited to the opening at {name} tonight."));
                }
            }
            self.scene.openings.push(Opening { gallery: g, from: start, until: start + pressure::OPENING_HOURS * 60, visitors: 0, donations: 0.0, sales: 0, invited });
            self.scene.openings_total += 1;
        }
        let keep = 24;
        if self.scene.openings.len() > keep {
            let extra = self.scene.openings.len() - keep;
            self.scene.openings.drain(..extra);
        }
    }

    /// At 18:00 the guests who are free go.
    fn send_invitees(&mut self) {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        let tonight: Vec<(BuildingId, Vec<CitizenId>)> = self.scene.openings.iter().filter(|o| o.on_at(tick)).map(|o| (o.gallery, o.invited.clone())).collect();
        for (g, guests) in tonight {
            for cid in guests {
                let Some(c) = self.citizens.get(cid) else { continue };
                let free = matches!(c.activity, Activity::AtHome | Activity::Idle) && !c.asleep && c.wakefulness > crate::citizens::TIRED + 0.1 && c.to_car.is_none();
                let on_shift = c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.shift_for(c.seed).contains(hour));
                if !free || on_shift || self.services.in_custody(cid) || self.is_patient(cid) {
                    continue;
                }
                // Not everyone makes it: about two in three.
                if mix(key_of(cid), tick) % 3 == 0 {
                    continue;
                }
                self.travel_to(cid, Some(g), Purpose::Visit(LandUse::Entertainment));
                if let Some(c) = self.citizens.get_mut(cid) {
                    if c.activity.is_traveling() || c.to_car.is_some() {
                        let name = self.buildings.get(g).map(|b| b.name.clone()).unwrap_or_default();
                        c.thought = format!("Off to the opening at {name}.");
                    }
                }
            }
        }
    }

    /// Every hour: visitors at a gallery look round; during an opening the
    /// rich who are enjoying it buy a piece now and then, and give.
    fn gallery_patrons(&mut self) {
        let tick = self.clock.tick;
        let hour = self.clock.hour();
        let visits: Vec<(CitizenId, BuildingId)> = self
            .citizens
            .iter()
            .filter_map(|(id, c)| match c.activity {
                Activity::Visiting(b) => Some((id, b)),
                _ => None,
            })
            .filter(|(_, b)| self.buildings.get(*b).and_then(|x| x.business.as_ref()).is_some_and(|z| z.kind == BusinessKind::Gallery && !z.closed))
            .collect();
        for (cid, g) in visits {
            let opening = self.scene.opening_at(g, tick).is_some();
            // What they can spend: pocket money and the household's purse.
            let (rich, mood, savings, artist, hh) = {
                let c = &self.citizens[cid];
                let hh = c.household.filter(|h| self.households.contains_key(*h));
                let purse = hh.map_or(0.0, |h| self.households[h].money.max(0.0));
                (c.class() == Tier::High, c.mood, c.savings.max(0.0) + purse, is_artist(c), hh)
            };
            if opening {
                if let Some(o) = self.scene.openings.iter_mut().rev().find(|o| o.gallery == g && o.on_at(tick)) {
                    o.visitors += 1;
                }
                if let Some(c) = self.citizens.get_mut(cid) {
                    c.mood = (c.mood + 0.02).min(1.0);
                    if artist {
                        c.mood = (c.mood + 0.02).min(1.0);
                    }
                }
            }
            if !rich {
                continue;
            }
            let roll = mix(key_of(cid) ^ key_of(g), tick) % 100;
            // A sale: one piece off the wall at two and a half times base.
            let price = Resource::Art.base_price() * 2.5;
            let has_art = self.buildings[g].business.as_ref().is_some_and(|z| z.stock_of(Resource::Art) >= 1.0);
            if has_art && savings > price * 3.0 && roll < if opening { 12 } else { 3 } {
                let payer = hh.map_or(Account::Person(cid), Account::Household);
                self.transfer(payer, Account::Business(g), price, Kind::Shopping);
                let c = &mut self.citizens[cid];
                let name = self.buildings[g].name.clone();
                c.remember(tick, format!("Bought a painting at {name}."));
                let z = self.buildings[g].business.as_mut().unwrap();
                z.take(Resource::Art, 1.0);
                z.revenue_today += price;
                z.deals += 1;
                self.scene.art_sold_total += 1;
                if let Some(o) = self.scene.openings.iter_mut().rev().find(|o| o.gallery == g && o.on_at(tick)) {
                    o.sales += 1;
                }
            } else if opening && mood > 0.45 && (12..40).contains(&roll) && hour >= pressure::OPENING_START_HOUR {
                // Enjoyed it: a gift towards the next show.
                let gift = (savings * 0.04).clamp(0.0, pressure::MAX_DONATION);
                if gift >= 5.0 {
                    let payer = hh.map_or(Account::Person(cid), Account::Household);
                    self.transfer(payer, Account::Business(g), gift, Kind::Gifts);
                    let c = &mut self.citizens[cid];
                    let name = self.buildings[g].name.clone();
                    c.remember(tick, format!("Loved the opening at {name}. Gave {gift:.0} towards the next show."));
                    let z = self.buildings[g].business.as_mut().unwrap();
                    z.revenue_today += gift;
                    z.donations += gift;
                    self.scene.donations_total += gift;
                    if let Some(o) = self.scene.openings.iter_mut().rev().find(|o| o.gallery == g && o.on_at(tick)) {
                        o.donations += gift;
                    }
                }
            }
        }
    }

    /// Once a day: artists drift to cheap rooms near the warehouses and
    /// into studio jobs; galleries that are thriving want to grow.
    fn art_scene_daily(&mut self) {
        let day = self.clock.day();
        // Studio jobs for artists working elsewhere or not at all.
        let mut studios: Vec<(BuildingId, u16)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.free_jobs() > 0 && b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::ArtStudio | BusinessKind::Gallery) && !z.closed))
            .map(|(id, b)| (id, b.free_jobs()))
            .collect();
        if !studios.is_empty() {
            let artists: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| is_artist(c) && c.work.is_none_or(|w| self.buildings.get(w).and_then(|b| b.business.as_ref()).is_none_or(|z| !matches!(z.kind, BusinessKind::ArtStudio | BusinessKind::Gallery))))
                .map(|(id, _)| id)
                .collect();
            let mut hired = 0;
            for cid in artists {
                if hired >= ARTIST_HIRES_PER_DAY {
                    break;
                }
                let Some(slot) = studios.iter_mut().find(|s| s.1 > 0) else { break };
                let studio = slot.0;
                slot.1 -= 1;
                if let Some(old) = self.citizens[cid].work {
                    if let Some(b) = self.buildings.get_mut(old) {
                        b.workers.retain(|w| *w != cid);
                    }
                    self.lay_off(cid, Some("Quit the day job to paint full time.".to_string()));
                }
                self.assign_job(cid, studio);
                hired += 1;
            }
        }
        // Artists' households drift to cheap rooms near the warehouses.
        let warehouses: Vec<Vec2> = self.buildings.values().filter(|b| b.land_use == LandUse::Industrial && b.tier == Tier::Low && b.port.is_none() && !b.is_power()).filter_map(|b| self.lots.get(b.lot).map(|l| l.center)).collect();
        if !warehouses.is_empty() {
            let near_warehouse = |p: Vec2| warehouses.iter().map(|w| w.distance(p)).fold(f32::INFINITY, f32::min);
            let mut movers: Vec<(u64, crate::citizens::HouseholdId)> = self
                .citizens
                .iter()
                .filter(|(_, c)| is_artist(c))
                .filter_map(|(id, c)| c.household.map(|h| (mix(key_of(id), day), h)))
                .filter(|(_, h)| self.households.get(*h).is_some_and(|hh| hh.moved_day + 7 <= day))
                .filter(|(_, h)| {
                    // Living somewhere dear or far from the warehouses.
                    let Some(home) = self.households[*h].home.and_then(|b| self.buildings.get(b)) else { return false };
                    let at = self.lots.get(home.lot).map_or(Vec2::ZERO, |l| l.center);
                    home.tier != Tier::Low || near_warehouse(at) > 500.0
                })
                .collect();
            movers.sort();
            movers.dedup_by_key(|m| m.1);
            let mut moved = 0;
            for (_, hid) in movers {
                if moved >= ARTIST_MOVES_PER_DAY {
                    break;
                }
                let Some(old) = self.households[hid].home else { continue };
                let old_rent = self.buildings.get(old).map_or(f32::INFINITY, |b| b.rent);
                let best = self
                    .buildings
                    .iter()
                    .filter(|(id, b)| *id != old && b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.tier == Tier::Low && b.rent <= old_rent)
                    .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, near_warehouse(l.center))))
                    .filter(|(_, d)| *d <= 500.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                if let Some((home, _)) = best {
                    self.move_household(hid, home, "Moved to {}: cheap rooms by the old warehouses, light to paint by.");
                    moved += 1;
                }
            }
        }
        // A gallery that pays its way and has been given enough wants room
        // to grow (the land agent's hook).
        for b in self.buildings.values_mut() {
            if let Some(z) = b.business.as_mut().filter(|z| z.kind == BusinessKind::Gallery) {
                z.wants_expand = !z.closed && z.donations >= pressure::EXPAND_DONATIONS && z.losing_days(7) <= 2;
            }
        }
    }

    // ── why it is here ──────────────────────────────────────────────────

    /// Why a business is where it is and how it is doing, in words.
    pub fn business_story(&self, bid: BuildingId) -> Option<BusinessStory> {
        let b = self.buildings.get(bid)?;
        let z = b.business.as_ref()?;
        let kind = z.kind;
        let why = z.origin.clone().unwrap_or_else(|| {
            match kind {
                BusinessKind::Parking => "Opened where drivers were circling for a space.".to_string(),
                BusinessKind::TaxiCompany => "A local put their savings into it when trips wanted cabs and found none.".to_string(),
                BusinessKind::Hotel => "Opened when visitors were being turned away for want of a bed.".to_string(),
                BusinessKind::Office => "Publicly funded offices take on the town's jobless.".to_string(),
                BusinessKind::CargoHub => "The city built it to bring in freight in bulk.".to_string(),
                BusinessKind::Grocery => "The town needs grocers for everyone's groceries.".to_string(),
                k if k.industry().is_some() => {
                    let ik = k.industry().unwrap();
                    format!("The ground and the neighbours here favour {} ({}).", ik.label(), k.label())
                }
                k => format!("The market said {} would pay its way here.", crate::names::a(k.label())),
            }
        });
        let week: Vec<&crate::market::DayBook> = z.books.iter().rev().take(7).collect();
        let (rev, spend) = week.iter().fold((0.0f32, 0.0f32), |(r, s), d| (r + d.revenue, s + d.spend));
        let days = week.len().max(1) as f32;
        let losing = z.losing_days(7);
        let rivals = self.buildings.values().filter(|x| x.business.as_ref().is_some_and(|y| y.kind == kind && !y.closed)).count().saturating_sub(1);
        let mut doing = if z.closed {
            "Shut: it ran out of money.".to_string()
        } else if b.construction.is_some() {
            "Being built.".to_string()
        } else if week.is_empty() {
            "Just opened.".to_string()
        } else if z.broke_days > 0 {
            format!("In trouble: {} days deep in debt (closes at five). Takes {:.0} a day against {:.0} paid out.", z.broke_days, rev / days, spend / days)
        } else if losing >= 4 {
            format!("Struggling: lost money {losing} of the last {} days ({:.0} a day in, {:.0} out).", week.len(), rev / days, spend / days)
        } else {
            format!("Paying its way: {:.0} a day in, {:.0} out.", rev / days, spend / days)
        };
        // What it is up against or short of.
        if !z.closed && b.operating() {
            let staff = if b.jobs > 0 { b.workers.len() as f32 / b.jobs as f32 } else { 1.0 };
            if staff < 0.5 {
                doing.push_str(&format!(" Short-staffed: {} of {} jobs filled.", b.workers.len(), b.jobs));
            }
            for (r, _) in kind.recipe().inputs {
                if z.stock_of(*r) + z.inbound_of(*r) < 0.5 {
                    doing.push_str(&format!(" Out of {}.", r.label()));
                }
            }
            if let Some(p) = PressureKind::of_business(kind).and_then(|k| self.pressure.reading(k)) {
                if p.gap < -0.5 && rivals > 0 {
                    doing.push_str(&format!(" More of them than the city needs: {} for about {:.1} wanted.", p.have, p.wanted));
                } else if rivals > 0 {
                    doing.push_str(&format!(" {rivals} competing in town."));
                }
            }
            if kind == BusinessKind::Gallery {
                if z.donations > 0.0 {
                    doing.push_str(&format!(" Given {:.0} by patrons so far.", z.donations));
                }
                if let Some(o) = self.scene.opening_at(bid, self.clock.tick) {
                    doing.push_str(&format!(" Opening tonight: {} guests so far.", o.visitors));
                }
            }
        }
        if z.wants_expand {
            doing.push_str(" Thriving: wants to buy the plots next door and grow.");
        }
        let struggling = !z.closed && (z.broke_days > 0 || losing >= 4);
        Some(BusinessStory { why, doing, struggling, wants_expand: z.wants_expand })
    }
}
