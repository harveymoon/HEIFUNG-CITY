//! City services and emergencies: the data. Police, fire, health, schools,
//! a 911 dispatch centre and two attractions (monuments, stadiums), each
//! placed by the player in one of three tiers; the vehicles they run; the
//! incidents (fires, crimes, people taken ill) those vehicles answer; and
//! the catalogue the Buildings tool offers. The systems that drive all of
//! this live in `world/services.rs`.
//!
//! The life of an incident: it *starts* (a fire catches, a crime happens,
//! someone collapses), somebody nearby *calls 911* after a short delay, a
//! dispatcher at a dispatch centre *assigns* the nearest free unit (slower
//! without one on shift), the vehicle *drives* there over the street
//! network with lights and sirens, *works* on scene, *resolves* it (fire
//! out, an arrest, a patient to hospital) and *returns* to its station.
//! Unanswered fires grow, spread to neighbours and can burn a building down.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::CitizenId;
use crate::traffic::CarId;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;

pub use crate::placeable::{parse_tier, BuildCategory, Placeable};

mod chase;
mod incident;
mod kinds;
mod stats;
mod units;

pub use chase::*;
pub use incident::*;
pub use kinds::*;
pub use stats::*;
pub use units::*;

new_key_type! {
    pub struct UnitId;
}

// ── balance ────────────────────────────────────────────────────────────────

/// Coverage fades to nothing over the last this-many metres of a station's
/// road radius: inside `radius - COVERAGE_FADE` a lot is fully covered.
pub const COVERAGE_FADE: f32 = 200.0;
/// A lot counts as covered (for the coverage share and the alerts) at this.
pub const COVERED: f32 = 0.5;
/// Hourly chance a building catches fire, by tier, before condition, land
/// use and fire coverage. Cheap, worn buildings burn.
pub const FIRE_RISK: [f32; 3] = [4.0e-5, 2.5e-5, 1.5e-5];
/// Hourly chance of a crime at an occupied building, before unemployment,
/// land value, night and police coverage.
pub const CRIME_RISK: f32 = 2.0e-4;
/// Hourly chance a citizen indoors falls ill, before age, hunger and
/// healthcare coverage.
pub const SICK_RISK: f32 = 1.2e-4;
/// How much full coverage cuts each risk.
pub const FIRE_COVER_CUT: f32 = 0.7;
pub const CRIME_COVER_CUT: f32 = 0.75;
pub const SICK_COVER_CUT: f32 = 0.5;
/// Fire intensity gained per tick while nobody fights it (full in ~5.5 h).
pub const FIRE_GROWTH: f32 = 0.003;
/// Intensity an engine on scene knocks down per tick.
pub const ENGINE_RATE: f32 = 0.012;
/// Condition a building loses per tick at full intensity.
pub const FIRE_DAMAGE: f32 = 0.0012;
/// At or below this condition a burning building is gone.
pub const BURNED_DOWN: f32 = 0.08;
/// [services] Condition the city restores on one of its own structures
/// (stations, plants, depots, docks...) per day while it is damaged: a
/// gutted station is back in service in about a day and a half (past the
/// derelict line) and fully repaired in under four.
pub const REPAIR_PER_DAY: f32 = 0.25;
/// [services] What repairs cost: this share of the structure's build price
/// for the whole of its condition (a gutted hospital about half its price).
pub const REPAIR_SHARE: f32 = 0.5;
/// Fires above this intensity can jump to buildings this close (lot
/// centres), checked every `SPREAD_EVERY` ticks with `SPREAD_CHANCE` x
/// intensity per neighbour.
pub const SPREAD_INTENSITY: f32 = 0.5;
pub const SPREAD_RADIUS: f32 = 28.0;
/// Lots with corners this close count as touching.
pub const SPREAD_GAP: f32 = 4.0;
pub const SPREAD_EVERY: u64 = 20;
pub const SPREAD_CHANCE: f32 = 0.04;
/// Ticks from the call to the dispatch: a dispatcher on shift, a staffed
/// centre with nobody at the desk (on call), and no dispatch centre at all
/// (the call goes to the regional line).
pub const DISPATCH_ON_SHIFT: u64 = 2;
pub const DISPATCH_ON_CALL: u64 = 8;
pub const DISPATCH_REGIONAL: u64 = 20;
/// Ticks of work on scene: a crime, a patient (fires burn until out).
pub const CRIME_WORK: f32 = 25.0;
/// [murder] Minutes the police spend on a homicide scene: the
/// investigation (the scene held, witnesses, the neighbours asked).
pub const HOMICIDE_WORK: f32 = 150.0;
/// [murder] Chance the investigation ends in an arrest: the killer known
/// (a feud), or a stranger (an offender's job gone wrong).
pub const HOMICIDE_ARREST_KNOWN: f32 = 0.8;
pub const HOMICIDE_ARREST_UNKNOWN: f32 = 0.45;
/// [murder] Days in jail for a killing (`SENTENCE_DAYS` for the rest).
pub const HOMICIDE_SENTENCE_DAYS: (u64, u64) = (12, 20);
pub const MEDICAL_WORK: f32 = 12.0;
/// [driving] Ticks the police spend at a crash scene, by the case: someone
/// hurt (statements, the scene kept while the paramedics work); a driver to
/// arrest (drunk or through a red light: a test, the paperwork); or only a
/// written-off car in the road to clear for the tow truck. A crash with
/// nobody hurt and every car still drivable is never a call: the drivers
/// swap details and go (`World::handle_crash`).
pub const CRASH_WORK: f32 = 20.0;
pub const CRASH_WORK_ARREST: f32 = 25.0;
pub const CRASH_WORK_CLEAR: f32 = 10.0;
/// A crash this close to an open crash scene joins it rather than opening
/// a call of its own.
pub const CRASH_SCENE_JOIN: f32 = 60.0;
/// [driving] Chance the police arrest a driver at fault who was drunk or
/// ran a red, once they are on scene.
pub const DUI_ARREST: f32 = 0.6;
/// Chance an arrest is made when the police get there, quick or slow (a
/// response over `QUICK_RESPONSE` ticks after the crime).
pub const ARREST_QUICK: f32 = 0.75;
pub const ARREST_SLOW: f32 = 0.4;
pub const QUICK_RESPONSE: u64 = 30;
/// An incident nobody is sent to ends on its own after this many ticks
/// from the call: crimes go unsolved, patients get better (unhappily).
pub const CRIME_GIVE_UP: u64 = 180;
pub const MEDICAL_GIVE_UP: u64 = 480;
/// Hours a patient stays in hospital, a range.
pub const STAY_HOURS: (u64, u64) = (6, 24);
/// Daily wage bonus at full school coverage of a worker's home.
pub const EDUCATION_BONUS: f32 = 0.12;
/// Stadium ticket price, chance a stadium hosts an event on a given day
/// (by tier), and when the crowd sets off.
pub const TICKET_PRICE: f32 = 12.0;
pub const EVENT_CHANCE: [f64; 3] = [0.3, 0.4, 0.6];
pub const EVENT_MINUTE: u64 = 18 * 60;
/// How far a fan travels to a game.
pub const EVENT_REACH: f32 = 1600.0;
/// Pupils set off for school at this minute and stay until it closes.
pub const SCHOOL_MINUTE: u64 = 7 * 60 + 30;
/// A hospital stay, billed to the patient's household; what it cannot pay
/// the city covers.
pub const HOSPITAL_BILL: f32 = 60.0;
/// Days in jail for an arrest, a range; without a jail with room the suspect
/// is held at the police station this many hours and let go early.
pub const SENTENCE_DAYS: (u64, u64) = (2, 5);
pub const HELD_HOURS: u64 = 6;
/// With no crewed station of the kind but a dispatcher to ask, a unit from
/// out of town comes: this many minutes after dispatch, for a fee.
pub const OUT_OF_TOWN_TRAVEL: u64 = 45;
/// [clarity] Per call, by incident kind (fire, crime, medical). An
/// ambulance from the next town is dear: a few calls cost more than a
/// clinic's upkeep for a week, so a town without one feels it.
pub const OUT_OF_TOWN_FEE: [f32; 3] = [1_500.0, 600.0, 2_400.0];
/// Police cars between calls patrol their station's district: they drive
/// to a beat (a busy or crime-hit block) and park there this many ticks, a
/// range, before moving on; ambulances post at a standby point near where
/// people live. Units on patrol or standby, and units driving home, take
/// the next call from where they are. Fire engines wait in the station.
pub const PATROL_STAY: (u64, u64) = (25, 50);
pub const STANDBY_STAY: (u64, u64) = (90, 180);
/// A beat or standby point is picked within this share of the station's
/// coverage radius (straight line).
pub const PATROL_REACH: f32 = 0.85;
/// A call queued this long with every city unit of the kind busy goes to a
/// neighbouring town as mutual aid (with a dispatcher to ask), billed like
/// out-of-town help but quicker: their units are already on the road.
pub const MUTUAL_AID_WAIT: u64 = 40;
pub const MUTUAL_AID_TRAVEL: u64 = 20;
/// A responding unit is on scene once its car is this close to its stop.
pub const PULL_UP: f32 = 25.0;
/// [fire-scene] Ticks a fire engine stays once the fire is out (or the
/// building is lost): the crew shuts the water off, rolls the hoses, lowers
/// the ladder and climbs back in before it drives home.
pub const PACK_UP_TICKS: u64 = 3;
/// [fire-scene] A fire burning this hard once the first engine is called
/// (or on scene) sounds a second alarm: a second engine with its own crew.
pub const SECOND_ALARM: f32 = 0.6;
/// [fire-scene] Where an engine pulls up: at least this far (m) from the
/// junctions at either end of the street, and this far behind the first
/// engine for a second.
pub const SCENE_JUNCTION_CLEAR: f32 = 18.0;
pub const SECOND_ENGINE_GAP: f32 = 11.0;
/// [fire-scene] An engine's half length and half width, for where it
/// stands at the kerb and what traffic steers round.
pub const ENGINE_HALF: (f32, f32) = (4.1, 1.4);
/// A kind counts as saturated while it has no unit free; the report raises
/// an alert after this many saturated minutes in a day.
pub const SATURATED_ALERT_MIN: u32 = 60;
/// Rubble stays on a burned lot this many ticks before it can be rebuilt.
pub const RUBBLE_TICKS: u64 = 2 * 1440;
/// Resolved incidents kept for the record (the Emergencies window's history).
pub const KEEP_RESOLVED: usize = 500;

/// Everything the services system owns.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Services {
    pub units: SlotMap<UnitId, Unit>,
    /// Active incidents and the last few resolved ones.
    pub incidents: Vec<Incident>,
    pub next_id: u32,
    pub admissions: Vec<Admission>,
    /// People in custody: held at a police station or serving time in a jail.
    pub custody: Vec<Custody>,
    /// Arrests per citizen, ever: the criminal record.
    pub record: Vec<(CitizenId, u32)>,
    /// Burned lots: rubble until the tick, nothing built before.
    pub rubble: Vec<(LotId, Vec2, u64)>,
    /// Stadiums hosting an event today.
    pub events_today: Vec<BuildingId>,
    pub stats: ServiceStats,
    /// Coverage per lot, per covering kind; rebuilt when roads or service
    /// buildings change.
    #[serde(skip)]
    pub coverage: HashMap<LotId, [f32; 4]>,
    #[serde(skip)]
    pub coverage_key: Option<u64>,
    /// Unit by the car it drives, rebuilt as units move.
    #[serde(skip)]
    pub by_car: HashMap<CarId, UnitId>,
    /// Queued calls no free unit could reach, and when that was found (they
    /// are retried every five minutes rather than every tick).
    #[serde(skip)]
    pub route_failed: HashMap<u32, u64>,
    /// The services' own random stream (see `World::srng`). Saved, so a
    /// loaded game rolls the same fires and crimes as one left running.
    #[serde(default, with = "crate::rng_save::opt")]
    pub rng: Option<crate::SimRng>,
    /// [emergency] Car chases under way.
    pub chases: Vec<Chase>,
}

impl Services {
    pub fn incident(&self, id: u32) -> Option<&Incident> {
        self.incidents.iter().find(|i| i.id == id)
    }

    pub fn incident_mut(&mut self, id: u32) -> Option<&mut Incident> {
        self.incidents.iter_mut().find(|i| i.id == id)
    }

    pub fn active(&self) -> impl Iterator<Item = &Incident> {
        self.incidents.iter().filter(|i| i.active())
    }

    /// [emergency] The chase `car` is in, as the fugitive or the police.
    pub fn chase_of_car(&self, car: CarId) -> Option<&Chase> {
        self.chases.iter().find(|c| c.car == car || self.units.get(c.unit).is_some_and(|u| u.car == Some(car)))
    }

    /// The unit driving `car`, if it is an emergency vehicle.
    pub fn unit_of_car(&self, car: CarId) -> Option<UnitId> {
        self.by_car.get(&car).copied()
    }

    /// Burning buildings and how hard they burn.
    pub fn fires(&self) -> impl Iterator<Item = (BuildingId, f32)> + '_ {
        self.active().filter(|i| i.kind == IncidentKind::Fire && i.intensity > 0.0).map(|i| (i.building, i.intensity))
    }

    pub fn is_burning(&self, b: BuildingId) -> bool {
        self.active().any(|i| i.kind == IncidentKind::Fire && i.building == b)
    }

    /// Arrests on a citizen's record.
    pub fn arrests_of(&self, c: CitizenId) -> u32 {
        self.record.iter().find(|r| r.0 == c).map_or(0, |r| r.1)
    }

    /// People serving time in each jail: (jail, inmates).
    pub fn jail_occupancy(&self) -> Vec<(BuildingId, u32)> {
        let mut out: Vec<(BuildingId, u32)> = Vec::new();
        for c in self.custody.iter().filter(|c| c.jailed) {
            match out.iter_mut().find(|o| o.0 == c.at) {
                Some(o) => o.1 += 1,
                None => out.push((c.at, 1)),
            }
        }
        out
    }

    pub fn in_custody(&self, c: CitizenId) -> bool {
        self.custody.iter().any(|x| x.citizen == c)
    }

    pub fn is_rubble(&self, lot: LotId) -> bool {
        self.rubble.iter().any(|r| r.0 == lot)
    }

    /// Coverage of a lot by one covering kind, `0..1`.
    pub fn coverage_of(&self, lot: LotId, kind: ServiceKind) -> f32 {
        match kind.coverage_index() {
            Some(k) => self.coverage.get(&lot).map_or(0.0, |c| c[k]),
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests;
