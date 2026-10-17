//! [attractions] The landmark attractions in the world: the rules for
//! where each may go (one per city, the pier on the sea shore, the
//! observatory on high ground), the grounds people walk round, the pier's
//! deck over the water, admissions and the daily books. The kinds and
//! their figures are `ServiceKind`'s; the data is `crate::attractions`.

use super::*;
use crate::attractions::{self as at, PierDeck};
use crate::services::ServiceKind;

impl World {
    /// The city's landmark of `kind` (standing, going up or clearing its
    /// site), if it has one: there is never more than one.
    pub fn landmark(&self, kind: ServiceKind) -> Option<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == kind)).map(|(id, _)| id).min()
    }

    /// [K16] The landmark already standing for a one-per-city attraction
    /// `what` (the Build window greys its button), or None: not a landmark,
    /// or none built yet.
    pub fn landmark_taken(&self, what: crate::services::Placeable) -> Option<BuildingId> {
        match what {
            crate::services::Placeable::Service(k, _) if k.is_landmark() => self.landmark(k),
            _ => None,
        }
    }

    /// The middle of the town's ground: the median ground height of every
    /// lot (as `splat_setting` measures the hills), or None on a flat map
    /// or a town of a handful of lots.
    pub fn town_median_height(&self) -> Option<f32> {
        if self.landscape.is_flat() || self.lots.len() < 8 {
            return None;
        }
        let mut hs: Vec<f32> = self.lots.values().map(|l| self.ground_height(l.center)).collect();
        hs.sort_by(|a, b| a.total_cmp(b));
        Some(hs[hs.len() / 2])
    }

    /// How far a site centred at `p` stands above the middle of the town,
    /// metres (None: a flat map).
    pub fn rise_above_town(&self, p: Vec2) -> Option<f32> {
        self.town_median_height().map(|mid| self.ground_height(p) - mid)
    }

    /// Why a landmark of `kind` cannot go on `lot` (a lot, or the site's
    /// lot about to be made), or None: one per city, the pier on the sea
    /// shore with open water off it, the observatory on high ground.
    pub(in crate::world) fn landmark_problem(&self, kind: ServiceKind, lot: &Lot) -> Option<String> {
        if let Some(b) = self.landmark(kind).and_then(|b| self.buildings.get(b)) {
            return Some(format!("one per city: {} is the city's {} already", b.name, kind.label()));
        }
        match kind {
            ServiceKind::Pier => at::plan_pier(&lot.polygon, &self.landscape).err(),
            ServiceKind::Observatory => match self.town_median_height() {
                None => Some("an observatory needs high ground: this map is flat".into()),
                Some(mid) => {
                    let rise = self.ground_height(lot.center) - mid;
                    (rise < at::OBSERVATORY_RISE).then(|| format!("an observatory needs high ground: this site is {rise:+.0} m from the middle of town ({mid:.0} m up), it needs {:+.0} m or more", at::OBSERVATORY_RISE))
                }
            },
            _ => None,
        }
    }

    /// A landmark has gone up on `lot` (`build_service`): the pier's deck
    /// over the water, the grounds people walk, a page in the books.
    pub(in crate::world) fn landmark_placed(&mut self, id: BuildingId, kind: ServiceKind) {
        let Some(lot) = self.buildings.get(id).map(|b| b.lot) else { return };
        self.attractions.books.entry(id).or_default();
        if kind == ServiceKind::Pier {
            if let Some(deck) = self.lots.get(lot).and_then(|l| at::plan_pier(&l.polygon, &self.landscape).ok()) {
                self.attractions.piers.insert(id, deck);
            }
        }
        // The joined plots each had their own pad: the site is levelled as
        // one (the pier's deck goes on with it, `add_pier_decks`).
        self.rebuild_ground();
    }

    /// Lay out the walkways and benches of every landmark people walk
    /// round (after the parks are planned, which clears every other lot's).
    pub(super) fn plan_attraction_grounds(&mut self) {
        let sites: Vec<(LotId, ServiceKind, Option<PierDeck>)> = self
            .buildings
            .iter()
            .filter_map(|(id, b)| {
                let k = b.service?.kind;
                (k.has_grounds() && self.lots.get(b.lot).is_some_and(|l| l.building == Some(id))).then(|| (b.lot, k, self.attractions.piers.get(&id).cloned()))
            })
            .collect();
        for (lot, k, pier) in &sites {
            let layout = at::plan_grounds(*k, &self.lots[*lot], pier.as_ref());
            self.lots[*lot].park = layout;
        }
        self.join_neighbouring_grounds(&sites.iter().map(|s| s.0).collect::<Vec<_>>());
    }

    /// [K10] Grounds side by side are walked as one: where an attraction's
    /// grounds (a zoo, a botanical garden, the grand park) meet another's
    /// or a park's, each side's walks run out to the seam and the fence
    /// opens there (`ParkLayout::entrances`), the walk graph joining the
    /// two ends (`WalkGraph` build). Not a beach (it opens only onto a
    /// park by its own rules) nor the pier.
    fn join_neighbouring_grounds(&mut self, sites: &[LotId]) {
        let walked = |l: &crate::zoning::Lot| l.park.as_ref().is_some_and(|p| !p.paths.is_empty()) && !l.shore.beach;
        let mut others: Vec<LotId> = self.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park && walked(l)).map(|(id, _)| id).collect();
        others.extend(sites.iter().copied().filter(|id| self.lots.get(*id).is_some_and(walked)));
        others.sort();
        others.dedup();
        let mut joins: Vec<(LotId, Vec2)> = Vec::new();
        for &a in sites {
            let Some(la) = self.lots.get(a).filter(|l| walked(l)) else { continue };
            for &b in &others {
                if b == a || (sites.contains(&b) && b < a) {
                    continue;
                }
                let Some(lb) = self.lots.get(b) else { continue };
                if let Some(m) = grounds_seam(&la.polygon, &lb.polygon) {
                    joins.push((a, m));
                    joins.push((b, m));
                }
            }
        }
        for (id, m) in joins {
            let Some(l) = self.lots.get_mut(id) else { continue };
            let poly = l.polygon.clone();
            let centre = l.center;
            let Some(layout) = l.park.as_mut() else { continue };
            let end = crate::park::pull_inside(&poly, m, centre);
            let Some(onto) = layout.paths.iter().flatten().copied().min_by(|p, q| p.distance(end).total_cmp(&q.distance(end))) else { continue };
            if onto.distance(end) < 0.5 || layout.entrances.iter().any(|e| e.distance(end) < 1.0) {
                continue;
            }
            layout.paths.push(vec![end, onto]);
            layout.entrances.push(end);
        }
    }

    /// Every pier's deck on the built ground, so walkers on it stand on
    /// the boards rather than the water (`World::surface_height`).
    pub(in crate::world) fn add_pier_decks(&mut self) {
        let mut ids: Vec<BuildingId> = self.attractions.piers.keys().copied().collect();
        ids.sort();
        for id in ids {
            self.add_pier_deck(id);
        }
    }

    fn add_pier_deck(&mut self, id: BuildingId) {
        let (Some(deck), Some(lot)) = (self.attractions.piers.get(&id).cloned(), self.buildings.get(id).map(|b| b.lot)) else { return };
        let z = self.lot_pad(lot) + at::PIER_DECK_RISE;
        for (a, b, hw) in deck.spans() {
            self.ground.decks.push((a, b, hw, z, z));
        }
    }

    /// A pier bulldozed: its deck leaves the built ground.
    pub(in crate::world) fn remove_pier(&mut self, id: BuildingId) {
        let Some(deck) = self.attractions.piers.remove(&id) else { return };
        let spans = deck.spans();
        self.ground.decks.retain(|d| !spans.iter().any(|(a, b, _)| d.0 == *a && d.1 == *b));
    }

    /// Where visitors stand to take in a landmark: out on the pier's head,
    /// along the walk of a park, zoo or garden, at the door of the rest.
    pub fn attraction_spot(&self, b: BuildingId) -> Option<Vec2> {
        let bld = self.buildings.get(b)?;
        let kind = bld.service?.kind;
        if let Some(d) = self.attractions.piers.get(&b) {
            return Some(d.tip - d.seaward * (d.head - 4.0));
        }
        if kind.has_grounds() {
            let path = self.lots.get(bld.lot).and_then(|l| l.park.as_ref()).and_then(|p| p.paths.first());
            if let Some(p) = path.and_then(|p| p.get(p.len() / 2)) {
                return Some(*p);
            }
        }
        self.building_entrance(b)
    }

    /// A visit to a landmark: what it costs at the door, if `purse` can
    /// pay (to the treasury), counted in its books. Returns what was paid.
    pub(in crate::world) fn admit_visitor(&mut self, b: BuildingId, tourist: bool, purse: f32) -> f32 {
        let Some(kind) = self.buildings.get(b).and_then(|x| x.service).map(|s| s.kind).filter(|k| k.is_landmark()) else { return 0.0 };
        let fee = kind.admission();
        let paid = if purse >= fee { fee } else { 0.0 };
        if paid > 0.0 {
            self.economy.credit_item(Flow::Tickets, paid, Item::Building(b));
        }
        let book = self.attractions.books.entry(b).or_default();
        if tourist {
            book.tourists_today += 1;
        } else {
            book.visits_today += 1;
        }
        book.visits_total += 1;
        book.takings_today += paid;
        paid
    }

    /// Is this a landmark the town's people go to for `lu` (a day out for
    /// `Park`, a night out for `Entertainment`), open now with room?
    pub(in crate::world) fn landmark_for_outing(&self, b: &Building, lu: LandUse, hour: u8) -> bool {
        let Some(s) = b.service.filter(|s| s.kind.is_landmark()) else { return false };
        let fits = match lu {
            LandUse::Park => s.kind.day_out(),
            LandUse::Entertainment => s.kind.night_out(),
            _ => false,
        };
        fits && b.operating() && b.opening_hours().contains(hour) && (b.visitors as u32) < s.kind.capacity(s.tier) && b.workers.len() * 4 >= b.jobs as usize
    }

    /// Once a day: the books close; a landmark gone takes its page.
    pub(super) fn daily_attractions(&mut self) {
        let live = &self.buildings;
        self.attractions.books.retain(|b, _| live.contains_key(*b));
        self.attractions.piers.retain(|b, _| live.contains_key(*b));
        for book in self.attractions.books.values_mut() {
            book.roll();
        }
    }
}

/// [K10] Where two grounds meet: the middle of the longest stretch (six
/// metres or more) along which their outlines run side by side, at most
/// [`GROUNDS_SEAM_GAP`] apart (two sites placed beside each other leave a
/// sliver between them).
pub(crate) fn grounds_seam(a: &[Vec2], b: &[Vec2]) -> Option<Vec2> {
    let mut best: Option<(f32, Vec2)> = None;
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        let len = p.distance(q);
        if len < 1e-3 {
            continue;
        }
        let dir = (q - p) / len;
        for j in 0..b.len() {
            let (r, s) = (b[j], b[(j + 1) % b.len()]);
            let off = |x: Vec2| (x - p).perp_dot(dir).abs();
            if off(r) > GROUNDS_SEAM_GAP || off(s) > GROUNDS_SEAM_GAP {
                continue;
            }
            let (tr, ts) = ((r - p).dot(dir), (s - p).dot(dir));
            let (lo, hi) = (tr.min(ts).max(0.0), tr.max(ts).min(len));
            if hi - lo >= 6.0 && best.is_none_or(|(l, _)| hi - lo > l) {
                best = Some((hi - lo, p + dir * ((lo + hi) * 0.5)));
            }
        }
    }
    best.map(|(_, m)| m)
}

/// [K10] Two grounds whose edges run this close count as side by side.
pub(crate) const GROUNDS_SEAM_GAP: f32 = 2.5;

