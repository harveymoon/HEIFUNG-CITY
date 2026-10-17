//! [insight] The city's memory, for anyone asking afterwards (the remote
//! API's `inspect {.., history: true}`, `replay`, the selected car's "came
//! from" line): for every vehicle on the road its trip so far (the street
//! pieces it drove onto and when, every re-plan with why and what it
//! avoided, U-turns, stalls and what held it, anger, crashes, errands such
//! as the parking hunt); for every citizen their trips of the last day or
//! two (the mode chosen and the runner-up, each leg, routes that failed and
//! why) and their key life events; for every building what happened to it
//! (built, sold, opened and closed, households in and out, incidents).
//!
//! Recording only: nothing here feeds back into the simulation, and it
//! draws no random numbers. The traffic model leaves small notes at its
//! decision points (`Traffic::notes`, `Note`); `World::step_insight`
//! (`world/insight.rs`) drains them and diffs the rest from the state once
//! a tick.
//!
//! Compact and bounded: each log is a byte stream of records (a tag, the
//! ticks since the record before, the position as a delta in half metres,
//! then the record's own fields as varints), capped in bytes; a full log
//! drops its oldest street pieces first, then its oldest records (counted in
//! `Log::dropped`). The whole store is saved as one base64 string of that
//! binary (`Insight`'s serde), so it costs the save a few bytes a record,
//! and a store that will not decode loads empty rather than failing the save.

use crate::buildings::BuildingId;
use crate::citizens::{CitizenId, Purpose, TripMode};
use crate::traffic::{CarId, Wait};
use crate::vehicle::VehicleId;
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use slotmap::{Key, KeyData, SecondaryMap};
use std::collections::VecDeque;

/// Bytes a vehicle's trip log on the road may hold (a few hundred street
/// pieces and events).
pub const TRIP_BYTES: usize = 3072;
/// A finished trip is kept trimmed to this (its street pieces go first).
pub const DONE_TRIP_BYTES: usize = 1536;
/// Finished trips kept for vehicles nobody in town drives (lorries, buses,
/// service vehicles, through traffic), newest last.
pub const DONE_TRIPS: usize = 300;
/// Bytes of trips and legs kept per citizen (a day or two of an ordinary life).
pub const PERSON_BYTES: usize = 384;
/// Bytes of life events kept per citizen.
pub const LIFE_BYTES: usize = 192;
/// Bytes of events kept per building.
pub const BUILDING_BYTES: usize = 256;
/// A stall shorter than this (traffic seconds) is ordinary traffic, not recorded.
pub const STALL_MIN: f32 = 60.0;
/// Anger is recorded when it has moved this many percent since last recorded.
pub const ANGER_STEP: u8 = 10;
/// Free text in a record (a reason, a business kind) is cut to this many bytes.
const TEXT_MAX: usize = 80;
/// Format of the saved store.
const FORMAT: u8 = 1;

/// Why a vehicle took a new route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Stood at the head of its lane, unable to make the lane change its turn needs.
    LaneChange,
    /// Its exit beyond the junction stayed full.
    FullExit,
    /// A crash in the junction or just beyond it.
    Crash,
    /// A fresh route from where it stands: on one route too long, or a
    /// turn on it was closed (lane arrows changed).
    Stuck,
    /// Sent somewhere else by its own errand (`Errand`): the parking hunt,
    /// back to base, a tow job; refined by the errand when drained.
    Redirect,
    /// Looking for a parking space.
    ParkingHunt,
    /// A lorry, van or city vehicle going on to its next stop or back to base.
    Errand,
    /// A new route set from outside (a bus leaving a stop, a show, a chase).
    NewRoute,
    /// An emergency: a siren or a fugitive.
    Emergency,
}

impl Why {
    const ALL: [Why; 9] = [Why::LaneChange, Why::FullExit, Why::Crash, Why::Stuck, Why::Redirect, Why::ParkingHunt, Why::Errand, Why::NewRoute, Why::Emergency];

    pub fn key(self) -> &'static str {
        match self {
            Why::LaneChange => "lane_change",
            Why::FullExit => "full_exit",
            Why::Crash => "crash_ahead",
            Why::Stuck => "stuck",
            Why::Redirect => "redirect",
            Why::ParkingHunt => "parking_hunt",
            Why::Errand => "errand",
            Why::NewRoute => "new_route",
            Why::Emergency => "emergency",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Why::LaneChange => "could not get into the lane its turn needs",
            Why::FullExit => "the exit beyond the junction stayed full",
            Why::Crash => "a crash in the way",
            Why::Stuck => "a fresh route from where it stood (on the road too long, or a turn closed)",
            Why::Redirect => "sent on somewhere else",
            Why::ParkingHunt => "looking for a parking space",
            Why::Errand => "on to its next stop or back to base",
            Why::NewRoute => "a new route set (a bus at a stop, a show, a chase)",
            Why::Emergency => "an emergency (siren or fugitive)",
        }
    }
}

/// What the traffic model notes for the history as it happens
/// (`Traffic::notes`); the world drains them every tick.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// Drove onto a street piece (`segment` index, along it or against),
    /// entering at `at`.
    Entered { segment: u32, forward: bool, at: Vec2 },
    /// Took a new route (`after_m` `None`: looked and found none, kept the
    /// old one). `avoided`: where the lane or step it went round begins.
    Replan { why: Why, avoided: Option<Vec2>, before_m: f32, after_m: Option<f32> },
    /// [rage] An illegal mid-street U-turn by an angry driver.
    RageUTurn,
}

/// What kind of vehicle a trip is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TripKind {
    #[default]
    Other,
    /// A private car with its owner (or driver) at the wheel.
    Private,
    Lorry,
    Bus,
    /// Police, fire, ambulance, tow truck with lights on.
    Service,
    Taxi,
    /// From elsewhere to elsewhere along the highway.
    Through,
    /// A city or business vehicle on its rounds (vans, patrols, tow and garbage trucks).
    Fleet,
}

impl TripKind {
    const ALL: [TripKind; 8] = [TripKind::Other, TripKind::Private, TripKind::Lorry, TripKind::Bus, TripKind::Service, TripKind::Taxi, TripKind::Through, TripKind::Fleet];

    pub fn key(self) -> &'static str {
        match self {
            TripKind::Other => "other",
            TripKind::Private => "private",
            TripKind::Lorry => "lorry",
            TripKind::Bus => "bus",
            TripKind::Service => "service",
            TripKind::Taxi => "taxi",
            TripKind::Through => "through",
            TripKind::Fleet => "fleet",
        }
    }
}

/// How a trip left the road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Gone from the road (parked, arrived, off the map, towed away).
    Left,
}

/// A citizen's life event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Life {
    /// First seen in town (moved in, born, arrived).
    Arrived,
    /// A new home (`other`: the building), or homeless (`other` none).
    Home,
    /// A new job (`other`: the workplace), or out of work.
    Job,
    /// A car of their own (`other`: the vehicle), or none any more.
    Car,
    /// A new household.
    Household,
    /// The victim of a crime (`other`: none; `text`: the incident).
    CrimeVictim,
    /// Suspected of a crime.
    Suspect,
    /// Taken ill or hurt (a medical call).
    Patient,
    /// In a crash at the wheel.
    Crash,
}

impl Life {
    const ALL: [Life; 9] = [Life::Arrived, Life::Home, Life::Job, Life::Car, Life::Household, Life::CrimeVictim, Life::Suspect, Life::Patient, Life::Crash];

    pub fn key(self) -> &'static str {
        match self {
            Life::Arrived => "arrived",
            Life::Home => "home",
            Life::Job => "job",
            Life::Car => "car",
            Life::Household => "household",
            Life::CrimeVictim => "crime_victim",
            Life::Suspect => "suspect",
            Life::Patient => "patient",
            Life::Crash => "crash",
        }
    }
}

/// Something that happened to a building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Happened {
    /// First seen: a site going up (or placed).
    Started,
    /// Finished building.
    Built,
    /// Coming down.
    Demolition,
    /// A business opened (`text`: its kind).
    Opened,
    /// Its business closed.
    Closed,
    /// Its business reopened.
    Reopened,
    /// A different owner (`other`: the estate owner, none: independent).
    Owner,
    /// Rebuilt or refitted taller or at another tier (`text`).
    Rebuilt,
    /// Households moved in (`other`: how many).
    MovedIn,
    /// Households moved out (`other`: how many).
    MovedOut,
    /// An incident (`text`: its kind; `other`: its id).
    Incident,
    /// An incident here was settled (`text`: how; `other`: its id).
    Resolved,
    /// A new name (`text`).
    Renamed,
}

impl Happened {
    const ALL: [Happened; 13] = [Happened::Started, Happened::Built, Happened::Demolition, Happened::Opened, Happened::Closed, Happened::Reopened, Happened::Owner, Happened::Rebuilt, Happened::MovedIn, Happened::MovedOut, Happened::Incident, Happened::Resolved, Happened::Renamed];

    pub fn key(self) -> &'static str {
        match self {
            Happened::Started => "started",
            Happened::Built => "built",
            Happened::Demolition => "demolition",
            Happened::Opened => "opened",
            Happened::Closed => "closed",
            Happened::Reopened => "reopened",
            Happened::Owner => "owner",
            Happened::Rebuilt => "rebuilt",
            Happened::MovedIn => "moved_in",
            Happened::MovedOut => "moved_out",
            Happened::Incident => "incident",
            Happened::Resolved => "resolved",
            Happened::Renamed => "renamed",
        }
    }
}

/// What a citizen was doing, as recorded at each change (a leg of a trip,
/// or being somewhere): the `Activity` variant and the thing it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Act {
    pub tag: u8,
    /// The building, car, bus, train, drone or driver it is about (a key's
    /// ffi bits, 0: none).
    pub with: u64,
    /// Walk 0, bike 1 for a `Traveling` leg.
    pub mode: u8,
}

/// The `Activity` names, by `Act::tag`.
pub const ACT_NAMES: [&str; 16] = ["arriving", "at_home", "working", "visiting", "idle", "traveling", "driving", "sitting", "waiting", "riding_bus", "on_train", "flying", "on_metro", "passenger", "abroad", "unknown"];

/// One record of a log.
#[derive(Clone, Debug, PartialEq)]
pub struct Rec {
    pub tick: u64,
    pub at: Vec2,
    pub ev: Ev,
}

/// What a record says.
#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    // ── vehicles ──
    /// Took to the road.
    Start,
    Enter { segment: u32, forward: bool },
    Replan { why: Why, avoided: Option<Vec2>, before_m: f32, after_m: Option<f32> },
    /// Back the way it came on the same street: at a dead end (or a turning
    /// place), or a U-turn elsewhere.
    UTurn { dead_end: bool },
    RageUTurn,
    /// Stood still `seconds` (traffic seconds), held by `cause`, behind `blocker`.
    Stall { seconds: u32, cause: Wait, blocker: Option<CarId> },
    Anger { pct: u8 },
    Crash,
    /// Its errand changed (`crate::vehicle::Errand` tag, the hunt's tries,
    /// the building it names).
    Errand { what: u8, tries: u8, building: Option<BuildingId> },
    End(End),
    // ── people ──
    /// Set off: the mode chosen (`TripMode::index`, `AIR` for a drone),
    /// what for and where to, its expected minutes, the runner-up mode and
    /// its minutes, the modes tried first that could not be done (a bit per
    /// mode), and whether this is a later leg (after a ride) rather than a
    /// fresh trip.
    Trip { mode: u8, purpose: Purpose, to: Option<BuildingId>, minutes: f32, alt: Option<(u8, f32)>, failed: u8, leg: bool },
    Act(Act),
    /// No way there (`why`, as the no-route alert says it).
    NoRoute { to: Option<BuildingId>, why: String },
    Life { what: Life, other: u64, text: String },
    // ── buildings ──
    Happened { what: Happened, other: u64, text: String },
}

/// `Ev::Trip`'s mode for a drone flight.
pub const AIR: u8 = 100;

/// A mode index as a name.
pub fn mode_key(m: u8) -> &'static str {
    if m == AIR {
        return "air";
    }
    TripMode::ALL.get(m as usize).map_or("unknown", |t| t.key())
}

/// The `Wait` variants, in a fixed order for the codec.
const WAITS: [Wait; 12] = [Wait::Free, Wait::Leader, Wait::Signal, Wait::Yield, Wait::ExitFull, Wait::LaneChange, Wait::Dwell, Wait::Entering, Wait::Arriving, Wait::Crashed, Wait::Blocked, Wait::RailCrossing];

/// A wait as a key.
pub fn wait_key(w: Wait) -> &'static str {
    match w {
        Wait::Free => "free",
        Wait::Leader => "queued_behind",
        Wait::Signal => "red_light",
        Wait::Yield => "giving_way",
        Wait::ExitFull => "exit_full",
        Wait::LaneChange => "lane_change",
        Wait::Dwell => "at_stop",
        Wait::Entering => "pulling_out",
        Wait::Arriving => "arriving",
        Wait::Crashed => "crashed",
        Wait::Blocked => "junction_blocked",
        Wait::RailCrossing => "level_crossing",
    }
}

/// `Errand` tags, by `Ev::Errand::what`.
pub const ERRAND_NAMES: [&str; 18] = ["trip", "parking_search", "to_parking_lot", "to_kerb_space", "to_dealer", "delivered_to_owner", "leaving_town", "haul", "return_to_base", "patrol", "tow_job", "recover_wreck", "taxi", "through", "out", "other", "other", "other"];

/// A key as ffi bits (0: none).
pub fn ffi<K: Key>(k: Option<K>) -> u64 {
    k.filter(|k| !k.is_null()).map_or(0, |k| k.data().as_ffi())
}

/// A key from ffi bits (`None` for 0).
pub fn unffi<K: Key>(v: u64) -> Option<K> {
    (v != 0).then(|| KeyData::from_ffi(v).into())
}

// ── the codec ──

fn put(b: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        b.push((v as u8) | 0x80);
        v >>= 7;
    }
    b.push(v as u8);
}

fn zz(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzz(u: u64) -> i64 {
    ((u >> 1) as i64) ^ -((u & 1) as i64)
}

fn put_i(b: &mut Vec<u8>, v: i64) {
    put(b, zz(v));
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    let mut end = s.len().min(TEXT_MAX);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    put(b, end as u64);
    b.extend_from_slice(&s.as_bytes()[..end]);
}

/// Metres as a whole number (negative and unknown as 0).
fn metres(m: f32) -> u64 {
    if m.is_finite() && m > 0.0 { m.round() as u64 } else { 0 }
}

/// A position in half metres.
fn q(p: Vec2) -> [i64; 2] {
    let f = |x: f32| if x.is_finite() { (x * 2.0).round() as i64 } else { 0 };
    [f(p.x), f(p.y)]
}

fn unq(p: [i64; 2]) -> Vec2 {
    Vec2::new(p[0] as f32 * 0.5, p[1] as f32 * 0.5)
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }

    fn done(&self) -> bool {
        self.i >= self.b.len()
    }

    fn u(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let byte = *self.b.get(self.i)?;
            self.i += 1;
            if shift >= 64 {
                return None;
            }
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
        }
    }

    fn i(&mut self) -> Option<i64> {
        self.u().map(unzz)
    }

    fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }

    fn string(&mut self) -> Option<String> {
        let n = self.u()? as usize;
        Some(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
}

fn put_purpose(b: &mut Vec<u8>, p: Purpose) {
    let (tag, sub) = match p {
        Purpose::Home => (0, 0),
        Purpose::Work => (1, 0),
        Purpose::Visit(lu) => (2, LandUse::ALL.iter().position(|x| *x == lu).unwrap_or(0) as u8),
        Purpose::Shop(r) => (3, crate::market::Resource::ALL.iter().position(|x| *x == r).unwrap_or(0) as u8),
        Purpose::Wander => (4, 0),
        Purpose::Sit => (5, 0),
        Purpose::ToStop => (6, 0),
    };
    b.push(tag);
    b.push(sub);
}

fn get_purpose(r: &mut Rd) -> Option<Purpose> {
    let (tag, sub) = (r.byte()?, r.byte()? as usize);
    Some(match tag {
        0 => Purpose::Home,
        1 => Purpose::Work,
        2 => Purpose::Visit(*LandUse::ALL.get(sub)?),
        3 => Purpose::Shop(*crate::market::Resource::ALL.get(sub)?),
        4 => Purpose::Wander,
        5 => Purpose::Sit,
        _ => Purpose::ToStop,
    })
}

impl Ev {
    fn tag(&self) -> u8 {
        match self {
            Ev::Start => 0,
            Ev::Enter { .. } => 1,
            Ev::Replan { .. } => 2,
            Ev::UTurn { .. } => 3,
            Ev::RageUTurn => 4,
            Ev::Stall { .. } => 5,
            Ev::Anger { .. } => 6,
            Ev::Crash => 7,
            Ev::Errand { .. } => 8,
            Ev::End(_) => 9,
            Ev::Trip { .. } => 10,
            Ev::Act(_) => 11,
            Ev::NoRoute { .. } => 12,
            Ev::Life { .. } => 13,
            Ev::Happened { .. } => 14,
        }
    }

    /// The record's own fields; `at` is the record's position, which
    /// positions inside it are written relative to.
    fn put_body(&self, b: &mut Vec<u8>, at: [i64; 2]) {
        match self {
            Ev::Start | Ev::RageUTurn | Ev::Crash | Ev::End(_) => {}
            Ev::Enter { segment, forward } => put(b, ((*segment as u64) << 1) | *forward as u64),
            Ev::Replan { why, avoided, before_m, after_m } => {
                b.push(Why::ALL.iter().position(|w| w == why).unwrap_or(0) as u8 | if avoided.is_some() { 0x80 } else { 0 } | if after_m.is_some() { 0x40 } else { 0 });
                if let Some(a) = avoided {
                    let p = q(*a);
                    put_i(b, p[0] - at[0]);
                    put_i(b, p[1] - at[1]);
                }
                put(b, metres(*before_m));
                if let Some(m) = after_m {
                    put(b, metres(*m));
                }
            }
            Ev::UTurn { dead_end } => b.push(*dead_end as u8),
            Ev::Stall { seconds, cause, blocker } => {
                put(b, *seconds as u64);
                b.push(WAITS.iter().position(|w| w == cause).unwrap_or(0) as u8);
                put(b, ffi(*blocker));
            }
            Ev::Anger { pct } => b.push(*pct),
            Ev::Errand { what, tries, building } => {
                b.push(*what);
                b.push(*tries);
                put(b, ffi(*building));
            }
            Ev::Trip { mode, purpose, to, minutes, alt, failed, leg } => {
                b.push(*mode);
                put_purpose(b, *purpose);
                put(b, ffi(*to));
                put(b, metres(*minutes));
                match alt {
                    Some((m, min)) => {
                        b.push(*m);
                        put(b, metres(*min));
                    }
                    None => b.push(255),
                }
                b.push(*failed);
                b.push(*leg as u8);
            }
            Ev::Act(a) => {
                b.push(a.tag | (a.mode << 5));
                put(b, a.with);
            }
            Ev::NoRoute { to, why } => {
                put(b, ffi(*to));
                put_str(b, why);
            }
            Ev::Life { what, other, text } => {
                b.push(Life::ALL.iter().position(|w| w == what).unwrap_or(0) as u8);
                put(b, *other);
                put_str(b, text);
            }
            Ev::Happened { what, other, text } => {
                b.push(Happened::ALL.iter().position(|w| w == what).unwrap_or(0) as u8);
                put(b, *other);
                put_str(b, text);
            }
        }
    }

    fn get_body(tag: u8, r: &mut Rd, at: [i64; 2]) -> Option<Ev> {
        Some(match tag {
            0 => Ev::Start,
            1 => {
                let v = r.u()?;
                Ev::Enter { segment: (v >> 1) as u32, forward: v & 1 == 1 }
            }
            2 => {
                let flags = r.byte()?;
                let why = *Why::ALL.get((flags & 0x3f) as usize)?;
                let avoided = if flags & 0x80 != 0 { Some(unq([at[0] + r.i()?, at[1] + r.i()?])) } else { None };
                let before_m = r.u()? as f32;
                let after_m = if flags & 0x40 != 0 { Some(r.u()? as f32) } else { None };
                Ev::Replan { why, avoided, before_m, after_m }
            }
            3 => Ev::UTurn { dead_end: r.byte()? != 0 },
            4 => Ev::RageUTurn,
            5 => Ev::Stall { seconds: r.u()? as u32, cause: *WAITS.get(r.byte()? as usize)?, blocker: unffi(r.u()?) },
            6 => Ev::Anger { pct: r.byte()? },
            7 => Ev::Crash,
            8 => Ev::Errand { what: r.byte()?, tries: r.byte()?, building: unffi(r.u()?) },
            9 => Ev::End(End::Left),
            10 => {
                let mode = r.byte()?;
                let purpose = get_purpose(r)?;
                let to = unffi(r.u()?);
                let minutes = r.u()? as f32;
                let alt = match r.byte()? {
                    255 => None,
                    m => Some((m, r.u()? as f32)),
                };
                Ev::Trip { mode, purpose, to, minutes, alt, failed: r.byte()?, leg: r.byte()? != 0 }
            }
            11 => {
                let t = r.byte()?;
                Ev::Act(Act { tag: t & 0x1f, mode: t >> 5, with: r.u()? })
            }
            12 => Ev::NoRoute { to: unffi(r.u()?), why: r.string()? },
            13 => Ev::Life { what: *Life::ALL.get(r.byte()? as usize)?, other: r.u()?, text: r.string()? },
            14 => Ev::Happened { what: *Happened::ALL.get(r.byte()? as usize)?, other: r.u()?, text: r.string()? },
            _ => return None,
        })
    }
}

/// A bounded log: records as a byte stream (see the module notes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    /// The tick and position the stream's deltas start from.
    t0: u64,
    p0: [i64; 2],
    /// The last record's tick and position (where the next delta is from).
    t: u64,
    p: [i64; 2],
    bytes: Vec<u8>,
    /// Records dropped to keep within the cap.
    pub dropped: u32,
}

impl Log {
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Bytes held.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    fn write(&mut self, r: &Rec) {
        let p = q(r.at);
        if self.bytes.is_empty() {
            self.t0 = r.tick;
            self.p0 = p;
            self.t = r.tick;
            self.p = p;
        }
        let tick = r.tick.max(self.t);
        let b = &mut self.bytes;
        b.push(r.ev.tag());
        put(b, tick - self.t);
        put_i(b, p[0] - self.p[0]);
        put_i(b, p[1] - self.p[1]);
        r.ev.put_body(b, p);
        self.t = tick;
        self.p = p;
    }

    /// Add a record, trimming the oldest if the log grows past `cap` bytes.
    pub fn push(&mut self, r: Rec, cap: usize) {
        self.write(&r);
        if self.bytes.len() > cap {
            self.trim(cap * 3 / 4);
        }
    }

    /// The records, oldest first (a damaged stream stops where it breaks).
    pub fn records(&self) -> Vec<Rec> {
        let mut out = Vec::new();
        let mut r = Rd::new(&self.bytes);
        let (mut t, mut p) = (self.t0, self.p0);
        while !r.done() {
            let Some(rec) = (|| {
                let tag = r.byte()?;
                t += r.u()?;
                p = [p[0] + r.i()?, p[1] + r.i()?];
                let ev = Ev::get_body(tag, &mut r, p)?;
                Some(Rec { tick: t, at: unq(p), ev })
            })() else {
                break;
            };
            out.push(rec);
        }
        out
    }

    fn rebuild(&mut self, recs: &[Rec]) {
        let dropped = self.dropped;
        *self = Log { dropped, ..Default::default() };
        for r in recs {
            self.write(r);
        }
    }

    /// Down to at most `cap` bytes: the oldest street pieces go first (the
    /// path matters less than what happened on it), then the oldest records.
    pub fn trim(&mut self, cap: usize) {
        if self.bytes.len() <= cap {
            return;
        }
        let mut recs = self.records();
        let n = recs.len();
        let enters = recs.iter().filter(|r| matches!(r.ev, Ev::Enter { .. })).count();
        // Each record costs about the same: drop in proportion.
        let per = (self.bytes.len() as f32 / n.max(1) as f32).max(1.0);
        let mut excess = ((self.bytes.len() - cap) as f32 / per).ceil() as usize + 1;
        let mut out = Vec::with_capacity(n);
        let mut drop_enters = excess.min(enters.saturating_sub(1));
        excess -= drop_enters;
        for r in recs.drain(..) {
            if drop_enters > 0 && matches!(r.ev, Ev::Enter { .. }) {
                drop_enters -= 1;
                self.dropped += 1;
                continue;
            }
            out.push(r);
        }
        let skip = excess.min(out.len().saturating_sub(1));
        self.dropped += skip as u32;
        self.rebuild(&out[skip..]);
        // Records vary in size: make sure.
        while self.bytes.len() > cap && !self.bytes.is_empty() {
            let recs = self.records();
            if recs.len() <= 1 {
                break;
            }
            let k = (recs.len() / 4).max(1);
            self.dropped += k as u32;
            self.rebuild(&recs[k..]);
        }
    }

    fn encode(&self, b: &mut Vec<u8>) {
        put(b, self.t0);
        put_i(b, self.p0[0]);
        put_i(b, self.p0[1]);
        put(b, self.t - self.t0);
        put_i(b, self.p[0] - self.p0[0]);
        put_i(b, self.p[1] - self.p0[1]);
        put(b, self.dropped as u64);
        put(b, self.bytes.len() as u64);
        b.extend_from_slice(&self.bytes);
    }

    fn decode(r: &mut Rd) -> Option<Log> {
        let t0 = r.u()?;
        let p0 = [r.i()?, r.i()?];
        let t = t0 + r.u()?;
        let p = [p0[0] + r.i()?, p0[1] + r.i()?];
        let dropped = r.u()? as u32;
        let n = r.u()? as usize;
        let bytes = r.bytes(n)?.to_vec();
        Some(Log { t0, p0, t, p, bytes, dropped })
    }
}

/// What a vehicle's trip log keeps in mind between ticks to notice changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watch {
    /// A stall under way: since when (tick), what held it, behind whom, where.
    pub stall: Option<(u64, Wait, Option<CarId>, Vec2)>,
    pub anger: u8,
    pub crashed: bool,
    /// The errand last recorded: tag, tries, building bits.
    pub errand: (u8, u8, u64),
    /// The street piece last driven onto, and which way.
    pub last: Option<(u32, bool)>,
    /// Where it was at the last look: where its trip ends.
    pub pos: Vec2,
}

/// One vehicle's trip: who and what, and its log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trip {
    pub car: CarId,
    pub kind: TripKind,
    pub vehicle: Option<VehicleId>,
    pub driver: Option<CitizenId>,
    /// Where it was going when it set off.
    pub to: Option<BuildingId>,
    pub start: u64,
    pub end: Option<u64>,
    pub log: Log,
    pub watch: Watch,
}

impl Trip {
    fn encode(&self, b: &mut Vec<u8>) {
        put(b, ffi(Some(self.car)));
        b.push(TripKind::ALL.iter().position(|k| *k == self.kind).unwrap_or(0) as u8);
        put(b, ffi(self.vehicle));
        put(b, ffi(self.driver));
        put(b, ffi(self.to));
        put(b, self.start);
        put(b, self.end.map_or(0, |e| e + 1));
        self.log.encode(b);
        let w = &self.watch;
        match w.stall {
            Some((t, cause, blocker, at)) => {
                b.push(1);
                put(b, t);
                b.push(WAITS.iter().position(|x| *x == cause).unwrap_or(0) as u8);
                put(b, ffi(blocker));
                let p = q(at);
                put_i(b, p[0]);
                put_i(b, p[1]);
            }
            None => b.push(0),
        }
        b.push(w.anger);
        b.push(w.crashed as u8);
        b.push(w.errand.0);
        b.push(w.errand.1);
        put(b, w.errand.2);
        match w.last {
            Some((s, f)) => put(b, (((s as u64) << 1) | f as u64) + 1),
            None => put(b, 0),
        }
        let p = q(w.pos);
        put_i(b, p[0]);
        put_i(b, p[1]);
    }

    fn decode(r: &mut Rd) -> Option<Trip> {
        let car = unffi(r.u()?).unwrap_or_default();
        let kind = *TripKind::ALL.get(r.byte()? as usize)?;
        let vehicle = unffi(r.u()?);
        let driver = unffi(r.u()?);
        let to = unffi(r.u()?);
        let start = r.u()?;
        let end = r.u()?.checked_sub(1);
        let log = Log::decode(r)?;
        let stall = match r.byte()? {
            0 => None,
            _ => Some((r.u()?, *WAITS.get(r.byte()? as usize)?, unffi(r.u()?), unq([r.i()?, r.i()?]))),
        };
        let anger = r.byte()?;
        let crashed = r.byte()? != 0;
        let errand = (r.byte()?, r.byte()?, r.u()?);
        let last = r.u()?.checked_sub(1).map(|v| ((v >> 1) as u32, v & 1 == 1));
        let pos = unq([r.i()?, r.i()?]);
        Some(Trip { car, kind, vehicle, driver, to, start, end, log, watch: Watch { stall, anger, crashed, errand, last, pos } })
    }
}

/// What a citizen's log last saw, to notice changes (not saved: the first
/// tick after a load looks again without recording).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PersonSeen {
    pub act: Act,
    pub home: u64,
    pub work: u64,
    pub car: u64,
    pub household: u64,
}

/// One citizen's history.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Person {
    /// Trips and legs: the mode chosen, each change of activity, routes
    /// that failed.
    pub trips: Log,
    pub life: Log,
    /// The last drive they made, as its trip log (trimmed).
    pub last_drive: Option<Box<Trip>>,
    pub seen: Option<PersonSeen>,
}

/// What a building's log last saw (not saved, like `PersonSeen`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildingSeen {
    pub site: bool,
    pub demolishing: bool,
    /// Business kind label and closed.
    pub business: Option<(&'static str, bool)>,
    pub owner: u64,
    pub tier: u8,
    pub floors: u8,
    pub households: u16,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Place {
    pub log: Log,
    pub seen: Option<BuildingSeen>,
}

/// The store on the `World` (`World::insight`).
#[derive(Clone, Debug, Default)]
pub struct Insight {
    /// Trips of the vehicles on the road now.
    pub live: SecondaryMap<CarId, Trip>,
    /// Finished trips of vehicles nobody in town drives, newest last.
    pub done: VecDeque<Trip>,
    pub people: SecondaryMap<CitizenId, Person>,
    pub buildings: SecondaryMap<BuildingId, Place>,
    /// The last incident id recorded, and the ones still open.
    pub last_incident: u32,
    pub open_incidents: Vec<u32>,
    /// False until the first look after a load or a new city: that look
    /// takes in who and what there is without recording it as news.
    pub synced: bool,
}

impl Insight {
    /// The trip of a car on the road, or its last finished one.
    pub fn trip_of(&self, car: CarId) -> Option<&Trip> {
        if let Some(t) = self.live.get(car) {
            return Some(t);
        }
        self.done.iter().rev().find(|t| t.car == car).or_else(|| self.people.values().filter_map(|p| p.last_drive.as_deref()).find(|t| t.car == car))
    }

    /// Bytes held in the logs (the save costs about 4/3 of this).
    pub fn bytes(&self) -> usize {
        let trips = self.live.values().chain(self.done.iter()).map(|t| t.log.size()).sum::<usize>();
        let people = self.people.values().map(|p| p.trips.size() + p.life.size() + p.last_drive.as_ref().map_or(0, |t| t.log.size())).sum::<usize>();
        let places = self.buildings.values().map(|p| p.log.size()).sum::<usize>();
        trips + people + places
    }

    /// The whole store as bytes (see the module notes).
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![FORMAT];
        put(&mut b, self.live.len() as u64);
        for t in self.live.values() {
            t.encode(&mut b);
        }
        put(&mut b, self.done.len() as u64);
        for t in &self.done {
            t.encode(&mut b);
        }
        let people: Vec<(CitizenId, &Person)> = self.people.iter().filter(|(_, p)| !p.trips.is_empty() || !p.life.is_empty() || p.last_drive.is_some()).collect();
        put(&mut b, people.len() as u64);
        for (id, p) in people {
            put(&mut b, ffi(Some(id)));
            p.trips.encode(&mut b);
            p.life.encode(&mut b);
            match &p.last_drive {
                Some(t) => {
                    b.push(1);
                    t.encode(&mut b);
                }
                None => b.push(0),
            }
        }
        let places: Vec<(BuildingId, &Place)> = self.buildings.iter().filter(|(_, p)| !p.log.is_empty()).collect();
        put(&mut b, places.len() as u64);
        for (id, p) in places {
            put(&mut b, ffi(Some(id)));
            p.log.encode(&mut b);
        }
        put(&mut b, self.last_incident as u64);
        put(&mut b, self.open_incidents.len() as u64);
        for &i in &self.open_incidents {
            put(&mut b, i as u64);
        }
        b
    }

    /// The store from `encode`'s bytes; `None` if they do not decode.
    pub fn decode(bytes: &[u8]) -> Option<Insight> {
        let mut r = Rd::new(bytes);
        if r.byte()? != FORMAT {
            return None;
        }
        let mut s = Insight::default();
        for _ in 0..r.u()? {
            let t = Trip::decode(&mut r)?;
            s.live.insert(t.car, t);
        }
        for _ in 0..r.u()? {
            s.done.push_back(Trip::decode(&mut r)?);
        }
        for _ in 0..r.u()? {
            let id: CitizenId = unffi(r.u()?)?;
            let trips = Log::decode(&mut r)?;
            let life = Log::decode(&mut r)?;
            let last_drive = match r.byte()? {
                0 => None,
                _ => Some(Box::new(Trip::decode(&mut r)?)),
            };
            s.people.insert(id, Person { trips, life, last_drive, seen: None });
        }
        for _ in 0..r.u()? {
            let id: BuildingId = unffi(r.u()?)?;
            s.buildings.insert(id, Place { log: Log::decode(&mut r)?, seen: None });
        }
        s.last_incident = r.u()? as u32;
        for _ in 0..r.u()? {
            s.open_incidents.push(r.u()? as u32);
        }
        Some(s)
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(B64[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `base64` back to bytes; `None` on anything else.
pub fn unbase64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for c in s.chunks(4) {
        let mut n = 0u32;
        for (k, &ch) in c.iter().enumerate() {
            n |= val(ch)? << (18 - 6 * k);
        }
        out.push((n >> 16) as u8);
        if c.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if c.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

impl Serialize for Insight {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64(&self.encode()))
    }
}

impl<'de> Deserialize<'de> for Insight {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // A store that will not decode loads empty: the city still loads.
        let s = String::deserialize(d)?;
        Ok(unbase64(&s).and_then(|b| Insight::decode(&b)).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(tick: u64, x: f32, ev: Ev) -> Rec {
        Rec { tick, at: Vec2::new(x, -x), ev }
    }

    #[test]
    fn records_round_trip_through_the_stream() {
        let mut log = Log::default();
        let evs = vec![
            rec(10, 100.0, Ev::Start),
            rec(10, 101.5, Ev::Enter { segment: 4321, forward: true }),
            rec(12, 140.0, Ev::Replan { why: Why::FullExit, avoided: Some(Vec2::new(150.0, -70.0)), before_m: 812.0, after_m: Some(990.0) }),
            rec(13, 140.0, Ev::Replan { why: Why::LaneChange, avoided: None, before_m: 20.0, after_m: None }),
            rec(15, 90.0, Ev::UTurn { dead_end: true }),
            rec(16, 90.0, Ev::Stall { seconds: 240, cause: Wait::ExitFull, blocker: Some(KeyData::from_ffi((3 << 32) | 77).into()) }),
            rec(17, 90.0, Ev::Anger { pct: 40 }),
            rec(18, 90.0, Ev::Errand { what: 1, tries: 2, building: None }),
            rec(19, -20.0, Ev::Trip { mode: 4, purpose: Purpose::Shop(crate::market::Resource::ALL[3]), to: None, minutes: 22.0, alt: Some((2, 31.0)), failed: 0b100, leg: false }),
            rec(19, -20.0, Ev::Act(Act { tag: 6, with: (1 << 32) | 5, mode: 0 })),
            rec(20, -20.0, Ev::NoRoute { to: None, why: "the grid doesn't connect".into() }),
            rec(21, -20.0, Ev::Life { what: Life::Job, other: 99, text: String::new() }),
            rec(22, -20.0, Ev::Happened { what: Happened::Opened, other: 0, text: "noodle bar".into() }),
            rec(30, 0.0, Ev::End(End::Left)),
        ];
        for r in &evs {
            log.push(r.clone(), 10_000);
        }
        assert_eq!(log.records(), evs);
        // And through the store's own encoding.
        let mut b = Vec::new();
        log.encode(&mut b);
        assert_eq!(Log::decode(&mut Rd::new(&b)).unwrap(), log);
    }

    #[test]
    fn a_full_log_drops_its_oldest_street_pieces_first() {
        let mut log = Log::default();
        log.push(rec(0, 0.0, Ev::Start), 400);
        for i in 0..400u32 {
            log.push(rec(i as u64, i as f32 * 20.0, Ev::Enter { segment: i * 37, forward: i % 2 == 0 }), 400);
            if i == 5 {
                log.push(rec(5, 100.0, Ev::Replan { why: Why::Stuck, avoided: None, before_m: 1.0, after_m: Some(2.0) }), 400);
            }
        }
        assert!(log.size() <= 400, "bounded: {}", log.size());
        let recs = log.records();
        assert!(log.dropped > 0);
        assert!(recs.iter().any(|r| matches!(r.ev, Ev::Replan { .. })), "the re-plan outlived the old street pieces");
        assert!(matches!(recs.last().unwrap().ev, Ev::Enter { segment, .. } if segment == 399 * 37), "the newest kept");
    }

    #[test]
    fn the_store_survives_base64_and_a_bad_one_loads_empty() {
        let mut s = Insight::default();
        let car: CarId = KeyData::from_ffi((1 << 32) | 3).into();
        let mut t = Trip { car, kind: TripKind::Private, start: 5, ..Default::default() };
        t.log.push(rec(5, 1.0, Ev::Start), TRIP_BYTES);
        t.watch.anger = 30;
        t.watch.stall = Some((7, Wait::Signal, None, Vec2::new(1.0, 2.0)));
        s.live.insert(car, t.clone());
        s.done.push_back(t.clone());
        let who: CitizenId = KeyData::from_ffi((1 << 32) | 9).into();
        let mut p = Person::default();
        p.life.push(rec(1, 0.0, Ev::Life { what: Life::Arrived, other: 0, text: String::new() }), LIFE_BYTES);
        p.last_drive = Some(Box::new(t.clone()));
        s.people.insert(who, p);
        s.last_incident = 12;
        s.open_incidents = vec![11, 12];
        let text = ron::to_string(&s).unwrap();
        let back: Insight = ron::from_str(&text).unwrap();
        assert_eq!(back.live.get(car), Some(&t));
        assert_eq!(back.done.len(), 1);
        assert_eq!(back.people.get(who).unwrap().life.records().len(), 1);
        assert_eq!(back.people.get(who).unwrap().last_drive.as_deref(), Some(&t));
        assert_eq!(back.open_incidents, vec![11, 12]);
        let bad: Insight = ron::from_str("\"not base64 at all!\"").unwrap();
        assert!(bad.live.is_empty());
        assert_eq!(unbase64(&base64(b"ab")).unwrap(), b"ab");
        assert_eq!(unbase64(&base64(b"abcd")).unwrap(), b"abcd");
    }
}
