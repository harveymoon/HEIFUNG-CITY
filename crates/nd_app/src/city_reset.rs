//! [fix/review-app] One reset for the app state that names the old city's
//! ids, run whenever a different city starts (a load, a revert, a new game,
//! a generated city), whoever started it: the title screen, the Saves and
//! History panels, the New Game window or the remote API.
//!
//! It is keyed on `Sim::generation`, which every city replacement bumps
//! (`Sim::city_replaced`), so a new way of replacing the city cannot forget
//! it. Before this, four hand-written copies of the reset had drifted: the
//! UI paths left the camera following an old id, and every path threw the
//! tool preferences away with the ids.
//!
//! The renderers have their own signal for the same moment,
//! `Sim::full_rebuild` (see `render::reconcile` for the id-keyed movers).

use bevy::prelude::*;

use crate::camera::{CameraFly, CameraFollow};
use crate::render::{Lighting, Overlays};
use crate::sim::{Sim, SimSet};
use crate::tools::{EffectivePlan, Selection, ToolState};

/// The `Sim::generation` the app state below was last reset for.
#[derive(Resource, Default)]
pub struct CityEpoch(pub u64);

pub struct CityResetPlugin;

impl Plugin for CityResetPlugin {
    fn build(&self, app: &mut App) {
        // In Input, before the tools read the selection and before the
        // renderers (Sync) draw it; the remote API's commands run before it
        // and catch up themselves (`catch_up`) between commands.
        app.init_resource::<CityEpoch>().add_systems(Update, on_city_replaced.in_set(SimSet::Input).before(crate::tools::tool_input));
    }
}

/// Reset the city-scoped state once per new `Sim::generation`.
#[allow(clippy::too_many_arguments)]
pub fn on_city_replaced(sim: Res<Sim>, mut epoch: ResMut<CityEpoch>, mut tools: ResMut<ToolState>, mut selection: ResMut<Selection>, mut eplan: ResMut<EffectivePlan>, mut follow: ResMut<CameraFollow>, mut fly: ResMut<CameraFly>, mut overlays: ResMut<Overlays>, mut lighting: ResMut<Lighting>) {
    // Checked before any `ResMut` is touched, so a normal frame marks
    // nothing changed.
    if epoch.0 == sim.generation {
        return;
    }
    catch_up(sim.generation, &mut epoch, &mut tools, &mut selection, &mut eplan, &mut follow, &mut fly, &mut overlays, &mut lighting);
}

/// The reset itself, for callers that need it before the next frame (the
/// remote API runs a batch of commands in one go). Does nothing when the
/// state already belongs to `generation`.
///
/// [clean-start] A new or loaded city also starts on the plain map in
/// Inspect: no map layer on (the "other projects" switch kept), no heat
/// vision, the Inspect tool (its panels closed with it); the tool
/// preferences stay.
#[allow(clippy::too_many_arguments)]
pub fn catch_up(generation: u64, epoch: &mut CityEpoch, tools: &mut ToolState, selection: &mut Selection, eplan: &mut EffectivePlan, follow: &mut CameraFollow, fly: &mut CameraFly, overlays: &mut Overlays, lighting: &mut Lighting) {
    if epoch.0 == generation {
        return;
    }
    epoch.0 = generation;
    tools.forget_city();
    tools.tool = crate::tools::Tool::Inspect;
    overlays.forget_city();
    lighting.heat = false;
    *selection = Selection::None;
    eplan.invalidate();
    follow.stop();
    fly.flight = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::roads::RoadKind;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .configure_sets(Update, (SimSet::Input, SimSet::Step, SimSet::Sync).chain())
            .insert_resource(Sim::new())
            .init_resource::<ToolState>()
            .init_resource::<Selection>()
            .init_resource::<EffectivePlan>()
            .init_resource::<CameraFollow>()
            .init_resource::<CameraFly>()
            .init_resource::<Overlays>()
            .init_resource::<Lighting>()
            .init_resource::<CityEpoch>()
            .add_systems(Update, on_city_replaced.in_set(SimSet::Input));
        app
    }

    /// A new city clears what names the old one's ids (the selection, the
    /// route being edited, the follow) and keeps the tool preferences.
    #[test]
    fn a_new_city_forgets_old_ids_and_keeps_preferences() {
        let mut app = app();
        app.update();
        let building = {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            sim.seed_demo_district();
            sim.world.implement();
            sim.world.buildings.keys().next()
        };
        {
            let w = app.world_mut();
            let mut tools = w.resource_mut::<ToolState>();
            tools.kind = RoadKind::Path;
            tools.lanes_fwd = 3;
            tools.status = "kept".into();
            tools.selected_gesture = Some(nd_sim::GestureId(7));
            if let Some(b) = building {
                *w.resource_mut::<Selection>() = Selection::Building(b);
            }
        }
        // A normal frame resets nothing.
        app.update();
        assert_eq!(app.world().resource::<ToolState>().selected_gesture, Some(nd_sim::GestureId(7)));
        // The city is replaced (as Load or New Game does).
        app.world_mut().resource_mut::<Sim>().new_game(nd_sim::LandscapeParams::flat());
        app.update();
        let tools = app.world().resource::<ToolState>();
        assert_eq!(tools.selected_gesture, None, "old ids go");
        assert_eq!((tools.kind, tools.lanes_fwd), (RoadKind::Path, 3), "preferences stay");
        assert_eq!(tools.status, "kept");
        assert!(*app.world().resource::<Selection>() == Selection::None);
        assert_eq!(app.world().resource::<CityEpoch>().0, app.world().resource::<Sim>().generation);
    }

    /// [clean-start] A loaded or new city starts on the plain map in
    /// Inspect, whatever layer, tool and view the old one was left in.
    #[test]
    fn a_new_city_starts_plain_in_inspect() {
        let mut app = app();
        app.update();
        {
            let w = app.world_mut();
            w.resource_mut::<ToolState>().tool = crate::tools::Tool::Road;
            let mut o = w.resource_mut::<Overlays>();
            o.traffic = true;
            o.zoning = true;
            o.land = Some(nd_sim::survey::LandLayer::ALL[0]);
            o.projects = false;
            w.resource_mut::<Lighting>().heat = true;
        }
        app.world_mut().resource_mut::<Sim>().new_game(nd_sim::LandscapeParams::flat());
        app.update();
        let w = app.world();
        assert_eq!(w.resource::<ToolState>().tool, crate::tools::Tool::Inspect);
        let o = w.resource::<Overlays>();
        assert!(!o.traffic && !o.zoning && o.land.is_none(), "no layer on");
        assert!(crate::map_layers::MAP_LAYERS.iter().all(|l| !o.get(l.key).unwrap_or(false)), "no layer on");
        assert!(!o.projects, "the other-projects switch is a preference");
        assert!(!w.resource::<Lighting>().heat);
        // The very first city starts plain too.
        let fresh = Overlays::default();
        assert!(crate::map_layers::MAP_LAYERS.iter().all(|l| !fresh.get(l.key).unwrap_or(false)));
    }
}
