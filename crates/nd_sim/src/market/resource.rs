//! Resources: what is made, bought and hauled. Their facts (label, price,
//! box, lorry, ...) are rows of the catalogue (`crate::catalogue::GOODS`).

use super::*;

/// [r13-catalogue] Add a good at the end, with its row in
/// `crate::catalogue` (see `docs/catalogue.md`); saves keep their keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resource {
    Produce,
    Grain,
    Flour,
    BakedGoods,
    Meat,
    Dairy,
    Groceries,
    Parts,
    Electronics,
    Goods,
    // -- industry block: oil and fish --
    /// Crude oil from the wells.
    Crude,
    /// Refined fuel: diesel for the lorries and boats.
    Fuel,
    /// Plastics and chemicals from the refinery.
    Plastics,
    /// The catch, landed by the fishing boats.
    Fish,
    // -- [economy] new supply chains, appended so saved maps keep their keys --
    /// Cattle and poultry from the cow and chicken farms, for the slaughterhouse.
    Livestock,
    /// Rock and ore dug out of a strip mine; sold out of town.
    Ore,
    /// Gutted, filleted fish: from the gutting sheds to the fish-stick
    /// factory, the fish markets and the seafood restaurants.
    Fillets,
    /// Paintings and sculpture from the artists' studios, hung and sold by
    /// the galleries.
    Art,
    /// [dirty-industry] Ingots and billets from the smelters; sold out of
    /// town. Appended.
    Metal,
    // -- [r14-materials] building materials (the supplies review), appended --
    /// Cement in bulk: mixed with sand and gravel into concrete.
    Cement,
    /// Sand and gravel for concrete, fill and paths.
    Aggregate,
    /// Bricks and concrete blocks on pallets.
    Bricks,
    /// Flat glass: window panes and curtain-wall units.
    Glass,
    /// Structural steel: beams, columns, decking.
    Steel,
    /// Reinforcing bar for concrete.
    Rebar,
    /// Sawn timber and boards.
    Timber,
    /// Roof sheets and tiles.
    Roofing,
    /// Insulation batts and boards.
    Insulation,
    /// Pipes and plumbing.
    Pipe,
    /// Cable, switchgear and the electrical fit-out.
    Wiring,
    /// Fixtures and fittings: doors, kitchens, sanitaryware, lifts.
    Fixtures,
    /// Paint and finishes.
    Paint,
}

impl Resource {
    pub const COUNT: usize = 32;
    pub const ALL: [Resource; Self::COUNT] = [
        Resource::Produce,
        Resource::Grain,
        Resource::Flour,
        Resource::BakedGoods,
        Resource::Meat,
        Resource::Dairy,
        Resource::Groceries,
        Resource::Parts,
        Resource::Electronics,
        Resource::Goods,
        Resource::Crude,
        Resource::Fuel,
        Resource::Plastics,
        Resource::Fish,
        Resource::Livestock,
        Resource::Ore,
        Resource::Fillets,
        Resource::Art,
        Resource::Metal,
        Resource::Cement,
        Resource::Aggregate,
        Resource::Bricks,
        Resource::Glass,
        Resource::Steel,
        Resource::Rebar,
        Resource::Timber,
        Resource::Roofing,
        Resource::Insulation,
        Resource::Pipe,
        Resource::Wiring,
        Resource::Fixtures,
        Resource::Paint,
    ];

    /// Its row in the catalogue.
    pub fn row(self) -> &'static crate::catalogue::GoodRow {
        crate::catalogue::good(self)
    }

    pub fn label(self) -> &'static str {
        self.row().label
    }

    /// Reference price per unit; the outer market trades around it.
    pub fn base_price(self) -> f32 {
        self.row().base_price
    }

    /// Hours of its own output a business keeps before shipping the rest
    /// out of town. [dirty-industry] Ingots have no local buyer: they go
    /// as soon as a lorryload stands in the yard.
    pub fn export_keep_hours(self) -> f32 {
        self.row().export_keep_hours
    }

    /// [dirty-industry] How much harder than an ordinary load a lorry full
    /// of this is on the road: ore, ingots and crude are dense, livestock
    /// lorries heavy and sloshing (`crate::road_wear::HEAVY` times this).
    pub fn road_wear_factor(self) -> f32 {
        self.row().road_wear
    }

    /// Sold to citizens directly, not to other businesses.
    pub fn is_retail(self) -> bool {
        self.row().retail
    }
}
