use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::zoning::LotId;

/// Things the simulation did this tick that a frontend may want to react to.
#[derive(Clone, Debug)]
pub enum SimEvent {
    RoadsChanged,
    LotsChanged,
    BuildingBuilt(BuildingId),
    /// Tier or condition changed enough to look different.
    BuildingChanged(BuildingId),
    BuildingRemoved(BuildingId, LotId),
    /// Eminent domain: road or path works reshaped the parcel under a
    /// building, so it was pulled down (a `BuildingRemoved` follows).
    Expropriated(BuildingId, LotId),
    CitizenArrived(CitizenId),
    CitizenLeft(CitizenId),
    NewThought(CitizenId),
    NewDay(u64),
    /// The master plan changed (implemented or loaded).
    PlanChanged,
    /// A business shut down (building stays, goes derelict).
    BusinessClosed(BuildingId),
    // ── services and emergencies (`nd_sim::services`); the u32 is the incident id
    /// A fire caught, a crime happened, someone fell ill.
    IncidentStarted(u32),
    /// Somebody called 911 about it.
    EmergencyCall(u32),
    /// A dispatcher sent a unit.
    UnitDispatched(u32),
    /// The unit got there.
    UnitOnScene(u32),
    /// Fire out, arrest made, patient admitted, or given up on.
    IncidentResolved(u32),
    /// A fire jumped to this building.
    FireSpread(BuildingId),
    /// A fire destroyed the building (a `BuildingRemoved` follows).
    BurnedDown(BuildingId, LotId),
    /// A stadium is hosting a game tonight.
    StadiumEvent(BuildingId),
    /// Police arrested someone over an incident.
    Arrested(u32, CitizenId),
    /// Someone left custody: sentence served, or let go early (no jail).
    Released(CitizenId),
    /// [driving] A crash (`World::driving.recent` id).
    Crash(u32),
    /// [driving] A street takeover started gathering, and one ended.
    Takeover(u32),
    TakeoverOver(u32),
    /// [events] A line of city news about a street race or a rave
    /// (`World::street_events.news`, by its `seq`).
    News(u32),
    /// [park-paths] A path joined to a park (or other grounds) lost its
    /// end there when the grounds went: cut back, or removed if nothing
    /// was left (`world/grounds.rs`).
    PathTrimmed { gesture: crate::plan::GestureId, removed: bool, grounds: crate::walk::GroundsKind },
}
