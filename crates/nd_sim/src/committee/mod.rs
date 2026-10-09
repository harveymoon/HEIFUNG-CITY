//! [committee] The mayor's AI committee (docs/AI_COMMITTEE.md): a pool of
//! fictional politicians, six seats the mayor fills and empties at will,
//! alerts between meetings, a meeting every 30 sim days with a fixed agenda,
//! votes, and passed proposals turned into named projects for the mayor to
//! implement.
//!
//! This module is the deterministic half and holds no model: the pool (from
//! the game's name generator and a hash of the world seed), the compact city
//! report the members argue over (`report`), the alert rules (`rules`), the
//! proposal actions with their validation, the rule-based votes, the
//! candidate sites and the implementation (`proposals`). What the members
//! *say* comes from a text backend in `nd_minds::committee` (a template
//! stand-in, a local model or the Claude API).
//!
//! The sim never reads any of this. The committee's state is saved with the
//! world (`World::committee`, `serde(default)`, so old saves load with an
//! empty one), and its only way back into the city is a project the mayor
//! implements by hand (`World::committee_implement`), recorded in the
//! project's status.

pub mod proposals;
pub mod report;
pub mod rules;

mod pool;
mod types;

pub use pool::*;
pub use types::*;

#[cfg(test)]
mod tests;

use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

pub use proposals::{Action, SiteOption};
pub use report::{Area, CityReport, Fact};

use crate::{SimRng, World};

/// Seats at the table (the mayor is not one of them: the mayor has a vote
/// of their own and, optionally, a veto).
pub const SEATS: usize = 6;
/// Candidates in the pool.
pub const POOL_SIZE: usize = 14;
/// Sim days between regular meetings.
pub const MEETING_INTERVAL_DAYS: u32 = 30;
/// Memory entries each politician keeps (oldest dropped).
pub const MEMORY_KEPT: usize = 12;
/// Alerts kept in the log.
pub const ALERTS_KEPT: usize = 80;
/// Past meetings' minutes kept in full.
pub const MINUTES_KEPT: usize = 6;
/// Proposals tabled per meeting at most (a turn limit: keeps cost and
/// length predictable).
pub const MAX_PROPOSALS: usize = 3;
/// Replies to the status reports per meeting at most.
pub const MAX_RESPONSES: usize = 2;

// ── the committee ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Committee {
    /// Filled once, from the world seed (`ensure`).
    pub pool: Vec<Politician>,
    /// Politician ids by seat.
    pub seats: Vec<Option<u32>>,
    pub alerts: Vec<Alert>,
    /// Day each alert rule last fired, by rule key.
    pub rule_last: std::collections::BTreeMap<String, u32>,
    /// Daily samples for the week-on-week rules (newest last).
    pub samples: Vec<Sample>,
    /// The day the next regular meeting is due.
    pub next_meeting_day: u32,
    pub meetings_held: u32,
    /// The meeting in session, if any.
    pub meeting: Option<Meeting>,
    /// The last meetings' minutes (newest last).
    pub minutes: Vec<Meeting>,
    pub projects: Vec<CommitteeProject>,
    pub next_id: u32,
    /// Meetings come by the calendar (off: only when the mayor calls one).
    pub auto_meetings: bool,
    /// The mayor may veto.
    pub veto_allowed: bool,
    /// The report the last meeting read, for "since the last meeting".
    pub baseline: Option<CityReport>,
}

impl Default for Committee {
    fn default() -> Self {
        Self {
            pool: Vec::new(),
            seats: vec![None; SEATS],
            alerts: Vec::new(),
            rule_last: Default::default(),
            samples: Vec::new(),
            next_meeting_day: 0,
            meetings_held: 0,
            meeting: None,
            minutes: Vec::new(),
            projects: Vec::new(),
            next_id: 1,
            auto_meetings: true,
            veto_allowed: true,
            baseline: None,
        }
    }
}

impl Committee {
    pub fn is_set_up(&self) -> bool {
        !self.pool.is_empty()
    }

    /// Fill the pool from `seed` (once) and seat the outgoing mayor's
    /// committee, one member for each of the six main scopes; schedule the
    /// first meeting. Deterministic in the seed; draws nothing from the
    /// world's RNG.
    pub fn ensure(&mut self, seed: u64, today: u32) {
        if self.seats.len() != SEATS {
            self.seats.resize(SEATS, None);
        }
        if self.next_id == 0 {
            self.next_id = 1;
        }
        if !self.pool.is_empty() {
            return;
        }
        self.pool = generate_pool(seed);
        self.next_id = self.next_id.max(self.pool.iter().map(|p| p.id + 1).max().unwrap_or(1));
        let first = [Scope::Transit, Scope::Housing, Scope::Finance, Scope::Safety, Scope::Environment, Scope::Business];
        for (seat, scope) in first.into_iter().enumerate() {
            if let Some(id) = self.pool.iter().find(|p| p.scope == scope).map(|p| p.id) {
                self.seat(seat, id, today);
            }
        }
        self.next_meeting_day = today + MEETING_INTERVAL_DAYS;
    }

    pub fn fresh_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn member(&self, id: u32) -> Option<&Politician> {
        self.pool.iter().find(|p| p.id == id)
    }

    pub fn member_mut(&mut self, id: u32) -> Option<&mut Politician> {
        self.pool.iter_mut().find(|p| p.id == id)
    }

    pub fn name_of(&self, id: u32) -> String {
        self.member(id).map_or_else(|| format!("member {id}"), |p| p.name.clone())
    }

    /// Seated members in seat order.
    pub fn seated(&self) -> Vec<&Politician> {
        self.seats.iter().flatten().filter_map(|&id| self.member(id)).collect()
    }

    pub fn seated_ids(&self) -> Vec<u32> {
        self.seats.iter().flatten().copied().filter(|&id| self.member(id).is_some()).collect()
    }

    pub fn seat_of(&self, id: u32) -> Option<usize> {
        self.seats.iter().position(|s| *s == Some(id))
    }

    /// Candidates not seated.
    pub fn candidates(&self) -> Vec<&Politician> {
        self.pool.iter().filter(|p| self.seat_of(p.id).is_none()).collect()
    }

    /// The first seated member who answers for `scope`.
    pub fn watcher(&self, scope: Scope) -> Option<u32> {
        self.seated().into_iter().find(|p| p.scope == scope).map(|p| p.id)
    }

    fn seat(&mut self, seat: usize, id: u32, today: u32) {
        self.seats[seat] = Some(id);
        if let Some(p) = self.member_mut(id) {
            p.record.seated_since = Some(today);
            p.record.times_appointed += 1;
        }
    }

    /// Appoint a candidate: to `seat` if given (whoever sat there is
    /// dismissed), else to the first empty seat.
    pub fn appoint(&mut self, id: u32, seat: Option<usize>, today: u32) -> Result<usize, String> {
        if self.member(id).is_none() {
            return Err(format!("no politician {id} in the pool"));
        }
        if let Some(s) = self.seat_of(id) {
            return Err(format!("{} already holds seat {}", self.name_of(id), s + 1));
        }
        let seat = match seat {
            Some(s) if s < SEATS => s,
            Some(s) => return Err(format!("there are {SEATS} seats; seat {} does not exist", s + 1)),
            None => self.seats.iter().position(Option::is_none).ok_or("every seat is taken: dismiss someone first, or name the seat to replace them")?,
        };
        if let Some(old) = self.seats[seat] {
            self.dismiss(old, today)?;
        }
        self.seat(seat, id, today);
        Ok(seat)
    }

    /// Dismiss a seated member; their seat stays empty until filled.
    pub fn dismiss(&mut self, id: u32, _today: u32) -> Result<usize, String> {
        let seat = self.seat_of(id).ok_or_else(|| format!("{} holds no seat", self.name_of(id)))?;
        self.seats[seat] = None;
        if let Some(p) = self.member_mut(id) {
            p.record.seated_since = None;
            p.record.times_dismissed += 1;
        }
        Ok(seat)
    }

    pub fn push_alert(&mut self, mut a: Alert) -> u32 {
        a.id = self.fresh_id();
        let id = a.id;
        if let Some(p) = self.member_mut(a.member) {
            p.record.alerts += 1;
        }
        self.alerts.push(a);
        if self.alerts.len() > ALERTS_KEPT {
            let extra = self.alerts.len() - ALERTS_KEPT;
            self.alerts.drain(..extra);
        }
        id
    }

    pub fn alert_mut(&mut self, id: u32) -> Option<&mut Alert> {
        self.alerts.iter_mut().find(|a| a.id == id)
    }

    pub fn unseen_alerts(&self) -> usize {
        self.alerts.iter().filter(|a| !a.seen).count()
    }

    pub fn project(&self, id: u32) -> Option<&CommitteeProject> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn project_mut(&mut self, id: u32) -> Option<&mut CommitteeProject> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Is a regular meeting due today?
    pub fn meeting_due(&self, today: u32) -> bool {
        self.auto_meetings && self.meeting.is_none() && self.is_set_up() && today >= self.next_meeting_day && !self.seated_ids().is_empty()
    }

    /// File a finished meeting: minutes kept, the next one scheduled.
    pub fn file_meeting(&mut self) {
        if let Some(m) = self.meeting.take() {
            self.baseline = Some(m.report.clone());
            self.minutes.push(m);
            if self.minutes.len() > MINUTES_KEPT {
                let extra = self.minutes.len() - MINUTES_KEPT;
                self.minutes.drain(..extra);
            }
        }
    }

    /// A daily sample for the week-on-week rules.
    pub fn push_sample(&mut self, s: Sample) {
        if self.samples.last().is_some_and(|l| l.day == s.day) {
            self.samples.pop();
        }
        self.samples.push(s);
        if self.samples.len() > 30 {
            let extra = self.samples.len() - 30;
            self.samples.drain(..extra);
        }
    }

    /// The sample `days` before `today`, or the oldest kept if fewer.
    pub fn sample_before(&self, today: u32, days: u32) -> Option<&Sample> {
        self.samples.iter().rev().find(|s| s.day + days <= today).or_else(|| self.samples.first().filter(|s| s.day < today))
    }
}

/// A stable 64-bit mix (splitmix64), for seeds from seeds.
pub use crate::hash::mix;

// ── the world's side ────────────────────────────────────────────────────────

impl World {
    /// Today, as the committee counts days (0-based like the clock).
    pub fn committee_today(&self) -> u32 {
        self.clock.day() as u32
    }

    /// Set the committee up if it is not (a new game, or a save from
    /// before the committee).
    pub fn committee_ensure(&mut self) {
        let today = self.committee_today();
        // The game id makes each new game's pool its own; a hash, not the
        // world's RNG, so the sim is untouched.
        let id = crate::hash::fnv_bytes(self.identity.game_id.bytes());
        let seed = mix(mix(self.config.seed, self.landscape.params.seed), id);
        self.committee.ensure(seed, today);
    }

    /// The day's committee work outside meetings: a sample for the rules
    /// and the alerts they raise. Returns the new alerts' ids. Reads the
    /// world, writes only the committee.
    pub fn committee_daily(&mut self) -> Vec<u32> {
        self.committee_ensure();
        let report = CityReport::build(self);
        let today = self.committee_today();
        self.committee.push_sample(Sample { day: today, rent_burden: report.rent_burden, contaminated_cells: report.contaminated_cells, riders: report.riders_per_day, crime: report.crime_per_day });
        rules::raise_alerts(&mut self.committee, &report, today)
    }
}

#[cfg(test)]
pub(crate) fn test_seed() -> u64 {
    42
}

