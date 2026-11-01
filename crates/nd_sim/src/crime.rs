//! Crime: a few citizens turn to it, commit offences, get arrested (or not),
//! serve time (or are let go early) and either go straight or carry on.
//!
//! - **Who turns.** Every hour each adult not already offending rolls a
//!   small chance, built from poverty (class), being out of work or on the
//!   street, hunger, unhappiness, cheap land at home, police coverage there,
//!   their personality (a boldness from the seed), youth, and the number of
//!   offenders living nearby: the contagion that lets crime spread through a
//!   poor district nobody polices. Rich, covered districts produce almost
//!   none. At most `MAX_SHARE` of adults offend at once.
//! - **What they do.** Each active offender (not in custody) may strike in an
//!   hour, more at night, less under police coverage: theft from a business
//!   (cash and stock), burglary of a home (household money), assault (the
//!   victim taken ill through the services' medical hook) or, rarely,
//!   murder, and vandalism (building condition). Every offence goes to
//!   `World::report_crime`, so the 911 call, the police response and the
//!   arrest are the services' own.
//! - **Arrest, jail, release.** The services hold the custody record; this
//!   system watches it. A jail sentence often ends a career (more so the
//!   longer it was); an early release from a police cell (no jail in town)
//!   almost never does, and emboldens. Released offenders who strike again
//!   count towards the reoffending rate.
//! - **Aftermath.** Victims remember. Offences leave heat at a hot spot,
//!   which decays day by day, pulls land value down and frightens the
//!   people living nearby (mood); the better-off move out of hot streets.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::{Activity, CitizenId, LifeStage};
use crate::land_value::LandValueBuilder;
use crate::services::{CrimeKind, ServiceKind};
use crate::zoning::LandUse;
use crate::World;
use glam::Vec2;
use rand::seq::IndexedRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ── balance ────────────────────────────────────────────────────────────────

/// Daily chance an adult turns to crime before the factors below.
pub const RECRUIT_PER_DAY: f32 = 0.006;
/// Poverty factor by class (low, middle, high).
pub const CLASS_FACTOR: [f32; 3] = [1.0, 0.22, 0.04];
/// Out of work (an adult with no job) and on the street.
pub const JOBLESS_FACTOR: f32 = 2.5;
pub const HOMELESS_FACTOR: f32 = 1.6;
/// Land value at home from which the neighbourhood adds nothing; at
/// `LV_ZERO - LV_SPAN` and below it adds the full factor of one.
/// [pt-growth] On the rescaled land value (1.0 is rare, a middling street
/// about 0.5): from 0.6 down, the full factor at 0.2.
pub const LV_ZERO: f32 = 0.6;
pub const LV_SPAN: f32 = 0.4;
/// How much full police coverage at home cuts recruitment.
pub const POLICE_RECRUIT_CUT: f32 = 0.8;
/// Contagion: each offender living within `PEER_RADIUS` adds this much
/// (up to `PEER_MAX` times the base).
pub const PEER_RADIUS: f32 = 140.0;
pub const PEER_FACTOR: f32 = 0.6;
pub const PEER_MAX: f32 = 4.0;
/// Never more than this share of adults offending at once.
pub const MAX_SHARE: f32 = 0.08;
/// Hourly chance an active offender strikes, before boldness, the night and
/// police deterrence where they are.
pub const OFFENCE_PER_HOUR: f32 = 0.012;
pub const NIGHT_FACTOR: f32 = 2.0;
pub const POLICE_DETER: f32 = 0.5;
/// Offenders strike within this distance of where they are.
pub const REACH: f32 = 260.0;
/// Share of assaults that end in a death.
pub const MURDER_SHARE: f32 = 0.06;
/// [murder] Who hears of a killing: neighbours living this close.
pub const GRIEF_RADIUS: f32 = 150.0;
/// [murder] Share of fights between enemies that end in a death.
pub const FEUD_KILL_CHANCE: f32 = 0.03;
/// Theft takes this share of a business's cash, at most `THEFT_MAX`, and up
/// to `THEFT_STOCK` units of what it sells.
pub const THEFT_SHARE: f32 = 0.06;
pub const THEFT_MAX: f32 = 80.0;
pub const THEFT_STOCK: f32 = 2.0;
/// Burglary takes this share of a household's money, at most `BURGLARY_MAX`.
pub const BURGLARY_SHARE: f32 = 0.25;
pub const BURGLARY_MAX: f32 = 100.0;
/// Condition vandalism knocks off a building, a range.
pub const VANDAL_DAMAGE: (f32, f32) = (0.02, 0.05);
/// Chance a jail term ends a career: a base plus this much per day served.
pub const JAIL_REFORM_BASE: f32 = 0.2;
pub const JAIL_REFORM_PER_DAY: f32 = 0.08;
/// Let go early from a police cell: hardly anyone stops.
pub const HELD_REFORM: f32 = 0.05;
/// Daily chance an offender gives it up: everyone, the employed, and more
/// for the employed who are not poor.
pub const DESIST_BASE: f32 = 0.01;
pub const DESIST_EMPLOYED: f32 = 0.04;
pub const DESIST_COMFORTABLE: f32 = 0.12;
/// Hot spots: offences within this distance share one; heat decays by this
/// factor every midnight and goes below `HEAT_FORGET`.
pub const HOTSPOT_MERGE: f32 = 60.0;
pub const HEAT_DECAY: f32 = 0.75;
pub const HEAT_FORGET: f32 = 0.2;
/// Heat that makes a hot spot (for the alert and the map).
pub const HOTSPOT_HEAT: f32 = 3.0;
/// Fear: homes within this distance of heat lose this much mood per heat a
/// day (heat capped at `FEAR_CAP`); from `FEAR_LEAVE` the better-off may leave.
pub const FEAR_RADIUS: f32 = 150.0;
pub const FEAR_MOOD: f32 = 0.012;
pub const FEAR_CAP: f32 = 5.0;
pub const FEAR_LEAVE: f32 = 4.0;
pub const FEAR_LEAVE_CHANCE: [f32; 3] = [0.0, 0.02, 0.06];
/// Land value lost per heat at a hot spot (radius `HEAT_LV_RADIUS`), and
/// around an offender's home.
pub const HEAT_LV: f32 = 0.025;
pub const HEAT_LV_RADIUS: f32 = 110.0;
pub const OFFENDER_HOME_LV: f32 = 0.01;

/// What an offender did; reported to the services as a `CrimeKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Offence {
    Theft,
    Burglary,
    Assault,
    Murder,
    Vandalism,
}

impl Offence {
    pub const ALL: [Offence; 5] = [Offence::Theft, Offence::Burglary, Offence::Assault, Offence::Murder, Offence::Vandalism];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn key(self) -> &'static str {
        match self {
            Offence::Theft => "theft",
            Offence::Burglary => "burglary",
            Offence::Assault => "assault",
            Offence::Murder => "murder",
            Offence::Vandalism => "vandalism",
        }
    }

    /// How the police record it.
    pub fn crime_kind(self) -> CrimeKind {
        match self {
            Offence::Theft => CrimeKind::Theft,
            Offence::Burglary => CrimeKind::Robbery,
            Offence::Assault => CrimeKind::Assault,
            Offence::Murder => CrimeKind::Homicide,
            Offence::Vandalism => CrimeKind::Vandalism,
        }
    }
}

/// Someone who offends.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offender {
    pub citizen: CitizenId,
    /// Tick they turned.
    pub since: u64,
    /// Personality: how often they strike, 0.4..1.8.
    pub boldness: f32,
    pub crimes: u32,
    pub arrests: u32,
    /// In custody at the last check, and whether that is a jail sentence.
    pub in_custody: bool,
    pub jailed: bool,
    pub custody_since: u64,
    /// Released at, and whether they have struck since.
    pub released_at: Option<u64>,
    pub reoffended: bool,
}

/// Where offences pile up.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Hotspot {
    pub pos: Vec2,
    pub heat: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CrimeStats {
    #[serde(deserialize_with = "crate::economy::padded")]
    pub offences_today: [u32; Offence::ALL.len()],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub offences_yesterday: [u32; Offence::ALL.len()],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub offences_total: [u32; Offence::ALL.len()],
    pub arrests_today: u32,
    pub arrests_total: u32,
    pub jailed_total: u32,
    pub released_total: u32,
    pub released_early_total: u32,
    pub reoffended_total: u32,
    pub reformed_total: u32,
    pub recruited_total: u32,
    pub killed_total: u32,
    pub stolen_today: f32,
    pub stolen_total: f32,
    pub fled_crime_total: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Crime {
    pub offenders: Vec<Offender>,
    pub hotspots: Vec<Hotspot>,
    pub stats: CrimeStats,
}

impl Crime {
    pub fn is_offender(&self, c: CitizenId) -> bool {
        self.offenders.iter().any(|o| o.citizen == c)
    }

    pub fn offender(&self, c: CitizenId) -> Option<&Offender> {
        self.offenders.iter().find(|o| o.citizen == c)
    }

    /// Offenders not in custody.
    pub fn active(&self) -> impl Iterator<Item = &Offender> {
        self.offenders.iter().filter(|o| !o.in_custody)
    }

    /// Share of released offenders who struck again.
    pub fn reoffend_rate(&self) -> f32 {
        if self.stats.released_total == 0 {
            0.0
        } else {
            self.stats.reoffended_total as f32 / self.stats.released_total as f32
        }
    }

    /// Heat within `r` of `p`, weighted by closeness.
    pub fn heat_at(&self, p: Vec2, r: f32) -> f32 {
        // [r6-perf] A spot a whole radius off along either axis (with a
        // margin for rounding in the distance) adds +0.0
        // (heat is positive), which leaves a sum of them unchanged: it is
        // skipped before the square root. Asked for every place a citizen
        // weighs going to (a 38,000-person city has 600 spots: 17 ms a
        // tick). The `+ 0.0` keeps the sum's zero positive, as the full sum
        // over any spot at all left it.
        let reach = r * 1.001;
        let s: f32 = self.hotspots.iter().filter(|h| (h.pos.x - p.x).abs() <= reach && (h.pos.y - p.y).abs() <= reach).map(|h| h.heat * (1.0 - h.pos.distance(p) / r).max(0.0)).sum();
        if self.hotspots.is_empty() { s } else { s + 0.0 }
    }

    pub fn offences_today(&self) -> u32 {
        self.stats.offences_today.iter().sum()
    }

    fn add_heat(&mut self, p: Vec2, w: f32) {
        match self.hotspots.iter_mut().filter(|h| h.pos.distance(p) < HOTSPOT_MERGE).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p))) {
            Some(h) => {
                // The spot drifts towards where the offences are.
                h.pos = h.pos.lerp(p, w / (h.heat + w));
                h.heat += w;
            }
            None => self.hotspots.push(Hotspot { pos: p, heat: w }),
        }
    }
}

/// Personality boldness from a citizen's seed, 0.4..1.8.
pub fn boldness_of(seed: u64) -> f32 {
    let h = seed.wrapping_mul(0x2545_F491_4F6C_DD1D).rotate_left(17);
    0.4 + (h % 1000) as f32 / 1000.0 * 1.4
}

/// What pushes one adult towards crime: the inputs to `recruit_chance`.
#[derive(Clone, Copy, Debug)]
pub struct Pressure {
    pub class: Tier,
    pub jobless: bool,
    pub homeless: bool,
    pub hungry_days: u8,
    pub mood: f32,
    pub land_value: f32,
    pub police: f32,
    pub peers: u32,
    pub boldness: f32,
    pub age: u8,
}

/// Daily chance someone under `p` turns to crime.
pub fn recruit_chance(p: &Pressure) -> f32 {
    let lv = ((LV_ZERO - p.land_value) / LV_SPAN).clamp(0.0, 1.2);
    let peers = (1.0 + PEER_FACTOR * p.peers as f32).min(PEER_MAX);
    let youth = if p.age < 30 { 1.3 } else if p.age > 50 { 0.6 } else { 1.0 };
    RECRUIT_PER_DAY
        * CLASS_FACTOR[p.class.index()]
        * if p.jobless { JOBLESS_FACTOR } else { 1.0 }
        * if p.homeless { HOMELESS_FACTOR } else { 1.0 }
        * (1.0 + 0.4 * p.hungry_days.min(4) as f32)
        * (1.3 - p.mood.clamp(0.0, 1.0))
        * lv
        * (1.0 - POLICE_RECRUIT_CUT * p.police.clamp(0.0, 1.0))
        * peers
        * p.boldness
        * youth
}

impl World {
    /// Where a citizen lives (their home's lot centre), or where they are.
    pub(crate) fn home_point(&self, c: CitizenId) -> Option<Vec2> {
        let c = self.citizens.get(c)?;
        Some(c.home.and_then(|h| self.building_lot(h)).map_or(c.pos, |l| l.center))
    }

    /// Police coverage of the lot nearest a point (the home's lot if known).
    fn police_at_home(&self, cid: CitizenId) -> f32 {
        let Some(c) = self.citizens.get(cid) else { return 0.0 };
        match c.home.and_then(|h| self.buildings.get(h)) {
            Some(b) => self.services.coverage_of(b.lot, ServiceKind::Police),
            None => self.coverage_at(c.pos, ServiceKind::Police),
        }
    }

    /// The pressures on one citizen right now.
    pub fn crime_pressure(&self, cid: CitizenId) -> Option<Pressure> {
        let at = self.home_point(cid)?;
        let peers = self.society.crime.offenders.iter().filter(|o| o.citizen != cid).filter_map(|o| self.home_point(o.citizen)).filter(|p| p.distance(at) < PEER_RADIUS).count() as u32;
        self.pressure_with(cid, at, peers)
    }

    /// [`crime_pressure`](Self::crime_pressure) with the home point and the
    /// offenders living near it already known.
    fn pressure_with(&self, cid: CitizenId, at: Vec2, peers: u32) -> Option<Pressure> {
        let c = self.citizens.get(cid)?;
        Some(Pressure {
            class: c.class(),
            jobless: c.work.is_none() && c.can_work(),
            homeless: c.home.is_none(),
            hungry_days: c.hungry_days,
            mood: c.mood,
            land_value: self.land_value.at(at),
            police: self.police_at_home(cid),
            peers,
            boldness: boldness_of(c.seed),
            age: c.age,
        })
    }

    /// Make someone an offender (tests, and the hourly roll).
    pub fn turn_to_crime(&mut self, cid: CitizenId) -> bool {
        if self.society.crime.is_offender(cid) {
            return false;
        }
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get_mut(cid) else { return false };
        c.remember(tick, "Started running with a bad crowd. Easy money.");
        let boldness = boldness_of(c.seed);
        self.society.crime.offenders.push(Offender { citizen: cid, since: tick, boldness, crimes: 0, arrests: 0, in_custody: false, jailed: false, custody_since: 0, released_at: None, reoffended: false });
        self.society.crime.stats.recruited_total += 1;
        true
    }

    // ── the hourly round ───────────────────────────────────────────────────

    pub(crate) fn hourly_crime(&mut self) {
        crate::prof_scope!("hourly_crime");
        self.track_custody();
        self.recruit_offenders();
        self.commit_offences();
    }

    /// Watch the services' custody record: arrests, sentences, releases.
    fn track_custody(&mut self) {
        crate::prof_scope!("track_custody");
        let tick = self.clock.tick;
        let mut reform: Vec<usize> = Vec::new();
        for k in 0..self.society.crime.offenders.len() {
            let o = self.society.crime.offenders[k].clone();
            let custody = self.services.custody.iter().find(|c| c.citizen == o.citizen).copied();
            match (o.in_custody, custody) {
                (false, Some(c)) => {
                    let s = &mut self.society.crime;
                    let x = &mut s.offenders[k];
                    x.in_custody = true;
                    x.jailed = c.jailed;
                    x.custody_since = tick;
                    x.arrests += 1;
                    s.stats.arrests_today += 1;
                    s.stats.arrests_total += 1;
                    if c.jailed {
                        s.stats.jailed_total += 1;
                    }
                }
                (true, None) => {
                    let days = tick.saturating_sub(o.custody_since) as f32 / crate::clock::MINUTES_PER_DAY as f32;
                    let chance = if o.jailed { JAIL_REFORM_BASE + JAIL_REFORM_PER_DAY * days } else { HELD_REFORM };
                    let roll: f32 = self.society_rng().random();
                    let s = &mut self.society.crime;
                    s.stats.released_total += 1;
                    if !o.jailed {
                        s.stats.released_early_total += 1;
                    }
                    let x = &mut s.offenders[k];
                    x.in_custody = false;
                    x.released_at = Some(tick);
                    x.reoffended = false;
                    if roll < chance {
                        reform.push(k);
                    } else if o.jailed {
                        x.boldness = (x.boldness * 0.8).max(0.4);
                    } else {
                        // Let go after a few hours: nothing to fear.
                        x.boldness = (x.boldness * 1.1).min(1.8);
                    }
                }
                _ => {}
            }
        }
        for k in reform.into_iter().rev() {
            let o = self.society.crime.offenders.remove(k);
            self.society.crime.stats.reformed_total += 1;
            if let Some(c) = self.citizens.get_mut(o.citizen) {
                c.remember(tick, "Done with that life. Not going back inside.");
            }
        }
        // Offenders who left town are off the books.
        let citizens = &self.citizens;
        self.society.crime.offenders.retain(|o| citizens.contains_key(o.citizen));
    }

    pub(crate) fn recruit_offenders(&mut self) {
        crate::prof_scope!("recruit_offenders");
        let adults = self.citizens.values().filter(|c| c.stage() == LifeStage::Adult).count();
        let cap = (adults as f32 * MAX_SHARE).ceil() as usize;
        let ids: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.stage() == LifeStage::Adult && !matches!(c.activity, Activity::Arriving)).map(|(id, _)| id).collect();
        // [perf] Offenders' homes bucketed by `PEER_RADIUS` cells, so each
        // adult counts the offenders near home from nine buckets rather
        // than the whole list (adults x offenders every hour). Buckets are
        // only counted, so their order does not matter; newly recruited
        // offenders join them as the old scan would have seen them.
        let cell_of = |p: Vec2| ((p.x / PEER_RADIUS).floor() as i32, (p.y / PEER_RADIUS).floor() as i32);
        let mut homes: HashMap<(i32, i32), Vec<(CitizenId, Vec2)>> = HashMap::new();
        let mut offenders: HashSet<CitizenId> = HashSet::new();
        // [review] Who is in custody, once, rather than a scan of the
        // custody list per adult (nobody is taken in during recruitment).
        let held: HashSet<CitizenId> = self.services.custody.iter().map(|x| x.citizen).collect();
        for o in &self.society.crime.offenders {
            offenders.insert(o.citizen);
            if let Some(p) = self.home_point(o.citizen) {
                homes.entry(cell_of(p)).or_default().push((o.citizen, p));
            }
        }
        for cid in ids {
            if self.society.crime.offenders.len() >= cap {
                break;
            }
            if offenders.contains(&cid) || held.contains(&cid) {
                continue;
            }
            let Some(at) = self.home_point(cid) else { continue };
            let (cx, cy) = cell_of(at);
            let mut peers = 0u32;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(v) = homes.get(&(cx + dx, cy + dy)) {
                        peers += v.iter().filter(|(o, p)| *o != cid && p.distance(at) < PEER_RADIUS).count() as u32;
                    }
                }
            }
            let Some(p) = self.pressure_with(cid, at, peers) else { continue };
            let chance = recruit_chance(&p) / 24.0;
            if chance <= 0.0 {
                continue;
            }
            if self.society_rng().random::<f32>() < chance && self.turn_to_crime(cid) {
                offenders.insert(cid);
                homes.entry(cell_of(at)).or_default().push((cid, at));
            }
        }
    }

    fn commit_offences(&mut self) {
        crate::prof_scope!("commit_offences");
        let hour = self.clock.hour();
        let night = !(6..21).contains(&hour);
        let active: Vec<(CitizenId, f32)> = self.society.crime.active().map(|o| (o.citizen, o.boldness)).collect();
        for (cid, boldness) in active {
            let Some(c) = self.citizens.get(cid) else { continue };
            if matches!(c.activity, Activity::Arriving | Activity::OnTrain { .. } | Activity::OnMetro { .. } | Activity::Riding { .. }) || self.is_patient(cid) {
                continue;
            }
            let pos = c.pos;
            let deter = 1.0 - POLICE_DETER * self.coverage_at(pos, ServiceKind::Police);
            let p = OFFENCE_PER_HOUR * boldness * if night { NIGHT_FACTOR } else { 1.0 } * deter;
            if self.society_rng().random::<f32>() < p {
                self.commit_offence(cid, None);
            }
        }
    }

    /// An offender strikes at a building within reach (or `at`): what they
    /// do follows the target. Returns the incident id.
    pub fn commit_offence(&mut self, cid: CitizenId, at: Option<BuildingId>) -> Option<u32> {
        let pos = self.citizens.get(cid)?.pos;
        let home = self.citizens.get(cid)?.home;
        let target = match at {
            Some(b) => b,
            None => {
                let near: Vec<BuildingId> = self
                    .buildings
                    .iter()
                    .filter(|(id, b)| Some(*id) != home && b.operating() && b.service.is_none() && b.land_use != LandUse::Park && !b.is_power() && b.rail.is_none())
                    .filter(|(_, b)| !b.residents.is_empty() || b.business.as_ref().is_some_and(|z| !z.closed))
                    .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| l.center.distance(pos) < REACH))
                    .map(|(id, _)| id)
                    .collect();
                *near.choose(self.society_rng())?
            }
        };
        let roll: f32 = self.society_rng().random();
        let b = self.buildings.get(target)?;
        let has_business = b.business.as_ref().is_some_and(|z| !z.closed);
        let mut offence = match (b.land_use, has_business, b.households.is_empty()) {
            (LandUse::Residential, _, false) => {
                if roll < 0.7 {
                    Offence::Burglary
                } else if roll < 0.85 {
                    Offence::Vandalism
                } else {
                    Offence::Assault
                }
            }
            (LandUse::Industrial, _, _) => {
                if roll < 0.5 && has_business {
                    Offence::Theft
                } else {
                    Offence::Vandalism
                }
            }
            (_, true, _) => {
                if roll < 0.7 {
                    Offence::Theft
                } else if roll < 0.85 {
                    Offence::Vandalism
                } else {
                    Offence::Assault
                }
            }
            _ => Offence::Vandalism,
        };
        if offence == Offence::Assault && self.society_rng().random::<f32>() < MURDER_SHARE {
            offence = Offence::Murder;
        }
        self.offend(cid, target, offence)
    }

    /// Carry out one offence and report it.
    pub fn offend(&mut self, cid: CitizenId, target: BuildingId, offence: Offence) -> Option<u32> {
        let tick = self.clock.tick;
        let (bname, at) = {
            let b = self.buildings.get(target)?;
            (b.name.clone(), self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center))
        };
        let present: Vec<CitizenId> = self.citizens.iter().filter(|(id, c)| *id != cid && c.inside == Some(target)).map(|(id, _)| id).collect();
        let regulars: Vec<CitizenId> = self.buildings[target].residents.iter().chain(self.buildings[target].workers.iter()).copied().filter(|c| *c != cid).collect();
        let mut victim: Option<CitizenId> = None;
        let mut stolen = 0.0;
        // [ledger] Cash taken, by whom it was taken from.
        let mut taken: Vec<(crate::bank::Account, f32)> = Vec::new();
        match offence {
            Offence::Theft => {
                victim = present.choose(self.society_rng()).copied();
                if let Some(z) = self.buildings[target].business.as_mut() {
                    let cash = (z.money.max(0.0) * THEFT_SHARE).min(THEFT_MAX);
                    taken.push((crate::bank::Account::Business(target), cash));
                    stolen += cash;
                    let sells: Vec<crate::market::Resource> = z.kind.recipe().outputs.iter().map(|(r, _)| *r).filter(|r| z.stock_of(*r) > 0.0).collect();
                    if let Some(&r) = sells.first() {
                        let take = z.stock_of(r).min(THEFT_STOCK);
                        if let Some(s) = z.stock.get_mut(&r) {
                            *s -= take;
                        }
                        stolen += take * r.base_price();
                    }
                }
                for w in self.buildings[target].workers.clone() {
                    if let Some(c) = self.citizens.get_mut(w) {
                        c.remember(tick, format!("Somebody cleaned out the till at {bname}."));
                        self.concerns.heard(w, c, crate::concerns::Concern::Safety, 0.3);
                        c.mood = (c.mood - 0.03).max(0.0);
                    }
                }
            }
            Offence::Burglary => {
                let hh: Vec<crate::citizens::HouseholdId> = self.buildings[target].households.clone();
                if let Some(&hid) = hh.choose(self.society_rng()) {
                    let members = self.households.get(hid).map(|h| h.members.clone()).unwrap_or_default();
                    if let Some(h) = self.households.get_mut(hid) {
                        let take = (h.money.max(0.0) * BURGLARY_SHARE).min(BURGLARY_MAX);
                        taken.push((crate::bank::Account::Household(hid), take));
                        stolen += take;
                    }
                    victim = members.first().copied();
                    for m in members {
                        if let Some(c) = self.citizens.get_mut(m) {
                            c.remember(tick, format!("Our place at {bname} got burgled."));
                            self.concerns.heard(m, c, crate::concerns::Concern::Safety, 0.6);
                            c.mood = (c.mood - 0.06).max(0.0);
                            c.thought = "They took everything that wasn't nailed down.".to_string();
                        }
                    }
                }
            }
            Offence::Assault | Offence::Murder => {
                victim = present.choose(self.society_rng()).copied().or_else(|| regulars.first().copied());
                let Some(v) = victim else {
                    return self.offend(cid, target, Offence::Vandalism);
                };
                if offence == Offence::Murder {
                    // Grief, the news and the report: `World::murder`, below.
                } else if let Some(c) = self.citizens.get_mut(v) {
                    c.remember(tick, format!("Got jumped at {bname}."));
                    self.concerns.heard(v, c, crate::concerns::Concern::Safety, 0.7);
                    c.mood = (c.mood - 0.12).max(0.0);
                    c.thought = "My ribs. I didn't even see him coming.".to_string();
                }
            }
            Offence::Vandalism => {
                let dmg = self.society_rng().random_range(VANDAL_DAMAGE.0..VANDAL_DAMAGE.1);
                let b = &mut self.buildings[target];
                let before = b.condition;
                b.condition = (b.condition - dmg).max(0.1);
                if (before * 10.0).floor() != (b.condition * 10.0).floor() {
                    self.events.push(crate::SimEvent::BuildingChanged(target));
                }
                for r in regulars.iter().take(12) {
                    if let Some(c) = self.citizens.get_mut(*r) {
                        c.remember(tick, format!("Somebody trashed the front of {bname}."));
                        self.concerns.heard(*r, c, crate::concerns::Concern::Safety, 0.2);
                        c.mood = (c.mood - 0.02).max(0.0);
                    }
                }
            }
        }
        // The offender's side of it.
        let (reoffend, first_since) = {
            let o = self.society.crime.offenders.iter_mut().find(|o| o.citizen == cid);
            match o {
                Some(o) => {
                    o.crimes += 1;
                    let again = o.released_at.is_some() && !o.reoffended;
                    if again {
                        o.reoffended = true;
                    }
                    (again, o.crimes == 1)
                }
                None => (false, false),
            }
        };
        let s = &mut self.society.crime;
        if reoffend {
            s.stats.reoffended_total += 1;
        }
        s.stats.offences_today[offence.index()] += 1;
        s.stats.offences_total[offence.index()] += 1;
        s.stats.stolen_today += stolen;
        s.stats.stolen_total += stolen;
        if offence != Offence::Murder {
            s.add_heat(at, 1.0);
        }
        // [ledger] The cash goes to the fence out of town; the thief's cut
        // of it and the stock comes back in.
        for (from, cash) in taken {
            self.transfer(from, crate::bank::Account::Outside, cash, crate::bank::Kind::Theft);
        }
        if stolen > 0.0 && self.citizens.contains_key(cid) {
            self.transfer(crate::bank::Account::Outside, crate::bank::Account::Person(cid), stolen * 0.5, crate::bank::Kind::Theft);
        }
        if let Some(c) = self.citizens.get_mut(cid) {
            c.thought = if first_since { "First job done. My hands are still shaking.".to_string() } else { "Nobody saw a thing.".to_string() };
            c.remember(tick, format!("Did {} at {bname}.", crate::names::a(offence.key())));
        }
        if offence == Offence::Murder {
            // A stranger's work: the police have no name to go on.
            return self.murder(cid, victim?, target, false);
        }
        let id = self.report_crime(offence.crime_kind(), target, Some(cid), victim);
        if offence == Offence::Assault {
            // Hurt: taken ill where they stand, an ambulance case.
            if let Some(v) = victim {
                if self.citizens.get(v).is_some_and(|c| c.inside.is_some()) && !self.is_patient(v) {
                    self.fall_ill(v);
                }
            }
        }
        id
    }

    // ── [murder] a killing ───────────────────────────────────────────────

    /// `killer` kills `victim` at `at` (an offender's job gone wrong, or a
    /// feud that went too far: `known`, the police know who to look for).
    /// Before the victim leaves the city the people in their life hear of
    /// it: household and friends grieve (memory, mood, a heavy safety
    /// concern), the place's regulars and the neighbours within
    /// `GRIEF_RADIUS` are shaken, and a line goes in the city news. Then
    /// it is reported as a homicide (a long investigation) and the victim
    /// is removed. Returns the incident. No new draws: the report's own.
    pub fn murder(&mut self, killer: CitizenId, victim: CitizenId, at: BuildingId, known: bool) -> Option<u32> {
        let tick = self.clock.tick;
        let (vname, household) = self.citizens.get(victim).map(|c| (c.name.clone(), c.household))?;
        let (bname, place) = {
            let b = self.buildings.get(at)?;
            (b.name.clone(), self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center))
        };
        let mut family: Vec<CitizenId> = household.and_then(|h| self.households.get(h)).map(|h| h.members.clone()).unwrap_or_default();
        family.retain(|m| *m != victim && *m != killer);
        let mut friends: Vec<CitizenId> = self.social.circles.get(&victim).map(|c| c.friends().map(|t| t.who).collect()).unwrap_or_default();
        friends.retain(|f| *f != killer && !family.contains(f));
        friends.sort();
        for m in &family {
            if let Some(c) = self.citizens.get_mut(*m) {
                c.remember(tick, format!("{vname} was killed at {bname}. Nothing is the same."));
                c.thought = format!("I keep expecting {vname} to walk in.");
                self.concerns.said(tick, *m, c, crate::concerns::Concern::Safety, 1.0, format!("{vname} was killed at {bname}. Who's next?"));
                c.mood = (c.mood - 0.3).max(0.0);
            }
        }
        for f in &friends {
            if let Some(c) = self.citizens.get_mut(*f) {
                c.remember(tick, format!("{vname} is dead. Killed at {bname}. I can't take it in."));
                self.concerns.heard(*f, c, crate::concerns::Concern::Safety, 0.9);
                c.mood = (c.mood - 0.18).max(0.0);
            }
        }
        // The place's regulars, and the neighbours.
        let regulars: Vec<CitizenId> = self.buildings[at].residents.iter().chain(self.buildings[at].workers.iter()).copied().collect();
        let near: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(id, _)| *id != victim && *id != killer && !family.contains(id) && !friends.contains(id))
            .filter(|(id, c)| regulars.contains(id) || c.home.and_then(|h| self.buildings.get(h)).and_then(|b| self.lots.get(b.lot)).is_some_and(|l| l.center.distance(place) <= GRIEF_RADIUS))
            .map(|(id, _)| id)
            .collect();
        for n in near {
            if let Some(c) = self.citizens.get_mut(n) {
                c.remember(tick, format!("Someone was killed at {bname}. Police everywhere."));
                self.concerns.heard(n, c, crate::concerns::Concern::Safety, 0.6);
                c.mood = (c.mood - 0.06).max(0.0);
            }
        }
        if let Some(c) = self.citizens.get_mut(killer) {
            c.remember(tick, format!("It went too far at {bname}. {vname} isn't getting up."));
            c.thought = "What have I done.".to_string();
        }
        self.social.push_news(tick, format!("🕯 {vname} was killed at {bname}. Police are investigating."), place, None);
        self.society.crime.add_heat(place, 3.0);
        self.society.crime.stats.killed_total += 1;
        let id = self.report_crime(CrimeKind::Homicide, at, Some(killer), Some(victim));
        if let Some(i) = id.and_then(|i| self.services.incident_mut(i)) {
            i.known_suspect = known;
        }
        // The victim is gone: the incident keeps their name.
        self.society_remove_citizen(victim);
        id
    }

    /// [murder] An arrest for a killing: the news, and the family hear.
    pub(crate) fn homicide_solved(&mut self, inc: &crate::services::Incident) {
        let tick = self.clock.tick;
        let name = |role: &str| inc.people.iter().find(|p| p.role == role).map(|p| p.name.clone());
        let (Some(killer), Some(vname)) = (name("suspect"), name("victim")) else { return };
        self.social.push_news(tick, format!("🕯 Police arrested {killer} for the killing of {vname}."), inc.pos, None);
        let grieving: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.memories.iter().any(|m| m.text.starts_with(&format!("{vname} was killed")) || m.text.starts_with(&format!("{vname} is dead")))).map(|(id, _)| id).collect();
        for g in grieving {
            if let Some(c) = self.citizens.get_mut(g) {
                c.remember(tick, format!("They arrested {killer} for what happened to {vname}."));
            }
        }
    }

    // ── midnight ───────────────────────────────────────────────────────────

    pub(crate) fn daily_crime(&mut self) {
        crate::prof_scope!("daily_crime");
        let tick = self.clock.tick;
        // Some give it up: a job helps, a decent wage more.
        let mut gone: Vec<usize> = Vec::new();
        for k in 0..self.society.crime.offenders.len() {
            let o = &self.society.crime.offenders[k];
            if o.in_custody {
                continue;
            }
            let Some(c) = self.citizens.get(o.citizen) else { continue };
            let mut p = DESIST_BASE;
            if c.work.is_some() {
                p += DESIST_EMPLOYED;
                if c.class() != Tier::Low {
                    p += DESIST_COMFORTABLE;
                }
            }
            if self.society_rng().random::<f32>() < p {
                gone.push(k);
            }
        }
        for k in gone.into_iter().rev() {
            let o = self.society.crime.offenders.remove(k);
            self.society.crime.stats.reformed_total += 1;
            if let Some(c) = self.citizens.get_mut(o.citizen) {
                c.remember(tick, "Steady work now. Keeping my head down.");
            }
        }
        // Fear: homes near hot spots sour; the better-off may move away.
        let hids: Vec<crate::citizens::HouseholdId> = self.households.keys().collect();
        for hid in hids {
            let Some(h) = self.households.get(hid) else { continue };
            let Some(home) = h.home else { continue };
            let Some(at) = self.building_lot(home).map(|l| l.center) else { continue };
            let heat = self.society.crime.heat_at(at, FEAR_RADIUS);
            if heat < 0.5 {
                continue;
            }
            let members = h.members.clone();
            let earners = members.iter().filter_map(|m| self.citizens.get(*m)).filter(|c| c.income > 0.0).count().max(1) as f32;
            let income: f32 = members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum();
            let class = Tier::for_income(income / earners);
            for &m in &members {
                if let Some(c) = self.citizens.get_mut(m) {
                    c.mood = (c.mood - FEAR_MOOD * heat.min(FEAR_CAP)).max(0.0);
                }
            }
            if heat >= FEAR_LEAVE && self.society_rng().random::<f32>() < FEAR_LEAVE_CHANCE[class.index()] {
                let street = self.buildings.get(home).map(|b| b.name.clone()).unwrap_or_default();
                for &m in &members {
                    if let Some(c) = self.citizens.get_mut(m) {
                        c.remember(tick, format!("Too much crime around {street}. We're getting out of this town."));
                        self.concerns.heard(m, c, crate::concerns::Concern::Safety, 0.9);
                    }
                }
                self.society.crime.stats.fled_crime_total += members.len() as u32;
                self.society_emigrate_household(hid, crate::economy::LeaveReason::Crime);
            }
        }
        let s = &mut self.society.crime;
        for h in s.hotspots.iter_mut() {
            h.heat *= HEAT_DECAY;
        }
        s.hotspots.retain(|h| h.heat >= HEAT_FORGET);
        s.stats.offences_yesterday = s.stats.offences_today;
        s.stats.offences_today = Default::default();
        s.stats.arrests_today = 0;
        s.stats.stolen_today = 0.0;
    }

    pub(crate) fn splat_crime(&self, b: &mut LandValueBuilder) {
        for h in &self.society.crime.hotspots {
            b.splat(h.pos, HEAT_LV_RADIUS, -HEAT_LV * h.heat.min(FEAR_CAP));
        }
        for o in &self.society.crime.offenders {
            if let Some(p) = self.citizens.get(o.citizen).and_then(|c| c.home).and_then(|h| self.building_lot(h)).map(|l| l.center) {
                b.splat(p, 60.0, -OFFENDER_HOME_LV);
            }
        }
    }
}
