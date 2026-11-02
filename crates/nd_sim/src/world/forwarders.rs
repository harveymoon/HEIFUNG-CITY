//! [r8] Freight forwarders: the town's exports in boxes. A forwarder
//! takes on the surplus goods the firms sell out of town (the firm is paid
//! for its goods as before and pays the forwarder a fee per load), leases
//! a sound empty from a line at the nearest depot, and sends a lorry with
//! it round the firms: each firm's goods are packed into the same box
//! (one box takes several firms' loads) until it is full or the round is
//! done; the full box goes to the port (or the freight yard) and out on the
//! next ship or train. Hooks: `market.rs` hands the hour's exports to
//! [`World::forwarders_take_exports`]; `tick.rs` runs
//! [`World::forwarder_rounds`] hourly.

use super::*;
use crate::containers::{self as cx, BoxId, Event, ExportLoad, Forwarder, LorryJob, Owner, Round, TruckBoxes, Where};
use super::containers::DepotKind;

/// Hours a firm's load may wait for collection before a round sets off
/// for it with a part-filled box.
const PENDING_HOURS: u64 = 4;
/// Units a full forty-foot box takes (two twenty-foot boxes' worth).
const BOX_FULL: f32 = cx::BOX_UNITS * 2.0;

impl World {
    /// [r8] The open freight forwarders, with their lot centres.
    pub fn forwarders_open(&self) -> Vec<(BuildingId, Vec2)> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.operating() && !b.is_derelict() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::FreightForwarder && !z.closed))
            .filter_map(|(id, b)| Some((id, self.lots.get(b.lot)?.center)))
            .collect()
    }

    /// [r8] The hour's exports (firm, goods, units, what the firm was paid):
    /// those from firms within a forwarder's reach are its to box up, and
    /// each firm pays the fee. Returns the units it took on.
    pub fn forwarders_take_exports(&mut self, exports: &[(BuildingId, Resource, f32, f32)]) -> f32 {
        if !self.containers.live || exports.is_empty() {
            return 0.0;
        }
        let fws = self.forwarders_open();
        if fws.is_empty() || !self.box_depots().iter().any(|d| matches!(d.1, DepotKind::Port | DepotKind::Rail)) {
            return 0.0;
        }
        let mut took = 0.0;
        for &(firm, resource, amount, income) in exports {
            if amount <= 0.0 || fws.iter().any(|f| f.0 == firm) {
                continue;
            }
            let Some(p) = self.building_lot(firm).map(|l| l.center) else { continue };
            let Some(&(fw, _)) = fws.iter().filter(|f| f.1.distance(p) <= cx::FORWARDER_REACH).min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)).then(a.0.cmp(&b.0))) else { continue };
            let fee = (cx::FORWARDER_FEE + amount * cx::FORWARDER_FEE_PER_UNIT).min(income * 0.25);
            let tick = self.clock.tick;
            let f = self.containers.forwarders.entry(fw).or_default();
            if f.pending.is_empty() {
                f.since = tick;
            }
            f.pending.push(ExportLoad { firm, resource, amount });
            f.fees_total += fee;
            f.fees_today += fee;
            if let Some(z) = self.buildings.get_mut(firm).and_then(|b| b.business.as_mut()) {
                z.spend_today += fee;
            }
            if let Some(z) = self.buildings.get_mut(fw).and_then(|b| b.business.as_mut()) {
                z.revenue_today += fee;
            }
            self.transfer(Account::Business(firm), Account::Business(fw), fee, crate::bank::Kind::Supplies);
            took += amount;
        }
        took
    }

    /// [r8] The hourly round of each forwarder: with loads waiting (a box's
    /// worth, or some for hours) and a lorry free, one sets off: it fetches
    /// a leased empty from the nearest depot that has a sound one (or one
    /// bought new when none has), calls at the firms, and takes the box to
    /// the port or the freight yard.
    pub(super) fn forwarder_rounds(&mut self) {
        if !self.containers.live {
            return;
        }
        let fws = self.forwarders_open();
        // The books of forwarders gone: their loads leave by road as before.
        self.containers.forwarders.retain(|b, _| fws.iter().any(|f| f.0 == *b));
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            for f in self.containers.forwarders.values_mut() {
                f.fees_today = 0.0;
            }
        }
        for (fw, at) in fws {
            let Some(f) = self.containers.forwarders.get(&fw) else { continue };
            if f.pending.is_empty() {
                continue;
            }
            let waiting: f32 = f.pending.iter().map(|l| l.amount).sum();
            let old = tick.saturating_sub(f.since) >= PENDING_HOURS * 60;
            if waiting < BOX_FULL * 0.75 && !old {
                continue;
            }
            let jobs = self.buildings.get(fw).map_or(0, |b| b.jobs) as usize;
            let fleet = (1 + jobs / 8).min(4);
            let out = self.containers.lorries.values().filter(|j| j.round.as_ref().is_some_and(|r| r.forwarder == fw)).count();
            if out >= fleet || self.truck_count() >= crate::market::MAX_TRUCKS + super::market::BOX_TRUCKS_EXTRA {
                continue;
            }
            // Where the full box goes: the port, else the nearest freight yard.
            let depots = self.box_depots();
            let near = |want: DepotKind| depots.iter().filter(|d| d.1 == want).min_by(|a, b| a.2.distance(at).total_cmp(&b.2.distance(at)).then(a.0.cmp(&b.0))).map(|d| d.0);
            let Some(to) = near(DepotKind::Port).or_else(|| near(DepotKind::Rail)) else { continue };
            let stops = self.round_loads(fw);
            if stops.is_empty() {
                continue;
            }
            self.start_round(fw, at, to, stops);
        }
    }

    /// The loads one box takes: firm by firm in the order they came, up to
    /// `ROUND_STOPS` firms and a full box (a big load split, the rest left
    /// for the next round); each firm's loads made one.
    fn round_loads(&mut self, fw: BuildingId) -> Vec<ExportLoad> {
        let tick = self.clock.tick;
        let Some(f) = self.containers.forwarders.get_mut(&fw) else { return Vec::new() };
        let mut stops: Vec<ExportLoad> = Vec::new();
        let mut room = BOX_FULL;
        let mut keep = Vec::new();
        for l in std::mem::take(&mut f.pending) {
            if room <= 0.01 {
                keep.push(l);
                continue;
            }
            let i = stops.iter().position(|s| s.firm == l.firm);
            if i.is_none() && stops.len() >= cx::ROUND_STOPS {
                keep.push(l);
                continue;
            }
            let take = l.amount.min(room);
            room -= take;
            match i {
                Some(i) => stops[i].amount += take,
                None => stops.push(ExportLoad { amount: take, ..l }),
            }
            if l.amount - take > 0.01 {
                keep.push(ExportLoad { amount: l.amount - take, ..l });
            }
        }
        f.pending = keep;
        f.since = tick;
        f.rounds += 1;
        stops
    }

    /// A sound line empty to lease, at the bay depot nearest `p` that has
    /// one (not promised to a lorry, not going out).
    fn empty_to_lease(&self, p: Vec2) -> Option<(BuildingId, BoxId)> {
        let day = self.clock.day() as i64;
        let promised: std::collections::BTreeSet<BoxId> = self.containers.lorries.values().flat_map(|j| j.take.iter().copied()).collect();
        let mut best: Option<(f32, BuildingId, u64, BoxId)> = None;
        for (id, r) in self.containers.boxes.iter() {
            let Where::Stored(d) = r.at else { continue };
            if r.goods.is_some() || !r.export.is_empty() || r.owner != Owner::Line || r.bound_out || r.kind != crate::rail::BoxKind::Dry || r.wear(day) >= cx::SOUND_WEAR || promised.contains(&id) {
                continue;
            }
            if self.bay_depot(d).is_none() {
                continue;
            }
            let Some(dp) = self.building_lot(d).map(|l| l.center) else { continue };
            let k = (dp.distance(p), d, r.since, id);
            if best.is_none_or(|b| (k.0, k.1, k.2) < (b.0, b.1, b.2)) {
                best = Some(k);
            }
        }
        best.map(|b| (b.1, b.3))
    }

    fn start_round(&mut self, fw: BuildingId, at: Vec2, to: BuildingId, stops: Vec<ExportLoad>) {
        let tick = self.clock.tick;
        let round = Round { forwarder: fw, stops: stops.clone(), to };
        let lease = self.empty_to_lease(at);
        let car = match lease {
            Some((depot, id)) => {
                let Some(car) = self.spawn_box_lorry(fw, depot, fw) else {
                    self.return_loads(fw, stops);
                    return;
                };
                if let Some(r) = self.containers.boxes.get_mut(id) {
                    r.owner = Owner::Forwarder(fw);
                    r.note(tick, Event::Leased, fw);
                }
                self.transfer(Account::Business(fw), Account::Outside, cx::LEASE_PRICE, crate::bank::Kind::Supplies);
                if let Some(z) = self.buildings.get_mut(fw).and_then(|b| b.business.as_mut()) {
                    z.spend_today += cx::LEASE_PRICE;
                }
                let j = self.containers.lorries.entry(car).or_default();
                j.depot = depot;
                j.take = vec![id];
                j.take_to = None;
                j.round = Some(round);
                car
            }
            None => {
                // No sound empty in town: one bought new, on the lorry from
                // the forwarder's yard straight to the first firm.
                let first = stops[0].firm;
                let Some(car) = self.spawn_box_lorry(fw, first, fw) else {
                    self.return_loads(fw, stops);
                    return;
                };
                let forty = true;
                let id = self.make_box(crate::rail::BoxKind::Dry, forty, Where::Truck(car), None, Event::Leased, fw);
                if let Some(r) = self.containers.boxes.get_mut(id) {
                    r.owner = Owner::Forwarder(fw);
                    // A newer box: leased new from the line.
                    r.built = self.clock.day() as i32 - 200;
                    r.knocks = r.knocks.min(10);
                }
                self.transfer(Account::Business(fw), Account::Outside, cx::LEASE_PRICE, crate::bank::Kind::Supplies);
                self.containers.trucks.insert(car, TruckBoxes { boxes: vec![id], to: None });
                self.containers.lorries.insert(car, LorryJob { depot: to, round: Some(round), ..Default::default() });
                self.dress_truck(car);
                car
            }
        };
        let _ = car;
    }

    fn return_loads(&mut self, fw: BuildingId, stops: Vec<ExportLoad>) {
        if let Some(f) = self.containers.forwarders.get_mut(&fw) {
            f.pending.extend(stops);
            f.rounds = f.rounds.saturating_sub(1);
        }
    }

    /// [r8] A forwarder's books, for the inspector: (loads waiting, units
    /// waiting, lorries out, the books).
    pub fn forwarder_card(&self, fw: BuildingId) -> Option<(usize, f32, usize, Forwarder)> {
        let bz = self.buildings.get(fw)?.business.as_ref()?;
        if bz.kind != BusinessKind::FreightForwarder {
            return None;
        }
        let f = self.containers.forwarders.get(&fw).cloned().unwrap_or_default();
        let out = self.containers.lorries.values().filter(|j| j.round.as_ref().is_some_and(|r| r.forwarder == fw)).count();
        Some((f.pending.len(), f.pending.iter().map(|l| l.amount).sum(), out, f))
    }
}
