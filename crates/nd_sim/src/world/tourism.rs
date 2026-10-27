//! [tourism] The world side of visitors (`crate::tourism`): what draws
//! them, arrivals by every way in, their days out, spending, hotel nights
//! and the city's tourist tax, departures, and hotels opening where
//! visitors find no bed.
//!
//! Schedule: attractions are re-read every two hours (at :02 of odd
//! hours), arrivals come on the hour (:03) from 08:00 to 21:00, the books
//! close at midnight (`daily_tourism`), and every tick moves the visitors
//! on foot and makes the decisions that are due. Every roll is drawn from
//! `rng_for` on the tick and the visitor's seed, never the world's stream.

use super::*;
use crate::services::ServiceKind;
use crate::tourism::{self as tm, Arrival, Attraction, AttractionKind, Goal, Tourist, TouristId, TouristState};

/// Salt for the tourism random streams.
const SALT: u64 = Stream::Tourism.salt();
/// Metres a taxi covers per tick (about 10 m/s for 4 s of motion).
const TAXI_M_PER_TICK: f32 = 40.0;

impl World {
    /// Beds in a working hotel (0 for anything else).
    pub fn hotel_beds(&self, b: BuildingId) -> u16 {
        let Some(bld) = self.buildings.get(b) else { return 0 };
        // [waterfront] Beach resorts sleep visitors too.
        let Some(kind) = bld.business.as_ref().filter(|z| z.kind.is_lodging() && !z.closed).map(|z| z.kind) else { return 0 };
        if !bld.operating() || bld.is_derelict() {
            return 0;
        }
        let beds = bld.floors.max(1) as u16 * tm::beds_per_floor(bld.tier);
        // A resort's low wings spread over its grounds: rooms by the acre.
        if kind == BusinessKind::BeachResort { beds * 2 } else { beds }
    }

    /// [waterfront] What a night costs at this hotel or resort: a resort
    /// charges over a hotel of its tier for the beach.
    pub fn night_price_at(&self, b: BuildingId) -> f32 {
        let Some(bld) = self.buildings.get(b) else { return 0.0 };
        let resort = bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::BeachResort);
        tm::night_price(bld.tier) * if resort { tm::RESORT_PREMIUM } else { 1.0 }
    }

    /// Visitors booked into each hotel.
    pub fn hotel_guests(&self) -> HashMap<BuildingId, u16> {
        let mut out: HashMap<BuildingId, u16> = HashMap::new();
        for t in self.tourism.tourists.values().filter(|t| t.commuter.is_none()) {
            *out.entry(t.hotel).or_default() += 1;
        }
        out
    }

    /// Every hotel with its beds and guests, in slot order.
    pub fn hotels(&self) -> Vec<(BuildingId, u16, u16)> {
        let guests = self.hotel_guests();
        self.buildings
            .iter()
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind.is_lodging()))
            .map(|(id, _)| (id, self.hotel_beds(id), guests.get(&id).copied().unwrap_or(0)))
            .collect()
    }

    // ── what draws them ────────────────────────────────────────────────────

    /// Re-read what there is to see and how the city can be reached.
    pub fn refresh_attractions(&mut self) {
        let mut out: Vec<Attraction> = Vec::new();
        let entrance = |w: &World, b: BuildingId| w.building_entrance(b).unwrap_or_default();
        for (id, b) in self.buildings.iter() {
            if !b.operating() {
                continue;
            }
            if let Some(s) = b.service {
                // [attractions] A landmark: its own pull, by how well it is
                // staffed; visitors stand where it is best seen.
                if let Some(kind) = AttractionKind::for_landmark(s.kind) {
                    let score = s.kind.tourist_pull() * (0.5 + 0.5 * b.staffing());
                    let pos = self.attraction_spot(id).unwrap_or_else(|| entrance(self, id));
                    out.push(Attraction { kind, name: b.name.clone(), pos, building: Some(id), score });
                    continue;
                }
                let kind = match s.kind {
                    ServiceKind::Monument => AttractionKind::Monument,
                    ServiceKind::Stadium => AttractionKind::Stadium,
                    _ => continue,
                };
                out.push(Attraction { kind, name: b.name.clone(), pos: entrance(self, id), building: Some(id), score: tm::landmark_score(kind == AttractionKind::Stadium, s.tier) });
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            match b.land_use {
                // [waterfront] Beaches are their own sights (below).
                LandUse::Park if lot.shore.beach => {}
                LandUse::Park => {
                    let score = tm::park_score(b.tier, lot.area());
                    if score > 0.0 {
                        // Parks are walked into: the middle of the lot.
                        out.push(Attraction { kind: AttractionKind::Park, name: b.name.clone(), pos: lot.center, building: Some(id), score });
                    }
                }
                LandUse::Entertainment if b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue && !z.closed) && !b.is_derelict() => {
                    out.push(Attraction { kind: AttractionKind::Nightlife, name: b.name.clone(), pos: entrance(self, id), building: Some(id), score: tm::nightlife_score(b.tier) });
                }
                _ => {}
            }
        }
        // Landmark districts: the building nearest the middle is the place
        // to stand and look.
        let ext = self.districts.grid.extents();
        for d in self.districts.list.iter().filter(|d| d.is_landmark()) {
            let Some(e) = ext.get(&d.id) else { continue };
            let mid = e.centroid();
            let mut n = 0usize;
            let mut best: Option<(f32, BuildingId)> = None;
            for (id, b) in self.buildings.iter().filter(|(_, b)| b.operating() && matches!(b.land_use, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment)) {
                let Some(lot) = self.lots.get(b.lot) else { continue };
                if self.districts.grid.at(lot.center) != d.id {
                    continue;
                }
                n += 1;
                let dist = lot.center.distance(mid);
                if best.is_none_or(|(bd, _)| dist < bd) {
                    best = Some((dist, id));
                }
            }
            if let (Some((_, b)), true) = (best, n >= 3) {
                out.push(Attraction { kind: AttractionKind::District, name: d.name.clone(), pos: entrance(self, b), building: None, score: tm::district_score(n, d.historic) });
            }
        }
        // The waterfront: built lots within 70 m of water. The viewpoint is
        // the one in the middle of the run.
        if !self.landscape.is_flat() {
            let mut shore: Vec<(LotId, Vec2)> = Vec::new();
            for (lid, lot) in self.lots.iter() {
                if lot.building.is_none() {
                    continue;
                }
                if self.landscape.nearest_shore(lot.center, 70.0).is_some() {
                    shore.push((lid, lot.entrance()));
                }
            }
            if !shore.is_empty() {
                let mid = shore.iter().fold(Vec2::ZERO, |a, (_, p)| a + *p) / shore.len() as f32;
                let at = shore.iter().min_by(|a, b| a.1.distance(mid).total_cmp(&b.1.distance(mid))).map(|(_, p)| *p).unwrap_or(mid);
                out.push(Attraction { kind: AttractionKind::Coast, name: "the waterfront".into(), pos: at, building: None, score: tm::coast_score(shore.len()) });
            }
        }
        // [waterfront] Public beaches people can reach and swim from, by
        // their sand and this week's weather (an Arctic beach draws nobody).
        let season = self.beach_appeal_season();
        for (bid, s) in &self.shore.beaches {
            let Some(lot) = self.buildings.get(*bid).and_then(|b| self.lots.get(b.lot)) else { continue };
            if !s.usable() {
                continue;
            }
            let score = tm::beach_score(if s.area > 0.0 { s.area } else { lot.area() }, season);
            if score > 0.0 {
                let pos = lot.park.as_ref().and_then(|p| p.paths.first()).and_then(|w| w.last().copied()).unwrap_or(lot.center);
                out.push(Attraction { kind: AttractionKind::Beach, name: s.name.clone(), pos, building: Some(*bid), score });
            }
        }
        self.tourism.attractiveness = out.iter().map(|a| a.score).sum();
        self.tourism.attractions = out;
        self.tourism.ways_in = self.tourist_ways_in();
        // [clarity] A dear tourist tax keeps some away; a cheap one draws more.
        self.tourism.demand_per_day = tm::demand_per_day(self.tourism.attractiveness, self.tourism.ways_in) * self.economy.taxes.tourist_pull();
    }

    /// Which ways in work now, by `Arrival` index.
    fn tourist_ways_in(&self) -> [bool; 4] {
        let road = self.entry_node_const().is_some();
        let rail = !self.stations.is_empty();
        // [ferries] A terminal with a boat to a town off the map is enough.
        let ferry = !self.ports.remote_terminals().is_empty();
        let air = !self.vertiports().is_empty();
        [road, rail, ferry, air]
    }

    fn vertiports(&self) -> Vec<BuildingId> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.service.is_some_and(|s| s.kind == ServiceKind::Vertiport))
            .map(|(id, _)| id)
            .collect()
    }

    // ── the tick ───────────────────────────────────────────────────────────

    pub(super) fn step_tourism(&mut self) {
        let tick = self.clock.tick;
        if tick % 120 == 62 || (tick % 60 == 2 && self.tourism.attractions.is_empty() && !self.buildings.is_empty()) {
            self.refresh_attractions();
        }
        if tick % 60 == 3 {
            self.tourist_arrivals();
            self.open_hotels();
        }
        if self.tourism.tourists.is_empty() {
            return;
        }
        self.move_tourists();
        let due: Vec<TouristId> = self.tourism.tourists.iter().filter(|(_, t)| t.next_tick <= tick).map(|(id, _)| id).collect();
        for id in due {
            self.tourist_decide(id);
        }
    }

    /// Visitors arriving this hour: the day's demand spread over the
    /// arrival hours, each looking for a bed they can afford.
    fn tourist_arrivals(&mut self) {
        let hour = self.clock.hour();
        if !(8..22).contains(&hour) {
            return;
        }
        let t = &mut self.tourism;
        t.carry += t.demand_per_day / 14.0;
        let n = t.carry.floor() as usize;
        t.carry -= n as f32;
        if n == 0 {
            return;
        }
        let ways = t.ways_in;
        let mut free: Vec<(BuildingId, i32, Tier)> = self.hotels().into_iter().filter(|(_, beds, _)| *beds > 0).map(|(id, beds, guests)| (id, beds as i32 - guests as i32, self.buildings[id].tier)).collect();
        let stations: Vec<BuildingId> = self.stations.clone();
        let ferries = self.ports.remote_terminals();
        let pads = self.vertiports();
        let tick = self.clock.tick;
        for k in 0..n {
            if self.tourism.tourists.len() >= tm::MAX_TOURISTS {
                self.tourism.stats.turned_away_today += (n - k) as u32;
                break;
            }
            let seed = crate::rng_for(self.stream_seed(Stream::Tourism), tick.wrapping_mul(131) + k as u64).random::<u64>();
            let mut rng = crate::rng_for(seed, SALT);
            let class = match rng.random::<f32>() {
                x if x < 0.35 => Tier::Low,
                x if x < 0.85 => Tier::Mid,
                _ => Tier::High,
            };
            // A bed: their own kind of hotel first; the rich avoid capsules.
            let weights: Vec<f32> = free
                .iter()
                .map(|(_, left, tier)| {
                    if *left <= 0 {
                        0.0
                    } else if *tier == class {
                        3.0
                    } else if class == Tier::High && *tier == Tier::Low {
                        0.15
                    } else {
                        1.0
                    }
                })
                .collect();
            let total: f32 = weights.iter().sum();
            if total <= 0.0 {
                self.tourism.stats.turned_away_today += 1;
                continue;
            }
            let mut r = rng.random_range(0.0..total);
            let mut pick = 0;
            for (i, w) in weights.iter().enumerate() {
                if r < *w {
                    pick = i;
                    break;
                }
                r -= w;
            }
            free[pick].1 -= 1;
            let hotel = free[pick].0;
            // How they came: by what works, the rich by air when they can.
            let mut modes: Vec<(Arrival, f32)> = Vec::new();
            if ways[0] {
                modes.push((Arrival::Road, 0.55));
            }
            if ways[1] && !stations.is_empty() {
                modes.push((Arrival::Rail, 0.3));
            }
            if ways[2] && !ferries.is_empty() {
                modes.push((Arrival::Ferry, 0.15));
            }
            if ways[3] && !pads.is_empty() && class == Tier::High {
                modes.push((Arrival::Air, 0.6));
            }
            if modes.is_empty() {
                // Demand only exists with a way in; a refresh is pending.
                free[pick].1 += 1;
                break;
            }
            let total: f32 = modes.iter().map(|m| m.1).sum();
            let mut r = rng.random_range(0.0..total);
            let mut via = modes[0].0;
            for (m, w) in &modes {
                if r < *w {
                    via = *m;
                    break;
                }
                r -= w;
            }
            let port = match via {
                Arrival::Road => None,
                Arrival::Rail => stations.choose(&mut rng).copied(),
                Arrival::Ferry => ferries.choose(&mut rng).copied(),
                Arrival::Air => pads.choose(&mut rng).copied(),
            };
            let nights = 1 + rng.random_range(0..=tm::MAX_EXTRA_NIGHTS);
            let budget = Tourist::daily_budget(class) * (nights + 1) as f32 * rng.random_range(0.8..1.25);
            let rooms = self.night_price_at(hotel) * (1.0 + self.economy.taxes.tourist_tax()) * nights as f32;
            let start = port.and_then(|p| self.building_entrance(p)).or_else(|| self.building_entrance(hotel)).unwrap_or_default();
            let age = rng.random_range(19..72u8);
            let tourist = Tourist {
                seed,
                age,
                class,
                pos: start,
                prev_pos: start,
                heading: 0.0,
                prev_heading: 0.0,
                hotel,
                via,
                port,
                arrived_tick: tick,
                leave_day: self.clock.day() + nights as u64,
                money: budget + rooms,
                spent: 0.0,
                state: TouristState::InHotel,
                next_tick: tick + 1,
                sights: 0,
                commuter: None,
            };
            let id = self.tourism.tourists.insert(tourist);
            let s = &mut self.tourism.stats;
            s.arrivals_today += 1;
            s.arrivals_total += 1;
            s.arrivals_by_mode_today[via.index()] += 1;
            if let (Arrival::Ferry | Arrival::Rail, Some(terminal)) = (via, port) {
                // [ferries] On the next boat in from the town over the
                // water; they step off when it ties up (`ferry_returned`).
                let t = &mut self.tourism.tourists[id];
                t.state = TouristState::Aboard { terminal, since: tick };
                t.next_tick = tick + tm::ABOARD_GIVE_UP;
            } else if port.is_some() {
                // Off the train, boat or drone: to the hotel first.
                let to = self.building_entrance(hotel).unwrap_or(start);
                self.tourist_leg(id, Goal::Hotel, to);
            } else {
                // Set down at the door by taxi or coach: check in, then out.
                let t = &mut self.tourism.tourists[id];
                t.next_tick = tick + 20 + rng.random_range(0..60);
            }
        }
    }

    /// When visitors are turned away for want of beds, a hotel goes up on a
    /// vacant commercial lot (a tourist quarter's first, then the one
    /// nearest the sights), whatever the general commercial demand says:
    /// the visitors are the demand.
    fn open_hotels(&mut self) {
        // Smoothed demand, or today's queue at the door while there is no
        // bed at all: the first hotel should not wait for midnight's books.
        let no_beds = self.hotels().iter().all(|h| h.1 == 0);
        let demand = self.tourism.hotel_demand.max(if no_beds { self.tourism.stats.turned_away_today as f32 } else { 0.0 });
        if demand < tm::HOTEL_DEMAND_TO_BUILD {
            return;
        }
        let building = self.buildings.values().filter(|b| b.construction.is_some() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Hotel)).count();
        if building >= 1 + (demand / 25.0) as usize {
            return;
        }
        if self.economy.cover_of(BusinessKind::Hotel).is_some_and(|c| c < 0.85) {
            return;
        }
        let mut rng = crate::rng_for(self.stream_seed(Stream::Tourism) ^ 0x407E1, self.clock.tick);
        if !rng.random_bool(0.5) {
            return;
        }
        let sights: Vec<(Vec2, f32)> = self.tourism.attractions.iter().map(|a| (a.pos, a.score)).collect();
        let mut lots: Vec<(LotId, f32)> = self
            .lots
            .iter()
            .filter(|(id, l)| l.land_use == LandUse::Commercial && self.lot_free_for(*id, SiteRule::STREET))
            .map(|(id, l)| {
                let quarter = self.districts.at(l.center).is_some_and(|d| d.commercial == Some(crate::districts::CommercialSpec::Tourist));
                let pull: f32 = sights.iter().map(|(p, s)| s / (1.0 + p.distance(l.center) / 300.0)).sum();
                // [waterfront] Rooms with a view: a hotel on the water first
                // (after a tourist quarter), drawn as a waterfront hotel.
                let view = if l.shore.is_waterfront() { 500.0 + l.shore.frontage } else { 0.0 };
                (id, pull + view + if quarter { 1000.0 } else { 0.0 })
            })
            .collect();
        lots.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| lot_order(a.0).cmp(&lot_order(b.0))));
        // The best lot may refuse (rubble still on it, say): try the next.
        let before = self.tourism.hotel_demand;
        self.tourism.hotel_demand = demand;
        for (lot, _) in lots.into_iter().take(8) {
            self.build_on(lot);
            if self.lots.get(lot).is_some_and(|l| l.building.is_some()) {
                break;
            }
        }
        self.tourism.hotel_demand = before;
    }

    // ── on foot ───────────────────────────────────────────────────────────

    /// Send a visitor to `to`: on foot over the pedestrian network when it
    /// is near and a way exists, otherwise by taxi (out of sight).
    pub(super) fn tourist_leg(&mut self, id: TouristId, goal: Goal, to: Vec2) {
        let tick = self.clock.tick;
        let Some(t) = self.tourism.tourists.get(id) else { return };
        let from = t.pos;
        let dist = from.distance(to);
        let walked = if dist <= tm::WALK_MAX_M { self.walk.route(from, to) } else { None };
        let t = &mut self.tourism.tourists[id];
        match walked {
            Some((pts, nodes)) => {
                let mut path = Vec::with_capacity(pts.len() + 2);
                path.push(from);
                path.extend(pts);
                path.push(to);
                t.state = TouristState::Walking { goal, path, step: 1 };
                t.next_tick = u64::MAX;
                // Their footsteps count for the shops they pass.
                for lot in self.walk.lots_passed(&nodes) {
                    if let Some(l) = self.lots.get_mut(lot) {
                        l.footfall_hour += 1;
                        l.footfall_today += 1;
                    }
                }
            }
            None => {
                let until = tick + 4 + (dist / TAXI_M_PER_TICK).ceil() as u64;
                t.state = TouristState::Riding { goal, to, until };
                t.next_tick = until;
            }
        }
    }

    /// Walk everyone on foot one tick along their path.
    fn move_tourists(&mut self) {
        let speed = self.config.walk_speed;
        let mut arrived: Vec<(TouristId, Goal)> = Vec::new();
        for (id, t) in self.tourism.tourists.iter_mut() {
            t.prev_pos = t.pos;
            t.prev_heading = t.heading;
            let TouristState::Walking { goal, path, step } = &mut t.state else { continue };
            let mut left = speed;
            while left > 0.0 && *step < path.len() {
                let target = path[*step];
                let d = target - t.pos;
                let len = d.length();
                if len <= left {
                    t.pos = target;
                    left -= len;
                    *step += 1;
                } else {
                    t.pos += d / len * left;
                    left = 0.0;
                }
                if len > 0.01 {
                    t.heading = d.y.atan2(d.x);
                }
            }
            if *step >= path.len() {
                arrived.push((id, *goal));
            }
        }
        for (id, goal) in arrived {
            self.tourist_arrive(id, goal);
        }
    }

    /// At the end of a leg.
    fn tourist_arrive(&mut self, id: TouristId, goal: Goal) {
        let tick = self.clock.tick;
        let Some(t) = self.tourism.tourists.get_mut(id) else { return };
        let mut rng = crate::rng_for(t.seed, tick ^ SALT);
        match goal {
            Goal::Hotel => {
                t.state = TouristState::InHotel;
                t.next_tick = tick + rng.random_range(30..120);
            }
            Goal::Sight(kind) => {
                t.state = TouristState::Sightseeing { kind, until: tick + rng.random_range(35..110) };
                t.next_tick = tick + 1;
                t.sights += 1;
                // A ticket at the stadium on a match day.
                if kind == AttractionKind::Stadium && t.money > 15.0 {
                    t.spent += 15.0;
                    self.pay_city(Account::Tourist(id), Flow::Tickets, 15.0, None);
                }
                // [attractions] In through the turnstile: a landmark
                // charges at the door and counts its visitors.
                if let Some(b) = kind.landmark().and_then(|k| self.landmark(k)) {
                    let purse = self.tourism.tourists[id].money;
                    let paid = self.admit_visitor(b, true, purse, Account::Tourist(id));
                    let t = &mut self.tourism.tourists[id];
                    t.spent += paid;
                    // Longer at a landmark than at a statue.
                    if let TouristState::Sightseeing { until, .. } = &mut t.state {
                        *until += 60;
                    }
                }
                self.tourism.stats.sights_today += 1;
                // Stand back from the entrance a little, facing it.
                let off = Vec2::new(rng.random_range(-4.0..4.0), rng.random_range(-4.0..4.0));
                let t = &mut self.tourism.tourists[id];
                t.heading = (-off.y).atan2(-off.x);
                t.pos += off;
            }
            Goal::Shop(b) => self.tourist_shop(id, b),
            Goal::Depart => {
                // [ferries] At the terminal: wait on the pier for the next
                // boat out (`Ports::boarding`).
                if let Some(t) = self.tourism.tourists.remove(id) {
                    // [pt-transit] Onto the next train out.
                    if let (Arrival::Rail, Some(p)) = (t.via, t.port) {
                        self.rail_departure(p);
                    }
                    if let (Arrival::Ferry, Some(p)) = (t.via, t.port) {
                        *self.ports.boarding.entry(p).or_insert(0) += 1;
                        if t.commuter.is_some() {
                            self.ports.stats.commuters_out_today += 1;
                        } else {
                            self.ports.stats.visitors_out_today += 1;
                        }
                    }
                    if t.commuter.is_none() {
                        self.tourism.stats.departures_today += 1;
                    }
                }
            }
            Goal::Work(b) => self.commuter_at_work(id, b),
        }
    }

    /// In a shop or venue: spend part of the day's budget there.
    fn tourist_shop(&mut self, id: TouristId, b: BuildingId) {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        let Some(t) = self.tourism.tourists.get(id) else { return };
        let mut rng = crate::rng_for(t.seed, tick ^ SALT ^ 0x5409);
        let want = (Tourist::daily_budget(t.class) * rng.random_range(0.12..0.35)).min(t.money.max(0.0));
        let Some(bld) = self.buildings.get_mut(b) else {
            let to = self.building_entrance(self.tourism.tourists[id].hotel).unwrap_or_default();
            self.tourist_leg(id, Goal::Hotel, to);
            return;
        };
        let open = bld.opening_hours().contains(hour) && bld.operating();
        let jobs = bld.jobs;
        let mut paid = 0.0;
        let mut venue = false;
        if let Some(bz) = bld.business.as_mut().filter(|z| !z.closed && open) {
            match bz.kind {
                BusinessKind::Venue => {
                    venue = true;
                    paid = want;
                }
                BusinessKind::GadgetShop | BusinessKind::Bakery | BusinessKind::Grocery | BusinessKind::Cafe | BusinessKind::Bookshop => {
                    let r = bz.kind.walk_in_sale().unwrap_or(Resource::Groceries);
                    let price = bz.sell_price(r, jobs).max(0.1);
                    let got = bz.take(r, want / price);
                    paid = got * price;
                    if r == Resource::BakedGoods {
                        self.economy.market.meals_sold_today += got as u32;
                    }
                }
                _ => {}
            }
            bz.revenue_today += paid;
            if paid > 0.0 {
                bz.deals += 1;
            }
        }
        // [ledger] From the visitor's wallet to the shop or venue.
        if self.business_account(b).is_some() {
            self.transfer(Account::Tourist(id), Account::Business(b), paid, Kind::Tourism);
        }
        let s = &mut self.tourism.stats;
        if venue {
            s.spend_venues_today += paid;
        } else {
            s.spend_shops_today += paid;
        }
        s.spend_total += paid as f64;
        let t = &mut self.tourism.tourists[id];
        t.spent += paid;
        let stay = if venue { rng.random_range(60..150) } else { rng.random_range(15..45) };
        t.state = TouristState::Inside { building: b, until: tick + stay };
        t.next_tick = tick + stay;
    }

    // ── decisions ─────────────────────────────────────────────────────────

    /// Whatever is next for a visitor whose time has come.
    fn tourist_decide(&mut self, id: TouristId) {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u32;
        let day = self.clock.day();
        let Some(t) = self.tourism.tourists.get(id).cloned() else { return };
        let mut rng = crate::rng_for(t.seed, tick ^ SALT ^ 0xDEC1);
        // The hotel is gone (pulled down, burnt, closed): home early.
        let leaving = matches!(t.state, TouristState::Walking { goal: Goal::Depart, .. } | TouristState::Riding { goal: Goal::Depart, .. } | TouristState::Aboard { .. });
        if t.commuter.is_none() && self.hotel_beds(t.hotel) == 0 && !leaving {
            self.tourist_depart(id);
            return;
        }
        match t.state {
            TouristState::Aboard { terminal, .. } if t.via == Arrival::Rail => {
                // [pt-transit] No train came (the track was cut): off at
                // the station anyway after a long wait.
                self.rail_disembark(id, terminal);
            }
            TouristState::Aboard { terminal, .. } => {
                // [ferries] The boat never came (the terminal shut, the
                // water route lost): ashore here anyway after a long wait.
                self.disembark(id, terminal);
            }
            TouristState::Inside { until, .. } if t.commuter.is_some() => {
                if tick < until {
                    self.tourism.tourists[id].next_tick = until;
                } else {
                    self.commuter_off_shift(id);
                }
            }
            TouristState::Riding { goal, to, .. } => {
                self.tourism.tourists[id].pos = to;
                self.tourism.tourists[id].prev_pos = to;
                self.tourist_arrive(id, goal);
            }
            TouristState::Walking { .. } => {
                // Moved by `move_tourists`; nothing is due.
                self.tourism.tourists[id].next_tick = u64::MAX;
            }
            TouristState::InHotel => {
                if day >= t.leave_day && hour >= 9 {
                    self.tourist_depart(id);
                } else if !(8..23).contains(&hour) || t.money < 5.0 {
                    // Asleep (or broke): out again after breakfast.
                    let wake = if hour >= 8 { (24 - hour as u64 + 8) * 60 } else { (8 - hour as u64) * 60 };
                    let tt = &mut self.tourism.tourists[id];
                    tt.next_tick = tick + wake.max(30) + rng.random_range(0..150);
                    if t.money < 5.0 && day >= t.leave_day {
                        self.tourist_depart(id);
                    }
                } else {
                    self.tourist_outing(id, &mut rng);
                }
            }
            TouristState::Sightseeing { until, .. } | TouristState::Inside { until, .. } => {
                if tick < until {
                    self.tourism.tourists[id].next_tick = until;
                    return;
                }
                if let TouristState::Inside { building, .. } = t.state {
                    // Out of the door they went in by.
                    if let Some(p) = self.building_entrance(building) {
                        self.tourism.tourists[id].pos = p;
                        self.tourism.tourists[id].prev_pos = p;
                    }
                }
                let late = !(8..22).contains(&hour);
                if late || rng.random_bool(0.4) || t.money < 5.0 {
                    let to = self.building_entrance(t.hotel).unwrap_or(t.pos);
                    self.tourist_leg(id, Goal::Hotel, to);
                } else {
                    self.tourist_outing(id, &mut rng);
                }
            }
        }
    }

    /// Off out: a sight by day, a shop now and then, a venue after dark.
    fn tourist_outing(&mut self, id: TouristId, rng: &mut SimRng) {
        let hour = self.clock.hour() as u32;
        let Some(t) = self.tourism.tourists.get(id) else { return };
        let here = t.pos;
        let night = hour >= 18 || hour < 2;
        let shop_first = if night { rng.random_bool(0.6) } else { rng.random_bool(0.3) };
        if shop_first {
            if let Some(b) = self.tourist_pick_shop(here, night, t.class, rng) {
                let to = self.building_entrance(b).unwrap_or(here);
                self.tourist_leg(id, Goal::Shop(b), to);
                return;
            }
        }
        // A sight, weighed by its pull and how far it is.
        let options: Vec<(usize, f32)> = self
            .tourism
            .attractions
            .iter()
            .enumerate()
            .filter(|(_, a)| a.kind != AttractionKind::Nightlife && a.kind.open_at(hour))
            .map(|(i, a)| (i, a.score / (1.0 + a.pos.distance(here) / 600.0).powi(2)))
            .filter(|(_, w)| *w > 0.0)
            .collect();
        let total: f32 = options.iter().map(|o| o.1).sum();
        if total > 0.0 {
            let mut r = rng.random_range(0.0..total);
            let mut pick = options[0].0;
            for (i, w) in &options {
                if r < *w {
                    pick = *i;
                    break;
                }
                r -= w;
            }
            let a = &self.tourism.attractions[pick];
            let (kind, to) = (a.kind, a.pos);
            // [waterfront] A beach-goer lays a towel somewhere on the sand.
            let to = if kind == AttractionKind::Beach { self.beach_spot(to, self.tourism.tourists[id].seed) } else { to };
            self.tourist_leg(id, Goal::Sight(kind), to);
            return;
        }
        if let Some(b) = self.tourist_pick_shop(here, night, Tier::Mid, rng) {
            let to = self.building_entrance(b).unwrap_or(here);
            self.tourist_leg(id, Goal::Shop(b), to);
            return;
        }
        // Nothing open to see: rest at the hotel for an hour.
        let t = &mut self.tourism.tourists[id];
        if matches!(t.state, TouristState::InHotel) {
            t.next_tick = self.clock.tick + 60;
        } else {
            let hotel = t.hotel;
            let to = self.building_entrance(hotel).unwrap_or(here);
            self.tourist_leg(id, Goal::Hotel, to);
        }
    }

    /// An open shop (by day) or venue (after dark) within reach, nearer and
    /// more to their taste first.
    fn tourist_pick_shop(&self, here: Vec2, night: bool, class: Tier, rng: &mut SimRng) -> Option<BuildingId> {
        let hour = self.clock.hour() as u8;
        let mut options: Vec<(BuildingId, f32)> = Vec::new();
        for (id, b) in self.buildings.iter() {
            let Some(z) = b.business.as_ref() else { continue };
            // [waterfront] the restaurants on piers, beach bars and boat rentals too
            let shore = matches!(z.kind, BusinessKind::PierRestaurant | BusinessKind::BeachBar) || (!night && z.kind == BusinessKind::BoatRental);
            let wanted = shore || if night { z.kind == BusinessKind::Venue } else { matches!(z.kind, BusinessKind::GadgetShop | BusinessKind::Bakery | BusinessKind::Cafe | BusinessKind::Bookshop) || (z.kind == BusinessKind::Venue && hour >= 18) };
            if !wanted || z.closed || !b.operating() || !b.opening_hours().contains(hour) {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let d = lot.center.distance(here);
            if d > 900.0 {
                continue;
            }
            let taste = if b.tier == class { 2.0 } else { 1.0 };
            // A neon strip or night market is where visitors go to shop.
            let strip = match b.look {
                Some(crate::districts::Look::NeonStrip | crate::districts::Look::NightMarket | crate::districts::Look::Tourist) => 2.5,
                _ => 1.0,
            };
            options.push((id, taste * strip / (1.0 + d / 250.0)));
        }
        let total: f32 = options.iter().map(|o| o.1).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = rng.random_range(0.0..total);
        for (id, w) in &options {
            if r < *w {
                return Some(*id);
            }
            r -= w;
        }
        options.last().map(|o| o.0)
    }

    /// Home: back to the station, terminal or pad they came by, or into a
    /// taxi at the hotel door for the highway.
    pub(super) fn tourist_depart(&mut self, id: TouristId) {
        let Some(t) = self.tourism.tourists.get(id) else { return };
        match t.port.filter(|p| self.buildings.contains_key(*p)).and_then(|p| self.building_entrance(p)) {
            Some(to) => self.tourist_leg(id, Goal::Depart, to),
            None => {
                self.tourism.tourists.remove(id);
                self.tourism.stats.departures_today += 1;
            }
        }
    }

    // ── midnight ──────────────────────────────────────────────────────────

    /// Close the books, then charge the night: each visitor pays their
    /// hotel and the city's tourist tax. Unmet demand for beds fades.
    pub(super) fn daily_tourism(&mut self) {
        self.tourism.stats.roll();
        // [attractions] The landmarks' books close with the day's tourism.
        self.daily_attractions();
        let t = &mut self.tourism;
        t.hotel_demand = t.hotel_demand * 0.5 + t.stats.turned_away_yesterday as f32 * 0.5;
        if self.tourism.tourists.is_empty() {
            return;
        }
        let ids: Vec<TouristId> = self.tourism.tourists.keys().collect();
        let mut tax_total = 0.0;
        let rate = self.economy.taxes.tourist_tax();
        for id in ids {
            if self.tourism.tourists[id].commuter.is_some() {
                continue;
            }
            let hotel = self.tourism.tourists[id].hotel;
            let price = self.night_price_at(hotel);
            let Some(b) = self.buildings.get_mut(hotel) else { continue };
            let Some(bz) = b.business.as_mut().filter(|z| z.kind.is_lodging() && !z.closed) else { continue };
            let t = &mut self.tourism.tourists[id];
            let tax = price * rate;
            // Rooms are paid ahead (in their money from the start); a broke
            // visitor still has the room they booked.
            t.spent += price + tax;
            bz.revenue_today += price;
            bz.deals += 1;
            tax_total += tax;
            // [ledger] The night to the hotel, the tax to the city.
            self.transfer(Account::Tourist(id), Account::Business(hotel), price, Kind::Tourism);
            self.pay_city(Account::Tourist(id), Flow::TouristTax, tax, None);
            let s = &mut self.tourism.stats;
            s.nights_today += 1;
            s.spend_hotels_today += price;
            s.spend_total += price as f64;
        }
        self.tourism.stats.tax_today += tax_total;
    }
}

/// A stable order for lots, to break ties without the slot map's order.
fn lot_order(lot: LotId) -> u64 {
    use slotmap::Key;
    lot.data().as_ffi()
}
