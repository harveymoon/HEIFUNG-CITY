//! [r5-layered] The bays under the raised roads (`crate::undercroft`): found
//! from the columns and the roads, given a use by the blocks around them,
//! run by stallholders and visited by people out for a stroll.

use super::*;
use crate::undercroft::*;

/// What a bay's ground must be clear of: lots, roads below the deck,
/// water and facilities, at a grid of points over it.
const SAMPLES_U: [f32; 3] = [-0.45, 0.0, 0.45];
const SAMPLES_V: [f32; 3] = [-0.4, 0.0, 0.4];

impl World {
    /// Signature of what the bays are found from: the columns, the raised
    /// pieces and the lots.
    fn under_key(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.pillars.len() * 3 + 4);
        for p in &self.pillars {
            words.push(p.at.x.to_bits() as u64);
            words.push(p.at.y.to_bits() as u64);
            words.push(p.level.to_bits() as u64);
        }
        words.push(self.roads.alive_segments().filter(|(_, s)| s.viaduct).count() as u64);
        words.push(self.roads.alive_segments().count() as u64);
        words.push(self.lots.len() as u64);
        words.push(self.facilities.len() as u64);
        crate::hash::fnv_words(words)
    }

    /// The bays under the decks, as the ground is now (`crate::undercroft`).
    pub fn find_bays(&self) -> Vec<Bay> {
        let mut out: Vec<Bay> = Vec::new();
        if self.pillars.is_empty() {
            return out;
        }
        let roads = &self.roads;
        let lot_list: Vec<&crate::zoning::Lot> = self.lots.values().collect();
        let lot_grid = crate::geom::Grid::new(48.0, lot_list.iter().map(|l| crate::geom::bounds(&l.polygon)));
        let flat = self.landscape.is_flat();
        let raised = |s: SegmentId| roads.try_segment(s).is_some_and(|sg| sg.alive && sg.viaduct && !sg.tunnel);
        let hw_of = |s: SegmentId| {
            let sg = roads.segment(s);
            deck_half_width(sg.kind, sg.lanes_fwd, sg.lanes_bwd)
        };
        // The piece a run carries on into past node `n` from `from`: the
        // node's only other live way, raised and of the same deck (a curve
        // is drawn as many short pieces; its bays run along the curve).
        let carry_on = |n: NodeId, from: SegmentId| -> Option<SegmentId> {
            let live: Vec<SegmentId> = roads.node(n).segments.iter().copied().filter(|&s| roads.segment(s).alive).collect();
            if live.len() != 2 {
                return None;
            }
            let next = live.into_iter().find(|&s| s != from)?;
            (raised(next) && roads.segment(next).kind == roads.segment(from).kind && (hw_of(next) - hw_of(from)).abs() < 0.05).then_some(next)
        };
        let mut pieces: Vec<SegmentId> = roads.alive_segments().filter(|(s, _)| raised(*s)).map(|(s, _)| s).collect();
        pieces.sort();
        let mut done: std::collections::BTreeSet<SegmentId> = Default::default();
        for first in pieces {
            if !done.insert(first) {
                continue;
            }
            let s = roads.segment(first);
            let mut nodes: std::collections::VecDeque<NodeId> = [s.a, s.b].into();
            let mut segs: Vec<SegmentId> = vec![first];
            for forward in [true, false] {
                let mut prev = first;
                loop {
                    let n = if forward { *nodes.back().unwrap() } else { *nodes.front().unwrap() };
                    let Some(next) = carry_on(n, prev) else { break };
                    if !done.insert(next) {
                        break;
                    }
                    let ns = roads.segment(next);
                    let far = if ns.a == n { ns.b } else { ns.a };
                    if forward {
                        nodes.push_back(far);
                    } else {
                        nodes.push_front(far);
                    }
                    segs.push(next);
                    prev = next;
                }
            }
            let pts: Vec<Vec2> = nodes.iter().map(|&n| roads.node(n).pos).collect();
            let lvs: Vec<f32> = nodes.iter().map(|&n| roads.node(n).level).collect();
            if lvs.iter().copied().fold(0.0, f32::max) < crate::roads::VIADUCT_LEVEL {
                continue;
            }
            let mut cum = vec![0.0f32];
            for w in pts.windows(2) {
                cum.push(cum.last().unwrap() + w[0].distance(w[1]));
            }
            let len = *cum.last().unwrap();
            if len < MIN_BAY {
                continue;
            }
            // A point, and the level, at arc length `t` along the run.
            let along = |t: f32| -> (Vec2, f32) {
                let t = t.clamp(0.0, len);
                let k = cum.partition_point(|&c| c <= t).clamp(1, pts.len() - 1) - 1;
                let seg = (cum[k + 1] - cum[k]).max(1e-3);
                let f = ((t - cum[k]) / seg).clamp(0.0, 1.0);
                (pts[k].lerp(pts[k + 1], f), lvs[k] + (lvs[k + 1] - lvs[k]) * f)
            };
            let hw = hw_of(first);
            // Column stations along the run, and its ends.
            let mut ts: Vec<f32> = vec![0.0, len];
            for p in &self.pillars {
                let mut best: Option<(f32, f32)> = None;
                for (k, w) in pts.windows(2).enumerate() {
                    let (f, q) = crate::roads::project_on_segment(p.at, w[0], w[1]);
                    let d = q.distance(p.at);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, cum[k] + f * (cum[k + 1] - cum[k])));
                    }
                }
                if let Some((d, t)) = best {
                    if d < hw + 0.8 {
                        ts.push(t);
                    }
                }
            }
            ts.sort_by(f32::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 0.5);
            let sid = first;
            for (c, l) in spans(&ts, len) {
                let (p0, l0) = along(c - l * 0.5);
                let (p1, l1) = along(c + l * 0.5);
                let (pos, lc) = along(c);
                let level = l0.min(l1).min(lc);
                let clearance = level - SOFFIT_DROP;
                if clearance < MIN_CLEARANCE {
                    continue;
                }
                let dir = (p1 - p0).normalize_or_zero();
                if dir == Vec2::ZERO {
                    continue;
                }
                let width = hw * 2.0;
                let free = |p: Vec2| {
                    if !flat && self.landscape.is_water(p) {
                        return false;
                    }
                    // A lot: only a park's ground (a plaza under the deck,
                    // like the parks under New York's bridges).
                    if lot_grid.in_box(p, p).into_iter().any(|i| {
                        let l = lot_list[i as usize];
                        l.contains(p) && !(l.land_use == LandUse::Park && l.building.and_then(|b| self.buildings.get(b)).is_none_or(|b| !b.is_special()))
                    }) {
                        return false;
                    }
                    if self.under_facility(p) {
                        return false;
                    }
                    // A way running under the deck (lower than it), with
                    // its pavements.
                    !roads.segments_in_box(p - Vec2::splat(30.0), p + Vec2::splat(30.0)).any(|o| {
                        if segs.contains(&o) {
                            return false;
                        }
                        let os = roads.segment(o);
                        if !os.alive || os.tunnel {
                            return false;
                        }
                        let (oa, ob) = roads.segment_points(o);
                        let (t, q) = crate::roads::project_on_segment(p, oa, ob);
                        let reach = os.half_width() + if os.is_street() { crate::zoning::SIDEWALK } else { 0.6 } + 0.8;
                        if q.distance(p) > reach {
                            return false;
                        }
                        let ol = roads.node(os.a).level + (roads.node(os.b).level - roads.node(os.a).level) * t;
                        ol < level - 2.0
                    })
                };
                // The whole span under the deck if its ground is free, else
                // the free half (a street or lots along one side), else a
                // strip down the middle; shorter if an end is taken.
                let side = [(0.0, width), (width * 0.25, width * 0.5), (-width * 0.25, width * 0.5), (0.0, width.min(3.2))];
                let fit = [l, l * 0.6].into_iter().flat_map(|ll| side.iter().map(move |&(v0, ww)| (ll, v0, ww))).find(|&(ll, v0, ww)| {
                    ww >= 2.6 && ll >= 5.5 && SAMPLES_U.iter().all(|&u| SAMPLES_V.iter().all(|&v| free(pos + dir * (u * ll) + dir.perp() * (v0 + v * ww))))
                });
                let Some((l, v0, width)) = fit else { continue };
                let pos = pos + dir.perp() * v0;
                if out.iter().any(|o| o.pos.distance(pos) < BAY_GAP || o.contains(pos, 0.0)) {
                    continue;
                }
                // [r5-layered] The headroom as built: on a hilly map the
                // deck's height over the ground is not its planned level
                // (the way is smoothed, the ground graded), so it is
                // measured, deck soffit to ground, over the bay; and the
                // ground must be near level for a floor.
                let clearance = if self.relief {
                    let mut room = f32::MAX;
                    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                    for &u in &SAMPLES_U {
                        for &v in &SAMPLES_V {
                            let q = pos + dir * (u * l) + dir.perp() * (v * width);
                            let g = self.ground_height(q);
                            lo = lo.min(g);
                            hi = hi.max(g);
                            if let Some(z) = self.ground.decks_at(q).into_iter().filter(|&z| z > g + 1.0).reduce(f32::min) {
                                room = room.min(z - g - SOFFIT_DROP - 0.2);
                            }
                        }
                    }
                    if hi - lo > 1.2 {
                        continue;
                    }
                    if room == f32::MAX { clearance } else { room }
                } else {
                    clearance
                };
                if clearance < MIN_CLEARANCE {
                    continue;
                }
                let park = lot_grid.in_box(pos, pos).into_iter().any(|i| lot_list[i as usize].contains(pos));
                let bay = Bay { id: bay_id(pos), pos, dir, len: l, width, level, clearance, walkway: s.kind == RoadKind::Path, park, ..Default::default() };
                let under = self.streets.of(sid).map(|st| st.name.clone()).unwrap_or_default();
                out.push(Bay { under, ..bay });
                if out.len() >= MAX_BAYS {
                    return out;
                }
            }
        }
        out
    }

    /// Find the bays again (the roads, columns or lots changed): a bay
    /// still there keeps its use, holders and tallies; one gone lets its
    /// people go.
    pub(super) fn refresh_bays(&mut self) {
        let found = self.find_bays();
        let mut old: BTreeMap<u64, Bay> = std::mem::take(&mut self.undercroft.bays).into_iter().map(|b| (b.id, b)).collect();
        let mut bays = Vec::with_capacity(found.len());
        for mut b in found {
            if let Some(o) = old.remove(&b.id) {
                b.use_ = o.use_;
                b.since_day = o.since_day;
                b.holders = o.holders;
                b.idle_days = o.idle_days;
                b.visits_today = o.visits_today;
                b.visits_yesterday = o.visits_yesterday;
                b.takings_today = o.takings_today;
                b.takings_yesterday = o.takings_yesterday;
                b.visits_total = o.visits_total;
            }
            bays.push(b);
        }
        let gone: Vec<u64> = old.keys().copied().collect();
        self.undercroft.trips.retain(|_, b| !gone.contains(b));
        self.undercroft.bays = bays;
        self.undercroft.key = self.under_key();
    }

    /// After a load: the bays as the saved ground has them.
    pub(super) fn undercroft_after_load(&mut self) {
        if !self.pillars.is_empty() || !self.undercroft.bays.is_empty() {
            self.refresh_bays();
        }
    }

    /// [r5-layered] Give bay `id` a use now (the remote API's dev lever):
    /// a trade takes stallholders from the blocks round it.
    pub fn set_bay_use(&mut self, id: u64, u: UnderUse) -> Result<(), String> {
        let i = self.undercroft.bays.iter().position(|b| b.id == id).ok_or("no bay with that id")?;
        let holders = if u.is_trade() {
            let bay = self.undercroft.bays[i].clone();
            let mut mine = bay.holders.clone();
            self.undercroft.bays[i].holders.clear();
            if mine.is_empty() {
                mine = self.pick_holders(&bay, if u == UnderUse::Market { MAX_HOLDERS } else { 2 });
            }
            if mine.is_empty() {
                return Err("nobody lives near enough to run it".into());
            }
            mine
        } else {
            Vec::new()
        };
        let day = self.clock.day();
        let b = &mut self.undercroft.bays[i];
        b.use_ = u;
        b.holders = holders;
        b.since_day = day;
        b.idle_days = 0;
        Ok(())
    }

    /// The bay whose ground holds `p`.
    pub fn bay_at(&self, p: Vec2, margin: f32) -> Option<&Bay> {
        self.undercroft.bays.iter().find(|b| b.contains(p, margin))
    }

    /// The bays under the decks of segment `sid`'s way near it.
    pub fn bays_under(&self, sid: SegmentId) -> Vec<&Bay> {
        let Some(s) = self.roads.try_segment(sid) else { return Vec::new() };
        let (a, b) = self.roads.segment_points(sid);
        let hw = deck_half_width(s.kind, s.lanes_fwd, s.lanes_bwd);
        self.undercroft.bays.iter().filter(|bay| crate::roads::project_on_segment(bay.pos, a, b).1.distance(bay.pos) < hw).collect()
    }

    /// People at (or on their way to) a bay now.
    pub fn bay_people(&self, id: u64) -> (usize, usize) {
        let Some(bay) = self.undercroft.bays.iter().find(|b| b.id == id) else { return (0, 0) };
        let mut here = 0;
        let mut coming = 0;
        for (cid, b) in &self.undercroft.trips {
            if *b != id {
                continue;
            }
            match self.citizens.get(*cid).map(|c| (&c.activity, c.pos)) {
                Some((Activity::Idle, p)) if bay.contains(p, 1.5) => here += 1,
                Some((Activity::Traveling { .. }, _)) => coming += 1,
                _ => {}
            }
        }
        (here, coming)
    }

    /// A walk to `p` that ends on the ground: to a point under a deck by
    /// way of the bay's edge (a route to the point itself may snap onto
    /// the deck's pavement overhead), else straight there.
    pub(crate) fn ground_route(&self, from: Vec2, p: Vec2) -> Option<Vec<Vec2>> {
        let via = self.bay_at(p, 1.0).map(|b| {
            let side = if (from - b.pos).dot(b.dir.perp()) >= 0.0 { 1.0 } else { -1.0 };
            let q = b.approach(side);
            // Out along the bay's spine to the edge, then in.
            (q, b.at((p - b.pos).dot(b.dir), side * (b.width * 0.5 - 0.6)))
        });
        match via {
            Some((q, edge)) => {
                let (mut path, _) = self.walk.route(from, q)?;
                path.push(q);
                path.push(edge);
                path.push(p);
                Some(path)
            }
            None => {
                let (mut path, _) = self.walk.route(from, p)?;
                path.push(p);
                Some(path)
            }
        }
    }

    /// What the blocks round a bay are like.
    fn bay_surroundings(&self, bay: &Bay, near: &[(u64, UnderUse, Vec2)]) -> Surroundings {
        let mut s = Surroundings { land_value: self.land_value.at(bay.pos), ..Default::default() };
        let r = Vec2::splat(REACH);
        for l in self.lots.values() {
            let d = l.center - bay.pos;
            if d.x.abs() > r.x || d.y.abs() > r.y || d.length() > REACH {
                continue;
            }
            let Some(b) = l.building.and_then(|b| self.buildings.get(b)) else { continue };
            if !b.operating() {
                continue;
            }
            s.homes += b.homes as u32;
            match b.land_use {
                LandUse::Commercial | LandUse::Entertainment => s.shop_jobs += b.jobs as u32,
                LandUse::Civic => s.office_jobs += b.jobs as u32,
                LandUse::Industrial => s.industry_jobs += b.jobs as u32,
                _ => {}
            }
        }
        s.street = self.roads.segments_in_box(bay.pos - Vec2::splat(40.0), bay.pos + Vec2::splat(40.0)).any(|o| {
            let os = self.roads.segment(o);
            if !os.alive || os.tunnel || !os.is_street() || os.viaduct || os.is_motorway() {
                return false;
            }
            let (oa, ob) = self.roads.segment_points(o);
            crate::roads::project_on_segment(bay.pos, oa, ob).1.distance(bay.pos) < 35.0
        });
        for (id, u, p) in near {
            if *id == bay.id {
                continue;
            }
            let d = p.distance(bay.pos);
            if d < 60.0 {
                s.same_near[*u as usize] = s.same_near[*u as usize].saturating_add(1);
            }
            if d < REACH {
                s.same_reach[*u as usize] = s.same_reach[*u as usize].saturating_add(1);
            }
        }
        s
    }

    /// Households to run a bay's stalls: from the homes within reach,
    /// those with nobody in work first, then the poorest; none that runs
    /// another bay.
    fn pick_holders(&self, bay: &Bay, n: usize) -> Vec<HouseholdId> {
        let taken: std::collections::BTreeSet<HouseholdId> = self.undercroft.bays.iter().flat_map(|b| b.holders.iter().copied()).collect();
        let mut pool: Vec<(bool, i64, HouseholdId)> = Vec::new();
        for (hid, hh) in self.households.iter() {
            if taken.contains(&hid) || hh.members.is_empty() {
                continue;
            }
            let Some(home) = hh.home.and_then(|b| self.building_lot(b)) else { continue };
            if home.center.distance(bay.pos) > REACH {
                continue;
            }
            let adult = hh.members.iter().filter_map(|m| self.citizens.get(*m)).any(|c| c.age >= crate::citizens::ADULT_AGE && c.age < 75);
            if !adult {
                continue;
            }
            let working = hh.members.iter().filter_map(|m| self.citizens.get(*m)).any(|c| c.work.is_some());
            pool.push((working, hh.money as i64, hid));
        }
        pool.sort();
        pool.into_iter().take(n).map(|(_, _, h)| h).collect()
    }

    /// At midnight: the day's tallies roll over, trades with no custom
    /// pack up, and bare bays the blocks round them want take a use.
    pub(super) fn daily_undercroft(&mut self) {
        if self.undercroft.bays.is_empty() && self.pillars.is_empty() {
            return;
        }
        self.refresh_bays();
        let day = self.clock.day();
        self.undercroft.last_visit.retain(|_, d| *d + 1 >= day);
        for b in self.undercroft.bays.iter_mut() {
            if b.use_.is_trade() {
                if b.visits_today == 0 && day > b.since_day {
                    b.idle_days = b.idle_days.saturating_add(1);
                } else {
                    b.idle_days = 0;
                }
            }
            b.visits_yesterday = b.visits_today;
            b.takings_yesterday = b.takings_today;
            b.visits_today = 0;
            b.takings_today = 0.0;
        }
        // Holders gone (moved out of town, lost their home): the stall
        // carries on with the rest; with none left it packs up.
        let households = &self.households;
        for b in self.undercroft.bays.iter_mut() {
            b.holders.retain(|h| households.get(*h).is_some_and(|hh| hh.home.is_some()));
            let packs_up = b.use_.is_trade() && (b.holders.is_empty() || b.idle_days >= IDLE_DAYS);
            if packs_up {
                b.use_ = UnderUse::Bare;
                b.holders.clear();
                b.idle_days = 0;
                b.since_day = day;
            }
        }
        let near: Vec<(u64, UnderUse, Vec2)> = self.undercroft.bays.iter().map(|b| (b.id, b.use_, b.pos)).collect();
        let camps: Vec<Vec2> = self.society.homeless.camps.iter().filter(|c| c.under).map(|c| c.pos).collect();
        let mut opened: Vec<(usize, UnderUse, Vec<HouseholdId>)> = Vec::new();
        for i in 0..self.undercroft.bays.len() {
            let bay = &self.undercroft.bays[i];
            if bay.use_ != UnderUse::Bare || crate::hash::hash01(bay.id, day, 0x7A4E) > TAKE_CHANCE || camps.iter().any(|&p| bay.contains(p, 1.0)) {
                continue;
            }
            // Uses taken earlier tonight count as neighbours too.
            let mut near_now = near.clone();
            for (k, u, _) in &opened {
                near_now[*k].1 = *u;
            }
            let s = self.bay_surroundings(bay, &near_now);
            let mut best: Option<(f32, UnderUse)> = None;
            for u in [UnderUse::Market, UnderUse::Hawkers, UnderUse::Workshop, UnderUse::Court] {
                // No lock-ups in a park.
                if bay.park && u == UnderUse::Workshop {
                    continue;
                }
                let score = suitability(u, &s, bay.clearance, bay.width, bay.len) + crate::hash::hash01(bay.id, day, u as u64) * 0.25;
                if score > 0.35 && best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, u));
                }
            }
            let Some((_, u)) = best else { continue };
            let holders = if u.is_trade() {
                let n = if u == UnderUse::Market { MAX_HOLDERS } else { 2 };
                let mut taken_now: Vec<HouseholdId> = opened.iter().flat_map(|o| o.2.iter().copied()).collect();
                let picked: Vec<HouseholdId> = self.pick_holders(bay, n + taken_now.len()).into_iter().filter(|h| !taken_now.contains(h)).take(n).collect();
                if picked.is_empty() {
                    continue;
                }
                taken_now.extend(picked.iter().copied());
                picked
            } else {
                Vec::new()
            };
            opened.push((i, u, holders));
        }
        for (i, u, holders) in opened {
            let b = &mut self.undercroft.bays[i];
            b.use_ = u;
            b.holders = holders;
            b.since_day = day;
            b.idle_days = 0;
        }
    }

    /// On the hour: the bays found again if the ground changed, camps
    /// under the decks marked, and the stallholders off to their stalls.
    pub(super) fn hourly_undercroft(&mut self) {
        if self.undercroft.bays.is_empty() && self.pillars.is_empty() {
            return;
        }
        if self.under_key() != self.undercroft.key {
            self.refresh_bays();
        }
        let day = self.clock.day();
        // Camps under a deck make the bay a camp; a camp gone leaves it bare.
        let camps: Vec<Vec2> = self.society.homeless.camps.iter().filter(|c| c.under).map(|c| c.pos).collect();
        for b in self.undercroft.bays.iter_mut() {
            let camp = camps.iter().any(|&p| b.contains(p, 1.0));
            if camp && b.use_ != UnderUse::Shelter {
                b.use_ = UnderUse::Shelter;
                b.holders.clear();
                b.since_day = day;
            } else if !camp && b.use_ == UnderUse::Shelter {
                b.use_ = UnderUse::Bare;
                b.since_day = day;
            }
        }
        // Who is no longer at (or on the way to) their bay.
        let bays: BTreeMap<u64, Bay> = self.undercroft.bays.iter().map(|b| (b.id, b.clone())).collect();
        let citizens = &self.citizens;
        self.undercroft.trips.retain(|cid, bid| {
            let Some(bay) = bays.get(bid) else { return false };
            match citizens.get(*cid).map(|c| (&c.activity, c.pos)) {
                Some((Activity::Idle, p)) => bay.contains(p, 1.5),
                Some((Activity::Traveling { to: None, purpose: Purpose::Wander, .. }, _)) => true,
                _ => false,
            }
        });
        // The stallholders' shifts: each holder's turn starts four hours
        // after the one before, from opening.
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        let mut sends: Vec<(CitizenId, Vec2, u64, u64, String)> = Vec::new();
        for bay in bays.values() {
            if !bay.use_.is_trade() {
                continue;
            }
            let (open, close) = bay.use_.hours();
            for (k, hid) in bay.holders.iter().enumerate() {
                let start = open as u64 + 4 * k as u64;
                if start >= close as u64 || hour as u64 != start {
                    continue;
                }
                let end = (start + 5).min(close as u64);
                let until = tick + (end - start) * 60;
                let Some(hh) = self.households.get(*hid) else { continue };
                let who = hh.members.iter().copied().find(|m| {
                    self.citizens.get(*m).is_some_and(|c| c.age >= crate::citizens::ADULT_AGE && c.age < 75 && !c.asleep && matches!(c.activity, Activity::AtHome | Activity::Idle) && c.inside.is_none_or(|b| Some(b) == hh.home))
                });
                let Some(cid) = who else { continue };
                let thought = match bay.use_ {
                    UnderUse::Market => "Setting the stall out under the flyover.",
                    UnderUse::Hawkers => "Wok's hot. Lunch crowd soon.",
                    _ => "Shutter up, tools out.",
                };
                sends.push((cid, bay.holder_spot(k), until, bay.id, thought.to_string()));
            }
        }
        for (cid, spot, until, bid, thought) in sends {
            if self.walk_to_under(cid, spot, until, &thought) {
                self.undercroft.trips.insert(cid, bid);
            }
        }
    }

    /// Walk someone out to `spot` under a deck, to stay until `until`.
    fn walk_to_under(&mut self, cid: CitizenId, spot: Vec2, until: u64, thought: &str) -> bool {
        let from = match self.citizens[cid].inside.and_then(|b| self.building_entrance(b)) {
            Some(e) => e,
            None => self.citizens[cid].pos,
        };
        let Some(path) = self.ground_route(from, spot) else { return false };
        let exit = self.leave_building(cid);
        let c = &mut self.citizens[cid];
        if let Some(p) = exit {
            c.pos = p;
            c.prev_pos = p;
        }
        c.asleep = false;
        c.mode = crate::citizens::TravelMode::Walk;
        c.transit = None;
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
        c.next_decision_tick = until;
        c.thought = thought.to_string();
        true
    }

    /// A stroll that stops at an open bay near the walker instead (the
    /// hawkers for a meal, the market, the workshops with a bike or car to
    /// see to, the court for the young). True when they set off.
    pub(super) fn under_stroll(&mut self, id: CitizenId) -> bool {
        if self.undercroft.bays.is_empty() {
            return false;
        }
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get(id) else { return false };
        if c.home.is_none() || crate::hash::hash01(c.seed, tick, 0x5B7A) > STROLL_CHANCE {
            return false;
        }
        // Once a day at most.
        let day = self.clock.day();
        if self.undercroft.last_visit.get(&id).is_some_and(|&d| d == day) {
            return false;
        }
        // How full each bay is (people there and on the way).
        let mut crowd: BTreeMap<u64, usize> = BTreeMap::new();
        for b in self.undercroft.trips.values() {
            *crowd.entry(*b).or_default() += 1;
        }
        let here = match c.inside.and_then(|b| self.building_entrance(b)) {
            Some(e) => e,
            None => c.pos,
        };
        let young = (10..36).contains(&c.age);
        let wheels = c.owns_car || c.owns_bike;
        let hungry = c.satiety < 0.6;
        let mut best: Option<(f32, usize)> = None;
        for (i, b) in self.undercroft.bays.iter().enumerate() {
            if !b.open_at(hour) {
                continue;
            }
            let d = b.pos.distance(here);
            if d > STROLL_REACH {
                continue;
            }
            let want = match b.use_ {
                UnderUse::Hawkers => if hungry { 1.4 } else { 0.7 },
                UnderUse::Market => 0.9,
                UnderUse::Workshop if wheels => 0.6,
                UnderUse::Court if young => 1.1,
                _ => continue,
            };
            if b.use_.spend() > 0.0 && c.savings < b.use_.spend() * 0.5 {
                continue;
            }
            if crowd.get(&b.id).copied().unwrap_or(0) >= b.use_.capacity(b.use_.units(b.len)) + b.holders.len() {
                continue;
            }
            let score = want - d / STROLL_REACH + crate::hash::hash01(c.seed ^ b.id, tick, 0x3C) * 0.5;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, i));
            }
        }
        let Some((_, i)) = best else { return false };
        let bay = self.undercroft.bays[i].clone();
        let spot = bay.spot(crate::hash::mix(c.seed, tick));
        let thought = match bay.use_ {
            UnderUse::Hawkers => "Noodles under the flyover. Cheap and hot.",
            UnderUse::Market => "Have a look round the stalls under the deck.",
            UnderUse::Workshop => "The lads under the bridge can fix that.",
            _ => "Few hoops under the deck.",
        };
        let until = tick + 240;
        if !self.walk_to_under(id, spot, until, thought) {
            return false;
        }
        self.undercroft.trips.insert(id, bay.id);
        self.undercroft.last_visit.insert(id, day);
        true
    }

    /// Reached a bay: pay, eat, play; stay a while. False when this was no
    /// bay visit (or the bay shut on the way), for the usual arrival.
    pub(super) fn under_arrive(&mut self, id: CitizenId) -> bool {
        let Some(&bid) = self.undercroft.trips.get(&id) else { return false };
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        let Some(i) = self.undercroft.bays.iter().position(|b| b.id == bid) else {
            self.undercroft.trips.remove(&id);
            return false;
        };
        let pos = self.citizens[id].pos;
        let bay = self.undercroft.bays[i].clone();
        if !bay.contains(pos, 2.0) {
            self.undercroft.trips.remove(&id);
            return false;
        }
        let holder_k = bay.holders.iter().position(|h| self.citizens[id].household == Some(*h));
        let c = &self.citizens[id];
        let seed = c.seed;
        // A stallholder at their stall: stands there till the shift ends.
        if holder_k.is_some() {
            let until = c.next_decision_tick.max(tick + 30);
            let c = &mut self.citizens[id];
            c.activity = Activity::Idle;
            c.inside = None;
            c.next_decision_tick = until;
            let d = bay.pos - c.pos;
            c.heading = d.y.atan2(d.x) + std::f32::consts::PI;
            c.prev_heading = c.heading;
            return true;
        }
        if !bay.open_at(hour) {
            self.undercroft.trips.remove(&id);
            let c = &mut self.citizens[id];
            c.activity = Activity::Idle;
            c.inside = None;
            c.thought = "Shutters down already.".to_string();
            c.next_decision_tick = tick + 5;
            return true;
        }
        // What it costs, by the visitor's means.
        let tab = crate::world::places::tab_factor(c.class());
        let amount = (bay.use_.spend() * tab * (0.6 + 0.8 * crate::hash::hash01(seed, tick, 0x91))).min(c.savings.max(0.0));
        let paid = if amount >= 1.0 && !bay.holders.is_empty() {
            let h = bay.holders[(crate::hash::mix(seed, tick) % bay.holders.len() as u64) as usize];
            let kind = if bay.use_ == UnderUse::Hawkers { Kind::Outings } else { Kind::Shopping };
            let a = self.transfer(Account::Person(id), Account::Household(h), amount * (1.0 - STOCK_SHARE), kind);
            let b = self.transfer(Account::Person(id), Account::Outside, amount * STOCK_SHARE, Kind::Imports);
            a + b
        } else {
            0.0
        };
        let under = if bay.under.is_empty() { if bay.walkway { "the walkway".to_string() } else { "the flyover".to_string() } } else { bay.under.clone() };
        let (thought, memory) = match bay.use_ {
            UnderUse::Hawkers => ("Plastic stool, bowl of noodles, the traffic overhead.", format!("Ate at the hawker stalls under {under}.")),
            UnderUse::Market => ("Haggled at the stalls under the deck.", format!("Browsed the market under {under}.")),
            UnderUse::Workshop => ("They had it fixed while I waited.", format!("Got the wheels seen to at the lock-ups under {under}.")),
            _ => ("Shooting hoops under the deck, the lights on the concrete.", format!("Played ball on the court under {under}.")),
        };
        let stay = bay.use_.stay() + (crate::hash::mix(seed, tick ^ 0x55) % 20);
        let household = self.citizens[id].household;
        {
            let c = &mut self.citizens[id];
            c.activity = Activity::Idle;
            c.inside = None;
            c.next_decision_tick = tick + stay;
            c.thought = thought.to_string();
            match bay.use_ {
                UnderUse::Hawkers => {
                    c.satiety = (c.satiety + 0.5).min(1.0);
                    c.mood = (c.mood + 0.02).min(1.0);
                }
                UnderUse::Court => c.mood = (c.mood + 0.04).min(1.0),
                _ => c.mood = (c.mood + 0.02).min(1.0),
            }
            let d = bay.pos - c.pos;
            if d.length_squared() > 0.01 {
                c.heading = d.y.atan2(d.x);
                c.prev_heading = c.heading;
            }
            if crate::hash::hash01(seed, bid, 0x4E) < 0.3 {
                c.remember(tick, memory);
            }
        }
        // Market produce goes home in a bag.
        if bay.use_ == UnderUse::Market && paid > 0.0 {
            if let Some(hh) = household.and_then(|h| self.households.get_mut(h)) {
                hh.groceries += 1.0;
            }
        }
        let b = &mut self.undercroft.bays[i];
        b.visits_today += 1;
        b.visits_total += 1;
        b.takings_today += paid;
        self.undercroft.visits_total += 1;
        self.undercroft.takings_total += paid as f64;
        true
    }

    /// Bays a camp may shelter in: bare (or already a camp), on land not
    /// dear.
    pub(crate) fn under_camp_spots(&self) -> Vec<(Vec2, Vec2)> {
        self.undercroft
            .bays
            .iter()
            .filter(|b| !b.park && matches!(b.use_, UnderUse::Bare | UnderUse::Shelter) && self.land_value.at(b.pos) < crate::homeless::UNDER_CAMP_LV)
            .map(|b| (b.at(0.0, b.width * 0.12), b.dir))
            .collect()
    }
}
