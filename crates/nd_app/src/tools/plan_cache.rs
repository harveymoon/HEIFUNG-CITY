//! [`EffectivePlan`]: the master plan with the project applied, cached.

use bevy::prelude::*;
use nd_sim::{Plan, World};

/// The master plan with the project applied, cached between changes.
/// `World::effective_plan()` clones every gesture, and the tools, the gizmos
/// and the Edit panel all want it each frame; `plan_version` bumps on every
/// change to either, so comparing it is enough to know when to clone again.
#[derive(Resource, Default)]
pub struct EffectivePlan {
    plan: Plan,
    key: Option<u64>,
}

impl EffectivePlan {
    /// Bring the cache up to date with `world` (a clone only when the plan changed).
    pub fn sync(&mut self, world: &World) -> &Plan {
        if self.key != Some(world.plan_version) {
            self.plan = world.effective_plan().clone();
            self.key = Some(world.plan_version);
        }
        &self.plan
    }

    /// Forget the cache: a loaded world starts its own `plan_version` count.
    pub fn invalidate(&mut self) {
        self.key = None;
    }
}
