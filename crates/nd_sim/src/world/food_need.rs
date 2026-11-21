//! [r12-hunger] Hunger as demand for food businesses.
//!
//! The market opened grocers by the town's head count (`grocery_jobs_needed`)
//! and every other shop by its takings, so people going hungry asked for
//! nothing: on Harvey's day-80 city 8,000 of 17,000 were hungry while the
//! commercial demand bar read the same as on a fed day. Now each midnight
//! the hungry (gone to bed hungry, or hungry for a day or more) are counted
//! by where they live, on a grid of [`NEED_CELL`] cells, and smoothed over
//! days ([`FoodNeed`]). That reading:
//!
//! - makes a grocer (and a place cooking meals) worth opening while a share
//!   of the town is hungry ([`HUNGER_DEMAND_SHARE`]), whatever the head
//!   count says (`World::kinds_viable_as`);
//! - draws builders to vacant commercial lots where people are hungry, and
//!   the building there opens as a grocer or a place cooking meals
//!   (`World::hunger_business`): a corner store, a diner, a noodle bar;
//! - lets a grocer with bare shelves where people are hungry buy groceries
//!   in at wholesale while its own supplies are on the way (the town-wide
//!   stock said "plenty" while the stock stood in a few shops far off).
//!
//! - in a town built up to the last commercial lot, turns a shut shopfront,
//!   else a gadget shop, bookshop or bike shop losing money (not the only
//!   one of its kind), where people are hungry into a grocer, up to
//!   [`FOOD_RETOOLS_PER_DAY`] a day (`World::daily_food_retool`), and lets
//!   empty shopfronts there sooner and to the kinds that feed people.
//!
//! The hungry also eat out: a grown-up with nothing in the pantry buys a
//! meal where meals are sold (`nd_minds`), so the eateries see the custom.

use super::*;
use serde::{Deserialize, Serialize};

/// Side of a cell of the hunger grid, metres.
pub const NEED_CELL: f32 = 400.0;
/// Hungry residents in a cell (and half its neighbours') for full pull.
pub const HUNGRY_FULL: f32 = 30.0;
/// Fewer hungry than this in a cell's reach is no pull at all (a few
/// people between meals are not a market).
pub const HUNGRY_MIN: f32 = 4.0;
/// Share of the town hungry from which hunger is demand town-wide.
pub const HUNGER_DEMAND_SHARE: f32 = 0.03;
/// How fast the reading follows the day's count (share of today).
pub const NEED_SMOOTHING: f32 = 0.5;
/// A vacant commercial lot under full pull goes up this readily each
/// development round (the land-use demand's own scale is 0.06).
pub const HUNGER_BUILD_CHANCE: f32 = 0.08;
/// Pull from which a new building there opens as a food business.
pub const FOOD_PULL: f32 = 0.25;

/// The hungry by where they live, smoothed over days.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FoodNeed {
    /// (cell x, cell y, hungry residents), sorted, cells under 0.5 dropped.
    pub cells: Vec<(i32, i32, f32)>,
    /// Hungry residents at the last count, and their share of the town.
    pub hungry: u32,
    pub share: f32,
    /// Food businesses opened where people were hungry, ever.
    pub opened_total: u32,
}

impl FoodNeed {
    pub fn cell_of(p: Vec2) -> (i32, i32) {
        ((p.x / NEED_CELL).floor() as i32, (p.y / NEED_CELL).floor() as i32)
    }

    fn at(&self, c: (i32, i32)) -> f32 {
        self.cells.binary_search_by(|x| (x.0, x.1).cmp(&c)).map_or(0.0, |i| self.cells[i].2)
    }

    /// Hungry residents within reach of `p`: its cell and half of each of
    /// the eight round it.
    pub fn hungry_near(&self, p: Vec2) -> f32 {
        let (cx, cy) = Self::cell_of(p);
        let mut n = self.at((cx, cy));
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx != 0 || dy != 0 {
                    n += 0.5 * self.at((cx + dx, cy + dy));
                }
            }
        }
        n
    }

    /// How strongly hunger round `p` asks for food, `0..1`.
    pub fn pull(&self, p: Vec2) -> f32 {
        if self.cells.is_empty() {
            return 0.0;
        }
        let n = self.hungry_near(p);
        if n < HUNGRY_MIN {
            return 0.0;
        }
        (n / HUNGRY_FULL).min(1.0)
    }

    /// Blend today's count by cell into the reading.
    pub fn blend(&mut self, today: &BTreeMap<(i32, i32), f32>, hungry: u32, people: u32) {
        let mut all: BTreeMap<(i32, i32), f32> = self.cells.iter().map(|&(x, y, n)| ((x, y), n)).collect();
        for (k, v) in all.iter_mut() {
            *v += (today.get(k).copied().unwrap_or(0.0) - *v) * NEED_SMOOTHING;
        }
        for (k, v) in today {
            all.entry(*k).or_insert(v * NEED_SMOOTHING);
        }
        self.cells = all.into_iter().filter(|(_, n)| *n >= 0.5).map(|((x, y), n)| (x, y, n)).collect();
        self.hungry = hungry;
        self.share = if people > 0 { hungry as f32 / people as f32 } else { 0.0 };
    }

    /// Hunger is demand town-wide.
    pub fn town_hungry(&self) -> bool {
        self.share >= HUNGER_DEMAND_SHARE
    }
}

/// Shops (or shut shopfronts) turned into grocers where people are hungry,
/// at most, a day.
pub const FOOD_RETOOLS_PER_DAY: usize = 2;

/// Kinds that feed the hungry: the grocer first, then the places that cook
/// meals over the counter.
pub const FOOD_KINDS: [BusinessKind; 7] = [
    BusinessKind::Grocery,
    BusinessKind::Diner,
    BusinessKind::NoodleBar,
    BusinessKind::StreetFood,
    BusinessKind::PizzaPlace,
    BusinessKind::FastFood,
    BusinessKind::FoodHall,
];

impl World {
    /// Midnight: count the hungry by where they live (home, else where
    /// they are) into the reading.
    pub(super) fn daily_food_need(&mut self) {
        let mut today: BTreeMap<(i32, i32), f32> = BTreeMap::new();
        let mut hungry = 0u32;
        for c in self.citizens.values() {
            if c.satiety >= 0.25 && c.hungry_days == 0 {
                continue;
            }
            hungry += 1;
            let at = c.home.and_then(|h| self.buildings.get(h)).and_then(|b| self.lots.get(b.lot)).map_or(c.pos, |l| l.center);
            *today.entry(FoodNeed::cell_of(at)).or_default() += 1.0;
        }
        let people = self.citizens.len() as u32;
        self.food_need.blend(&today, hungry, people);
    }

    /// [r12-hunger] How strongly hunger round lot `lot` asks for food.
    pub fn hunger_pull_at(&self, lot: LotId) -> f32 {
        self.lots.get(lot).map_or(0.0, |l| self.food_need.pull(l.center))
    }

    /// [r12-hunger] The business a new building on a commercial lot where
    /// people are hungry opens as: a grocer when the market would carry one
    /// (60%, by the lot's hash), else a place cooking meals it would; a
    /// lot under the nightlife sub-zone takes only the eateries. None when
    /// the lot is not hungry or the market carries none of them.
    pub(super) fn hunger_business(&self, lot_id: LotId, lu: LandUse, jobs: u16) -> Option<BusinessKind> {
        if lu != LandUse::Commercial || self.hunger_pull_at(lot_id) < FOOD_PULL {
            return None;
        }
        let nightlife = self.commercial_sub_set(lot_id) == Some(crate::commercial::CommercialSub::Leisure);
        let menu: Vec<BusinessKind> = FOOD_KINDS.iter().copied().filter(|k| !(nightlife && *k == BusinessKind::Grocery)).collect();
        let viable = self.kinds_viable(&menu, Some(jobs));
        if viable.is_empty() {
            return None;
        }
        let h = crate::hash::mix(slotmap::Key::data(&lot_id).as_ffi(), 0x6E0D_F00D);
        if viable.contains(&BusinessKind::Grocery) && h % 10 < 6 {
            return Some(BusinessKind::Grocery);
        }
        let eat: Vec<BusinessKind> = viable.iter().copied().filter(|k| *k != BusinessKind::Grocery).collect();
        let pick = if eat.is_empty() { &viable } else { &eat };
        Some(pick[(h / 10) as usize % pick.len()])
    }

    /// [r12-hunger] While the town is hungry, turn a shut shopfront (first)
    /// or a non-food shop losing money where people are hungry into a
    /// grocer: a built-up town has no vacant lot for one. The hungriest
    /// first. Returns how many.
    pub(super) fn daily_food_retool(&mut self) -> usize {
        if !self.food_need.town_hungry() {
            return 0;
        }
        let supply = self.pressure_supply();
        let mut picks: Vec<(u8, i64, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Commercial && b.operating() && !b.is_derelict() && !b.is_tenant() && b.jobs >= 2 && b.plot.form.is_none_or(|f| !f.is_mall()))
            .filter_map(|(id, b)| {
                let z = b.business.as_ref()?;
                let shop = matches!(z.kind, BusinessKind::GadgetShop | BusinessKind::Bookshop | BusinessKind::BikeShop);
                let rank = if z.closed && (shop || FOOD_KINDS.contains(&z.kind) || matches!(z.kind, BusinessKind::Bakery | BusinessKind::Cafe)) {
                    0
                } else if !z.closed && shop && (z.broke_days > 0 || z.losing_days(5) >= 3) && supply.open[z.kind.index()] > 1 {
                    1
                } else {
                    return None;
                };
                let pull = self.lots.get(b.lot).map_or(0.0, |l| self.food_need.pull(l.center));
                (pull >= FOOD_PULL).then_some((rank, -(self.lots.get(b.lot).map_or(0.0, |l| self.food_need.hungry_near(l.center)) as i64), id))
            })
            .collect();
        picks.sort();
        picks.truncate(FOOD_RETOOLS_PER_DAY);
        let tick = self.clock.tick;
        for &(_, _, id) in &picks {
            let jobs = self.buildings[id].jobs;
            let (kind, shut) = self.buildings[id].business.as_ref().map(|z| (z.kind, z.closed)).unwrap_or((BusinessKind::Grocery, true));
            let old = format!("{} {}", if shut { "shut" } else { "struggling" }, kind.label());
            self.set_business(id, Some(Business::new(BusinessKind::Grocery, jobs)));
            self.fit_business_name(id);
            if let Some(z) = self.buildings[id].business.as_mut() {
                z.origin = Some(format!("A {old} turned into a grocer: people round here were going hungry."));
            }
            self.food_need.opened_total += 1;
            self.events.push(SimEvent::BuildingChanged(id));
            let (name, at) = (self.buildings[id].name.clone(), self.building_lot(id).map(|l| l.center).unwrap_or_default());
            self.social.push_news(tick, format!("🏢 A {old} became {name}, a grocer for a hungry neighbourhood."), at, None);
        }
        picks.len()
    }
}
