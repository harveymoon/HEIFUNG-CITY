//! [clarity] How a citizen outdoors holds themselves, for the renderer:
//! standing (walking), lying on the ground (hurt in a crash, waiting for
//! the paramedics), or sitting on the kerb (some of the homeless, by their
//! camp or out loitering). Lying down is also how the homeless sleep
//! rough at night.

use super::*;
use crate::services::{IncidentKind, Stage};

/// A figure's pose outdoors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pose {
    Standing,
    /// Flat on the ground.
    Lying,
    /// Sat on the kerb or the pavement.
    SittingKerb,
    /// On a park bench (`Activity::Sitting`).
    Bench,
}

impl World {
    /// Citizens lying hurt on the street until the medics take them:
    /// patients of an open road medical call (a crash, an assault outdoors)
    /// not yet in the ambulance.
    pub fn down_on_street(&self) -> std::collections::HashSet<CitizenId> {
        self.services
            .active()
            .filter(|i| i.kind == IncidentKind::Medical && i.on_road && matches!(i.stage, Stage::Unreported | Stage::Reported | Stage::Dispatched | Stage::OnScene))
            .filter_map(|i| i.citizen)
            .collect()
    }

    /// The pose of one citizen (see `pose_with` to reuse `down_on_street`
    /// across a crowd).
    pub fn pose_of(&self, id: CitizenId) -> Pose {
        self.pose_with(id, &self.down_on_street())
    }

    /// The pose of one citizen, given who is down on the street.
    pub fn pose_with(&self, id: CitizenId, down: &std::collections::HashSet<CitizenId>) -> Pose {
        let Some(c) = self.citizens.get(id) else { return Pose::Standing };
        if c.inside.is_some() {
            return Pose::Standing;
        }
        if down.contains(&id) && !c.activity.is_traveling() {
            return Pose::Lying;
        }
        match c.activity {
            Activity::Sitting { .. } => Pose::Bench,
            // [walk-crossings] Never lying or sitting in a street's lanes.
            Activity::Idle if self.is_homeless(id) => match homeless_pose(c.seed, self.clock.tick) {
                Pose::Standing => Pose::Standing,
                _ if self.in_carriageway(c.pos) => Pose::Standing,
                p => p,
            },
            _ => Pose::Standing,
        }
    }
}

/// A homeless person standing around: at night most lie down to sleep;
/// by day some sit on the kerb and a few lie down, changing every hour or
/// so (from the seed, so it does not flicker frame to frame).
pub fn homeless_pose(seed: u64, tick: u64) -> Pose {
    let hour = (tick / 60) % 24;
    let night = hour >= crate::homeless::NIGHT_HOUR || hour < crate::homeless::WAKE_HOUR;
    let slot = tick / 70;
    let roll = (seed ^ slot.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9) >> 58; // 0..64
    match (night, roll) {
        (true, 0..=47) => Pose::Lying,
        (true, _) => Pose::SittingKerb,
        (false, 0..=7) => Pose::Lying,
        (false, 8..=27) => Pose::SittingKerb,
        _ => Pose::Standing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homeless_people_sometimes_lie_down_or_sit_on_the_kerb() {
        let poses: Vec<Pose> = (0..400u64).map(|s| homeless_pose(s.wrapping_mul(0x2545_F491_4F6C_DD1D), 12 * 60)).collect();
        let n = |p: Pose| poses.iter().filter(|&&q| q == p).count();
        assert!(n(Pose::Lying) > 10 && n(Pose::SittingKerb) > 60 && n(Pose::Standing) > 150, "by day: {} lying, {} sitting, {} standing", n(Pose::Lying), n(Pose::SittingKerb), n(Pose::Standing));
        let night: Vec<Pose> = (0..400u64).map(|s| homeless_pose(s.wrapping_mul(0x2545_F491_4F6C_DD1D), 2 * 60)).collect();
        assert!(night.iter().all(|&p| p != Pose::Standing), "nobody stands about at night");
        assert!(night.iter().filter(|&&p| p == Pose::Lying).count() > 250, "most sleep");
    }
}
