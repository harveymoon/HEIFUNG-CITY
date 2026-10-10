//! [overlay] What the player has selected: the [`Selection`] resource,
//! the check after every sim step that it still exists, and the one way a
//! pick becomes the selection (a click with the Inspect tool and the remote
//! API's `select` alike). Drawn by `overlay::selection`.

use bevy::prelude::*;
use nd_sim::{BuildingId, CarId, CitizenId};

use crate::sim::Sim;
use crate::tools::{Tool, ToolState};

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Citizen(CitizenId),
    Building(BuildingId),
    /// A vehicle on the road: a car, a truck or a bus (a car's driver is a
    /// click away in the inspector).
    Car(CarId),
    /// A private car standing parked (`world.vehicles`).
    Parked(nd_sim::VehicleId),
    /// [toll-inspect] A toll booth or plaza (`world.tolls`, by its id).
    Toll(u32),
    /// [camera] A pavement delivery robot (`world.bots`).
    Bot(nd_sim::BotId),
    /// [train-inspect] A train on the railway (`world.rail.trains`).
    Train(nd_sim::TrainId),
    /// [train-inspect] A metro train in its tunnel (`world.subway.trains`).
    Metro(nd_sim::MetroTrainId),
    /// [road-inspect] A street piece (kept on its piece through rebuilds by
    /// `road_select::keep_on_rebuild`).
    Road(nd_sim::SegmentId),
    /// [fire-scene] An emergency unit: a fire engine, police car or
    /// ambulance, on the road or standing on scene (`fire_scene`).
    Unit(nd_sim::UnitId),
    /// [ferry-routes] A ferry or freighter on the water (`world.ports.ships`).
    Ship(nd_sim::ports::ShipId),
    /// [feat/pt-ux] A drone taxi or a helicopter (`world.air.drones`).
    Drone(nd_sim::air::DroneId),
    /// [feat/pt-ux] A camper or van at the kerb that a household lives in
    /// (or left behind): `world.society.homeless.rvs`, by its seed.
    Rv(u64),
}

/// [fix/review-app] What the selection should be now that the sim has
/// moved: the same, `None` when the thing is gone, or the station for an
/// emergency unit back home. One exhaustive match, so a new kind of
/// selection has to say how it can vanish.
pub fn validated(sel: Selection, sim: &Sim) -> Selection {
    let w = &sim.world;
    let alive = match sel {
        Selection::None => true,
        Selection::Citizen(id) => w.citizens.contains_key(id),
        Selection::Building(id) => w.buildings.contains_key(id),
        Selection::Car(id) => w.traffic.cars.contains_key(id),
        Selection::Parked(id) => w.vehicles.cars.get(id).is_some_and(|c| c.spot().is_some()),
        Selection::Toll(id) => w.tolls.get(id).is_some(),
        Selection::Bot(id) => w.bots.bots.contains_key(id),
        Selection::Train(id) => w.rail.trains.contains_key(id),
        Selection::Metro(id) => w.subway.trains.contains_key(id),
        // A street piece is re-pointed across rebuilds by
        // `road_select::keep_on_rebuild` and the road card: left to them.
        Selection::Road(_) => true,
        Selection::Unit(id) => match w.services.units.get(id) {
            None => false,
            Some(u) if crate::fire_scene::unit_pose(w, id, sim.frac()).is_none() => return Selection::Building(u.station),
            Some(_) => true,
        },
        Selection::Ship(id) => w.ports.ships.contains_key(id),
        Selection::Drone(id) => w.air.drones.contains_key(id),
        Selection::Rv(seed) => w.society.homeless.rv(seed).is_some(),
    };
    if alive { sel } else { Selection::None }
}

pub fn validate_selection(sim: Res<Sim>, mut selection: ResMut<Selection>) {
    let now = validated(*selection, &sim);
    if now != *selection {
        *selection = now;
    }
}

/// [fix/review-app] Select `sel` as a click with the Inspect tool does,
/// for the click and the remote API's `select` alike: an emergency unit's
/// car is the unit, and a bus shows its whole line (path, stops, its
/// sister buses). With the Inspect tool, anything else lets the line go.
pub(crate) fn apply_pick(sim: &Sim, tools: &mut ToolState, selection: &mut Selection, sel: Selection) {
    let sel = match sel {
        Selection::Car(id) => sim.world.services.unit_of_car(id).map_or(sel, Selection::Unit),
        s => s,
    };
    *selection = sel;
    let line = match sel {
        Selection::Car(id) => sim.world.traffic.cars.get(id).and_then(|c| c.bus.as_ref()).map(|b| b.route),
        _ => None,
    };
    if line.is_some() || tools.tool == Tool::Inspect {
        tools.route_sel = line;
    }
}
