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
}
