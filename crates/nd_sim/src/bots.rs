//! Home deliveries. Every quarter hour in the day a few shops (grocers,
//! bakeries, gadget shops) sell an order to a nearby household that can
//! afford it and has a short pantry; the sale is settled on the spot. Where
//! both the shop and the home stand on high land value the order goes by
//! pavement robot: a small six-wheeled [`DeliveryBot`] that walks the walk
//! graph to the door at a slow walking pace and back. Elsewhere the shop's
//! van (or the bakery's scooter) drives it, a liveried car of the traffic
//! model with no driver and no cargo, so it simply vanishes on arrival.

use crate::buildings::BuildingId;
use crate::citizens::{CitizenId, HouseholdId};
use crate::market::{BusinessKind, Resource};
use crate::vehicle::VehicleModel;
use crate::world::World;
use crate::zoning::LandUse;
use glam::Vec2;
use rand::seq::{IndexedRandom, SliceRandom};
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct BotId;
}

/// Shop and home both at least this valuable: the order goes by robot.
pub const BOT_LAND_VALUE: f32 = 0.6;
/// Robots trundle along slower than people walk.
pub const BOT_MPS: f32 = 1.1;
/// The robot fleet's size, city-wide.
pub const MAX_BOTS: usize = 40;
/// Delivery vans on the road at once.
pub const MAX_VANS: usize = 10;
/// How far a shop delivers (straight line, metres).
pub const DELIVERY_RANGE: f32 = 450.0;
/// Shops considered per round.
const SHOPS_PER_ROUND: usize = 8;
/// Ticks between rounds.
pub const ROUND_TICKS: u64 = 15;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliveryBot {
    pub shop: BuildingId,
    pub home: BuildingId,
    pub resource: Resource,
    pub pos: Vec2,
    pub prev_pos: Vec2,
    pub heading: f32,
    pub prev_heading: f32,
    pub path: Vec<Vec2>,
    pub idx: usize,
    /// Delivered; heading back to the shop.
    pub returning: bool,
    /// Ticks on the way; a bot that cannot finish is recalled.
    pub age: u32,
    /// [camera] Its number in the shop's fleet, from 1 (the lowest free one
    /// when it set out). 0 on robots from older saves.
    #[serde(default)]
    pub serial: u32,
    /// [camera] How much of `resource` it carries to the door (nothing on
    /// the way back).
    #[serde(default)]
    pub amount: f32,
    /// [camera] The household that ordered.
    #[serde(default)]
    pub household: Option<HouseholdId>,
}

impl DeliveryBot {
    /// Where it is heading now: the customer's door, or back to the shop.
    pub fn destination(&self) -> BuildingId {
        if self.returning {
            self.shop
        } else {
            self.home
        }
    }

    /// What is in its hold: the order on the way out, nothing on the way back.
    pub fn cargo(&self) -> Option<(Resource, f32)> {
        (!self.returning && self.amount > 0.0).then_some((self.resource, self.amount))
    }

    /// Metres of pavement left to where it is heading.
    pub fn metres_left(&self) -> f32 {
        let mut d = 0.0;
        let mut at = self.pos;
        for &p in self.path.iter().skip(self.idx) {
            d += at.distance(p);
            at = p;
        }
        d
    }

    /// The pavement still ahead, from where it stands.
    pub fn path_ahead(&self) -> Vec<Vec2> {
        std::iter::once(self.pos).chain(self.path.iter().skip(self.idx).copied()).collect()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bots {
    pub bots: SlotMap<BotId, DeliveryBot>,
    pub delivered_today: u32,
    pub van_orders_today: u32,
}

/// Retail a shop sends out to homes, and how much of it per order.
fn order_of(kind: BusinessKind) -> Option<(Resource, f32)> {
    match kind {
        BusinessKind::Grocery => Some((Resource::Groceries, 4.0)),
        BusinessKind::Bakery => Some((Resource::BakedGoods, 2.0)),
        BusinessKind::GadgetShop | BusinessKind::Bookshop => Some((Resource::Goods, 1.0)),
        BusinessKind::Cafe => Some((Resource::BakedGoods, 1.0)),
        _ => None,
    }
}

impl Bots {
    /// [camera] The robots out from `shop`, by fleet number.
    pub fn fleet_of(&self, shop: BuildingId) -> Vec<BotId> {
        let mut v: Vec<(u32, BotId)> = self.bots.iter().filter(|(_, b)| b.shop == shop).map(|(id, b)| (b.serial, id)).collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// [camera] The lowest fleet number free at `shop`, from 1.
    pub fn next_serial(&self, shop: BuildingId) -> u32 {
        let used: Vec<u32> = self.bots.values().filter(|b| b.shop == shop).map(|b| b.serial).collect();
        (1..).find(|n| !used.contains(n)).unwrap_or(1)
    }

    /// Delivery vans on the road now: liveried cars without driver or cargo.
    pub fn vans(w: &World) -> usize {
        w.traffic.cars.values().filter(|c| c.cargo.is_none() && c.bus.is_none() && c.citizen == CitizenId::default() && is_delivery(&c.model)).count()
    }
}

/// [camera] A robot's short id, from its slot: "B-12".
pub fn bot_code(id: BotId) -> String {
    use slotmap::Key;
    format!("B-{}", id.data().as_ffi() & 0xffff_ffff)
}

/// [camera] A robot's name: the shop's name and its number in the fleet,
/// "Corner Grocer bot #2" (its code on robots from older saves).
pub fn bot_name(w: &World, id: BotId) -> String {
    let Some(b) = w.bots.bots.get(id) else { return bot_code(id) };
    let shop = w.buildings.get(b.shop).map_or("A shop's", |s| s.name.as_str());
    if b.serial > 0 {
        format!("{shop} bot #{}", b.serial)
    } else {
        format!("{shop} bot {}", bot_code(id))
    }
}

/// [camera] The livery colour of a shop's robot fleet, as sRGB 0..1: a hue
/// of its own per company (golden-ratio steps over the building's slot, so
/// neighbouring shops differ), bright and saturated so it reads on the
/// pavement. Stable across saves, since building ids are.
pub fn fleet_colour(shop: BuildingId) -> [f32; 3] {
    use slotmap::Key;
    let slot = (shop.data().as_ffi() & 0xffff_ffff) as u32;
    let hue = (slot as f32 * 0.618_034 + 0.13).fract();
    hsv(hue, 0.78, 0.95)
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn is_delivery(m: &VehicleModel) -> bool {
    use crate::vehicle::Livery::*;
    // [megacorp] a mega-corp's vans count too
    matches!(m.livery, Pizza | Grocery | Parcels | Corp(..))
}

impl World {
    /// One delivery round: a few shops take an order each, by robot on rich
    /// streets, by van elsewhere. Called every [`ROUND_TICKS`].
    pub fn dispatch_deliveries(&mut self) {
        crate::prof_scope!("dispatch_deliveries");
        let hour = self.clock.hour() as u8;
        if !(8..22).contains(&hour) {
            return;
        }
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.config.seed ^ 0xb075, tick);
        let mut shops: Vec<BuildingId> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.construction.is_none() && !b.is_derelict() && b.serves_at(hour))
            .filter(|(_, b)| b.business.as_ref().is_some_and(|bz| !bz.closed && order_of(bz.kind).is_some_and(|(r, n)| bz.stock_of(r) >= n * 2.0)))
            .map(|(id, _)| id)
            .collect();
        shops.shuffle(&mut rng);
        shops.truncate(SHOPS_PER_ROUND);
        let mut vans = Bots::vans(self);
        for shop in shops {
            if !rng.random_bool(0.5) {
                continue;
            }
            let Some(at) = self.building_entrance(shop) else { continue };
            let b = &self.buildings[shop];
            let Some(bz) = b.business.as_ref() else { continue };
            let kind = bz.kind;
            let Some((resource, amount)) = order_of(kind) else { continue };
            let price = bz.sell_price(resource, b.jobs) * amount;
            let shop_rich = b.land_value >= BOT_LAND_VALUE;
            // Homes in range with a household short of food (or, for
            // gadgets, with money to burn) and the money to pay.
            let mut homes: Vec<(BuildingId, HouseholdId)> = Vec::new();
            for (hid, home) in self.buildings.iter().filter(|(_, h)| h.land_use == LandUse::Residential && !h.households.is_empty() && h.construction.is_none()) {
                if self.lots.get(home.lot).map_or(true, |l| l.center.distance(at) > DELIVERY_RANGE) {
                    continue;
                }
                for &hh in &home.households {
                    let Some(h) = self.households.get(hh) else { continue };
                    let short = resource != Resource::Groceries || h.groceries < h.pantry_target() * 0.5;
                    if short && h.money > price + 80.0 {
                        homes.push((hid, hh));
                    }
                }
            }
            let Some(&(home, hh)) = homes.choose(&mut rng) else { continue };
            let Some(door) = self.building_entrance(home) else { continue };
            let by_bot = shop_rich && self.buildings[home].land_value >= BOT_LAND_VALUE && self.bots.bots.len() < MAX_BOTS;
            if by_bot {
                let Some((mut path, _)) = self.walk.route(at, door) else { continue };
                path.push(door);
                let start = path[0];
                let serial = self.bots.next_serial(shop);
                self.bots.bots.insert(DeliveryBot { shop, home, resource, pos: start, prev_pos: start, heading: 0.0, prev_heading: 0.0, path, idx: 0, returning: false, age: 0, serial, amount, household: Some(hh) });
            } else {
                if vans >= MAX_VANS {
                    continue;
                }
                let stop = |w: &World, b: BuildingId| w.building_lot(b).map(|l| match l.car_segment {
                    Some(s) => (s, l.car_point),
                    None => (l.front_segment, l.entrance()),
                });
                let (Some((fs, fp)), Some((ts, tp))) = (stop(self, shop), stop(self, home)) else { continue };
                let Some((route, s0, s1)) = self.traffic.net.find_route(fs, fp, ts, tp) else { continue };
                let id = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
                self.traffic.cars[id].model = VehicleModel::delivery(kind, &mut rng);
                // [megacorp] A corp's shop sends its van in the corp's colours.
                if let Some(l) = self.corp_livery(shop) {
                    self.traffic.cars[id].model.livery = l;
                }
                // [carlife] The van drives back to the shop afterwards.
                self.truck_dispatched(id, Some(shop));
                vans += 1;
                self.bots.van_orders_today += 1;
            }
            self.settle_delivery(shop, hh, resource, amount, price);
        }
    }

    /// The sale: stock out of the shop, money from the household's purse,
    /// groceries into its pantry (a takeaway is eaten with the groceries).
    fn settle_delivery(&mut self, shop: BuildingId, hh: HouseholdId, resource: Resource, amount: f32, price: f32) {
        let Some(bz) = self.buildings.get_mut(shop).and_then(|b| b.business.as_mut()) else { return };
        let got = bz.take(resource, amount);
        if got <= 0.0 {
            return;
        }
        let cost = price * got / amount;
        bz.money += cost;
        bz.revenue_today += cost;
        self.economy.market.deals_today += 1;
        let Some(h) = self.households.get_mut(hh) else { return };
        h.money -= cost;
        if matches!(resource, Resource::Groceries | Resource::BakedGoods) {
            h.groceries += got;
            self.economy.market.meals_sold_today += got as u32;
        }
        let members = h.members.clone();
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                if resource == Resource::Goods {
                    c.mood = (c.mood + 0.04).min(1.0);
                }
                if let Some(h) = self.households.get(hh) {
                    c.groceries = h.groceries;
                }
            }
        }
    }

    /// Move every robot one tick along its path; at the door it turns for
    /// home, at the shop it parks itself (despawns).
    pub fn step_bots(&mut self) {
        let step = self.config.metres_per_tick(BOT_MPS);
        let mut done = Vec::new();
        for (id, bot) in self.bots.bots.iter_mut() {
            bot.prev_pos = bot.pos;
            bot.prev_heading = bot.heading;
            bot.age += 1;
            let mut left = step;
            while left > 0.0 && bot.idx < bot.path.len() {
                let target = bot.path[bot.idx];
                let d = target - bot.pos;
                let len = d.length();
                if len > 1e-3 {
                    bot.heading = d.y.atan2(d.x);
                }
                if len <= left {
                    bot.pos = target;
                    bot.idx += 1;
                    left -= len;
                } else {
                    bot.pos += d / len * left;
                    left = 0.0;
                }
            }
            if bot.idx >= bot.path.len() {
                if bot.returning {
                    done.push(id);
                } else {
                    bot.returning = true;
                    bot.amount = 0.0;
                    bot.path.reverse();
                    bot.idx = 0;
                    self.bots.delivered_today += 1;
                }
            }
            // A bot stuck far longer than any trip could take is recalled.
            if bot.age > 600 {
                done.push(id);
            }
        }
        for id in done {
            self.bots.bots.remove(id);
        }
        // Robots of shops or homes that are gone are recalled.
        let gone: Vec<BotId> = self.bots.bots.iter().filter(|(_, b)| !self.buildings.contains_key(b.shop) || !self.buildings.contains_key(b.home)).map(|(id, _)| id).collect();
        for id in gone {
            self.bots.bots.remove(id);
        }
    }
}

/// Vehicles on the road by body (`"sedan"`, `"bubble_bus"`, ...), plus
/// how many are tuned, beat-up or in a fleet livery, for the report.
pub fn fleet_counts(w: &World) -> std::collections::BTreeMap<&'static str, usize> {
    let mut out = std::collections::BTreeMap::new();
    for c in w.traffic.cars.values() {
        *out.entry(c.model.body.label()).or_insert(0) += 1;
        if c.model.tuned {
            *out.entry("tuned").or_insert(0) += 1;
        }
        if c.model.wear >= 2 {
            *out.entry("beat_up").or_insert(0) += 1;
        }
        if c.model.livery != crate::vehicle::Livery::None {
            *out.entry("liveried").or_insert(0) += 1;
        }
    }
    out
}
