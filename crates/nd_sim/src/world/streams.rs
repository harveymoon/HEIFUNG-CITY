//! [review] The registry of the world's salted random streams: every
//! system that draws from a stream of its own (rather than the world's
//! `rng`) seeds it with `World::stream_seed(Stream::..)`, the world seed
//! xor the stream's salt, so the salts sit side by side and two systems
//! cannot share one by accident (as `erng(4)` once did; the test below
//! keeps them distinct). The salts are the ones each system used before
//! the registry, so every stream draws exactly what it drew.
//!
//! Not here: the world's own stream (`World::rng`, saved), draws keyed by
//! a citizen's or building's seed (`rng_for(c.seed ..)`), and the hash
//! mixers (`mix`, `unit`).
//!
//! Moving the world stream's draws onto keyed streams (review §5.2) would
//! change outcomes, so it is a plan, not done: see the review doc.

use super::*;

/// A salted stream. Each one names the system and what it draws for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stream {
    /// `World::srng`: fires, crime, illness, dispatch (cached, saved).
    Services,
    /// `World::society_rng`: trash, camps, offenders (cached, saved).
    Society,
    /// `World::arng`: drones, air taxis, air ambulances (cached, saved).
    Air,
    /// `World::erng`: the estate's daily decisions (fresh per tick).
    Estate,
    /// `World::trng`: taxi companies (fresh per call).
    Taxis,
    /// Crash outcomes (`handle_crash`, keyed by the report).
    Crash,
    /// Street takeovers: whether one starts today.
    TakeoverDay,
    /// Street takeovers: where one starts.
    TakeoverStart,
    /// Street takeovers: outsiders drawn in.
    TakeoverOutsiders,
    /// Street takeovers: the minute-by-minute run.
    TakeoverRun,
    /// Street takeovers: how one ends.
    TakeoverEnd,
    /// Motorcycle packs: where one starts.
    MotoStart,
    /// Motorcycle packs: the club's riders.
    MotoClub,
    /// Street races: a police bust.
    RaceBust,
    /// Districts: the business a district's look picks.
    DistrictBusiness,
    /// Ferry day trips (`ferry::SALT`).
    Ferry,
    /// Tourists (`tourism::SALT`).
    Tourism,
}

impl Stream {
    #[cfg(test)]
    pub(super) const ALL: [Stream; 17] = [
        Stream::Services,
        Stream::Society,
        Stream::Air,
        Stream::Estate,
        Stream::Taxis,
        Stream::Crash,
        Stream::TakeoverDay,
        Stream::TakeoverStart,
        Stream::TakeoverOutsiders,
        Stream::TakeoverRun,
        Stream::TakeoverEnd,
        Stream::MotoStart,
        Stream::MotoClub,
        Stream::RaceBust,
        Stream::DistrictBusiness,
        Stream::Ferry,
        Stream::Tourism,
    ];

    /// The salt, xored into the world seed.
    pub(super) const fn salt(self) -> u64 {
        match self {
            Stream::Services => 0x5E7_1CE5,
            Stream::Society => 0x50C1_E7A1,
            Stream::Air => 0xA1_2D_0E,
            Stream::Estate => 0x5E57_A7E0_1A4D,
            Stream::Taxis => 0x7A_C1_CA_B5,
            Stream::Crash => 0xC2A5_4ED1,
            Stream::TakeoverDay => 0x7A4E_0FE5,
            Stream::TakeoverStart => 0x7A4E_0FE6,
            Stream::TakeoverOutsiders => 0x0575_1DE5,
            Stream::TakeoverRun => 0x7A4E_0FE7,
            Stream::TakeoverEnd => 0x7A4E_0FE8,
            Stream::MotoStart => 0x070_70C5,
            Stream::MotoClub => 0x070_7C1B,
            Stream::RaceBust => 0x5ACE_B057,
            Stream::DistrictBusiness => 0xD157,
            Stream::Ferry => 0xFE44_7001,
            Stream::Tourism => 0x7015_7000,
        }
    }
}

impl World {
    /// The seed a salted stream starts from: the world seed xor its salt.
    pub(super) fn stream_seed(&self, s: Stream) -> u64 {
        self.config.seed ^ s.salt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stream_has_its_own_salt() {
        for (i, a) in Stream::ALL.iter().enumerate() {
            for b in &Stream::ALL[i + 1..] {
                assert_ne!(a.salt(), b.salt(), "{a:?} and {b:?} share a salt");
            }
        }
    }
}
