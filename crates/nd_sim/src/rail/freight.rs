//! Rail freight: orders, wagons and manifests.

use super::*;

/// [freight] Imports ordered by rail, waiting out of town for a freight
/// train: the load (`from` is the yard it comes off at) and the tick it was
/// ordered.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RailOrder {
    pub cargo: Cargo,
    #[serde(default)]
    pub since: u64,
}

/// [freight] Load a freight train at the edge of the map: the orders for
/// `yards` (the ones it calls at), oldest first, as many as fit in
/// `capacity`. An order that does not fit is left for the next train and
/// smaller ones behind it may go instead; an empty train always takes the
/// first, so no order is too big to ever leave. Orders for other yards stay.
pub fn fill_wagons(orders: &mut Vec<RailOrder>, yards: &[BuildingId], capacity: f32) -> Vec<Cargo> {
    let mut aboard: Vec<Cargo> = Vec::new();
    let mut load = 0.0f32;
    orders.sort_by_key(|o| o.since);
    orders.retain(|o| {
        if !yards.contains(&o.cargo.from) {
            return true;
        }
        if load + o.cargo.amount <= capacity + 1e-3 || aboard.is_empty() {
            load += o.cargo.amount;
            aboard.push(o.cargo);
            false
        } else {
            true
        }
    });
    aboard
}

/// [freight] Wagons for a freight train with `units` to carry.
pub fn wagons_for(units: f32) -> u8 {
    ((units / WAGON_CAPACITY).ceil().max(0.0) as u32).clamp(MIN_WAGONS as u32, MAX_WAGONS as u32) as u8
}

/// [train-inspect] One load of goods a freight train brings in from out of
/// town: what and how much, the yard it comes off at, and who it is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreightLoad {
    pub resource: crate::market::Resource,
    pub amount: f32,
    /// What the buyer pays for it.
    pub price: f32,
    /// The yard it is set down at.
    pub yard: BuildingId,
    /// The business that ordered it.
    pub buyer: BuildingId,
    /// The cargo hub it passes through on the way to the buyer, if any.
    pub hub: Option<BuildingId>,
}

/// [train-inspect] Everything on a freight train and still to be picked up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FreightManifest {
    /// [freight] Imports aboard (loaded at the edge of the map), to be set
    /// down at the yards ahead, in call order.
    pub loads: Vec<FreightLoad>,
    /// [freight] Wagons, the units they hold and the units aboard.
    pub wagons: u8,
    pub capacity: f32,
    pub aboard: f32,
    /// Units of the town's exports aboard, going out of town.
    pub exports_aboard: f32,
    /// Exports waiting at the yards ahead, picked up when it calls (as
    /// much as the wagons have room for then).
    pub exports_to_collect: Vec<(BuildingId, f32)>,
    /// Units of imports already set down at the yards it called at.
    pub unloaded: f32,
}

impl FreightManifest {
    pub fn import_units(&self) -> f32 {
        self.loads.iter().map(|l| l.amount).sum()
    }
}

/// [r4] (K33) Units of goods one twenty-foot container (one TEU) holds: a
/// wagon carries two, or one forty-footer.
pub const TEU_UNITS: f32 = WAGON_CAPACITY / 2.0;
/// [r4] (K33) Traffic seconds the yard crane takes per container, and the
/// shortest and longest stand of a freight train at a yard.
pub const LIFT_SECONDS: f32 = 4.0;
pub const FREIGHT_DWELL_MAX: f32 = 96.0;

/// [r4] (K33) What sort of box goods travel in, by what they are: a dry
/// box for most, a reefer (refrigerated, its machinery at the front end)
/// for food that keeps cold, an ISO tank in its frame for liquids, and an
/// open-top box under a tarp for loose bulk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoxKind {
    Dry,
    Reefer,
    Tank,
    OpenTop,
}

impl BoxKind {
    pub fn for_resource(r: Option<crate::market::Resource>) -> BoxKind {
        use crate::market::Resource as R;
        match r {
            // [r8] Groceries are mostly packaged dry goods: a dry box (a
            // port full of white reefers read as faded boxes).
            Some(R::Meat | R::Dairy | R::Fish | R::Fillets | R::Produce) => BoxKind::Reefer,
            Some(R::Crude | R::Fuel | R::Plastics) => BoxKind::Tank,
            Some(R::Ore | R::Grain) => BoxKind::OpenTop,
            _ => BoxKind::Dry,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BoxKind::Dry => "dry box",
            BoxKind::Reefer => "reefer",
            BoxKind::Tank => "tank container",
            BoxKind::OpenTop => "open-top box",
        }
    }
}

/// [r4] (K33) Where a container is in a yard call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lift {
    /// On its wagon.
    Aboard,
    /// Lifted off at the yard the train stands at: the `n`-th move of the
    /// crane's work there.
    Off(u8),
    /// Lifted on there (an export), the `n`-th move.
    On(u8),
}

/// [r4] (K33) One standard ISO container on a freight wagon: the wagon
/// (vehicle index, 1 is the first behind the locomotive), its place on it
/// (slot 0 the front half, 1 the back; a forty-footer takes the whole
/// wagon at slot 0), what is in it, the yard it comes off at (`None`: an
/// export, leaving town), the seed of its shipping line and paint, and
/// whether the crane is moving it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxLoad {
    pub wagon: u8,
    pub slot: u8,
    pub forty: bool,
    pub kind: BoxKind,
    pub resource: Option<crate::market::Resource>,
    pub yard: Option<BuildingId>,
    pub seed: u32,
    pub lift: Lift,
    /// [K34] The box's record (`World::containers`), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<crate::containers::BoxId>,
}

/// [r4] (K33) Box loads for `cargo` on a train of `wagons` wagons: each
/// load in as many TEU as its units need (at least one), forty-footers
/// while two or more are left (never tanks: those are twenty-foot frames),
/// in load order from the first wagon back. A load bigger than the wagons
/// left is boxed as far as they go (the train took an outsize order).
pub fn box_cargo(cargo: &[Cargo], wagons: u8, seed: u32) -> Vec<BoxLoad> {
    let mut boxes = Vec::new();
    let mut slots = free_slots(&boxes, wagons).into_iter();
    for (i, c) in cargo.iter().enumerate() {
        let kind = BoxKind::for_resource(Some(c.resource));
        let mut teu = ((c.amount / TEU_UNITS).ceil() as u32).max(1);
        while teu > 0 {
            let Some((wagon, slot)) = slots.next() else { return boxes };
            // A forty-footer needs the wagon to itself.
            let forty = teu >= 2 && kind != BoxKind::Tank && slot == 0;
            if forty {
                slots.next();
            }
            let n = boxes.len() as u32;
            boxes.push(BoxLoad { wagon, slot, forty, kind, resource: Some(c.resource), yard: Some(c.from), seed: crate::hash::mix(seed as u64, ((i as u64) << 20) ^ n as u64) as u32, lift: Lift::Aboard, id: None });
            teu -= if forty { 2 } else { 1 };
        }
    }
    boxes
}

/// [r4] (K33) The wagon slots no box takes, front of the train first.
pub fn free_slots(boxes: &[BoxLoad], wagons: u8) -> Vec<(u8, u8)> {
    let taken = |w: u8, s: u8| boxes.iter().any(|b| b.wagon == w && (b.slot == s || b.forty));
    (1..=wagons).flat_map(|w| [(w, 0u8), (w, 1u8)]).filter(|&(w, s)| !taken(w, s)).collect()
}

/// [r4] (K33) The stand a freight train makes for `moves` crane moves.
pub fn freight_dwell(moves: usize) -> f32 {
    (10.0 + moves as f32 * LIFT_SECONDS).clamp(DWELL_SECONDS, FREIGHT_DWELL_MAX)
}
