//! [social] People talking to each other: ties between citizens, the short
//! conversations that move them, and the crews and clubs that grow out of
//! friendships and make things in the city.
//!
//! This file holds the data and the pure rules (compatibility, labels,
//! interests, how a conversation goes); `world/social.rs` runs them: who
//! meets whom where, what friends and enemies then do, and what groups make.
//!
//! - A [`Tie`] is one side of a relationship: affinity (`-1..1`, liking),
//!   familiarity (`0..1`, how well they know each other), trust (`0..1`),
//!   when and how they met and the last contact. The label
//!   ([`Bond`]: stranger, acquaintance, friend, close friend, partner,
//!   rival, enemy) is derived. Each citizen keeps at most [`MAX_TIES`];
//!   the weakest goes first. Ties fade without contact.
//! - [`compatibility`] is fixed per pair: personalities, tastes, age, class,
//!   shared interests, plus a little luck from a hash of the pair.
//! - A [`Talk`] is one conversation: a [`Topic`] drawn from their lives and
//!   an [`Outcome`] that moves both ties.
//! - A [`Group`] (band, sports club, book club, street-art crew, startup,
//!   community garden, neighbourhood association) has members, momentum and
//!   a reputation; at thresholds it makes something ([`Work`]).
//!
//! Nothing here draws from the world's random stream: every roll is a hash
//! of the pair, the tick and a salt (`pedestrians::hash01`), so the same
//! seed makes the same friends.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::CitizenId;
use crate::concerns::Concern;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub use crate::pedestrians::hash01;

mod groups;
mod talk;
mod ties;

pub use groups::*;
pub use talk::*;
pub use ties::*;

/// Ties a citizen keeps at most; past this the weakest is dropped.
pub const MAX_TIES: usize = 16;
/// Conversations each citizen remembers (newest last).
pub const MAX_TALK_NOTES: usize = 6;
/// Conversations kept city-wide for the speech bubbles and the API.
pub const RECENT_TALKS: usize = 400;
/// News lines kept.
pub const NEWS_KEEP: usize = 60;
/// Days of social history kept.
pub const HISTORY_DAYS: usize = 60;
/// Members a group takes at most.
pub const MAX_MEMBERS: usize = 10;
/// Groups a citizen belongs to at most.
pub const MAX_GROUPS_EACH: usize = 2;
/// Minutes between two conversations of the same pair.
pub const PAIR_COOLDOWN: u64 = 45;

/// Affinity and familiarity thresholds of the labels.
pub const FRIEND_AFFINITY: f32 = 0.3;
pub const FRIEND_FAMILIARITY: f32 = 0.25;
pub const CLOSE_AFFINITY: f32 = 0.6;
pub const CLOSE_FAMILIARITY: f32 = 0.5;
pub const RIVAL_AFFINITY: f32 = -0.25;
pub const ENEMY_AFFINITY: f32 = -0.55;

/// Ids in the saved tuples as their 64-bit form (`KeyData::as_ffi`, stable
/// across saves): a pretty-printed id struct took more room than the rest.
fn to_ffi<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

fn from_ffi<K: slotmap::Key>(v: u64) -> K {
    slotmap::KeyData::from_ffi(v).into()
}

/// A line of social news.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct News {
    pub seq: u32,
    pub tick: u64,
    pub text: String,
    pub pos: Vec2,
    pub group: Option<u32>,
}

/// The day's counts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SocialDay {
    pub day: u64,
    pub talks: u32,
    pub good_talks: u32,
    pub bad_talks: u32,
    pub friendships: u32,
    pub close_friendships: u32,
    pub couples: u32,
    pub feuds: u32,
    pub fights: u32,
    pub meetups: u32,
    pub home_visits: u32,
    pub favours: u32,
    pub loans: u32,
    pub jobs_found: u32,
    pub homes_found: u32,
    pub moved_in: u32,
    pub avoided: u32,
    pub gossip: u32,
    pub groups_formed: u32,
    pub groups_folded: u32,
    pub splits: u32,
    pub works: u32,
}

/// Everything social, saved with the world.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Social {
    pub circles: BTreeMap<CitizenId, Circle>,
    pub groups: Vec<Group>,
    pub next_group: u32,
    pub murals: Vec<Mural>,
    pub gardens: Vec<Garden>,
    pub gigs: Vec<Gig>,
    pub petitions: Vec<Petition>,
    pub next_petition: u32,
    pub outings: Vec<Outing>,
    /// Two friends walking together: each keeps the slower pace until then.
    pub walking_with: BTreeMap<CitizenId, (CitizenId, u64)>,
    pub news: Vec<News>,
    pub news_seq: u32,
    pub today: SocialDay,
    pub history: VecDeque<SocialDay>,
    pub talks_total: u64,
    pub friendships_total: u32,
    pub feuds_total: u32,
    pub fights_total: u32,
    pub couples_total: u32,
    /// Recent conversations, for the bubbles (not saved).
    #[serde(skip)]
    pub recent: VecDeque<Talk>,
    #[serde(skip)]
    pub serial: u64,
    /// Place choices an enemy steered today (counted by `social_pull`).
    #[serde(skip)]
    pub avoid_hits: Tally,
}

/// A counter bumped from `&self` (place choice reads the world shared).
#[derive(Debug, Default)]
pub struct Tally(std::sync::atomic::AtomicU32);

impl Clone for Tally {
    fn clone(&self) -> Self {
        Tally(std::sync::atomic::AtomicU32::new(self.get()))
    }
}

impl Tally {
    pub fn bump(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get(&self) -> u32 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn take(&self) -> u32 {
        self.0.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Social {
    pub fn circle(&self, c: CitizenId) -> Option<&Circle> {
        self.circles.get(&c)
    }

    pub fn bond(&self, a: CitizenId, b: CitizenId) -> Bond {
        self.circles.get(&a).map_or(Bond::Stranger, |c| c.bond(b))
    }

    pub fn group(&self, id: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: u32) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn active_groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|g| g.active())
    }

    pub fn gig_at(&self, venue: BuildingId, tick: u64) -> Option<&Gig> {
        self.gigs.iter().find(|g| g.venue == venue && g.on_at(tick))
    }

    pub fn push_news(&mut self, tick: u64, text: String, pos: Vec2, group: Option<u32>) {
        self.news_seq += 1;
        self.news.push(News { seq: self.news_seq, tick, text, pos, group });
        if self.news.len() > NEWS_KEEP {
            self.news.remove(0);
        }
    }

    pub fn push_talk(&mut self, mut t: Talk) {
        self.serial += 1;
        t.serial = self.serial;
        self.recent.push_back(t);
        while self.recent.len() > RECENT_TALKS {
            self.recent.pop_front();
        }
    }

    /// Ties of each label city-wide (each pair counted from both sides).
    pub fn bond_counts(&self) -> BTreeMap<Bond, u32> {
        let mut out = BTreeMap::new();
        for c in self.circles.values() {
            for t in &c.ties {
                *out.entry(t.bond()).or_insert(0) += 1;
            }
        }
        out
    }

    /// Close a day: push it on the history and start a new one.
    pub fn roll_day(&mut self, day: u64) {
        let mut d = std::mem::take(&mut self.today);
        d.day = day.saturating_sub(1);
        self.history.push_back(d);
        while self.history.len() > HISTORY_DAYS {
            self.history.pop_front();
        }
        self.today.day = day;
    }

    pub fn yesterday(&self) -> Option<&SocialDay> {
        self.history.back()
    }
}

#[cfg(test)]
mod tests;
