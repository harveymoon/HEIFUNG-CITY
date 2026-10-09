//! Resources: what is made, bought and hauled, with base prices.

use super::*;

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
}

impl Resource {
    pub const ALL: [Resource; 19] = [
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
    ];

    pub fn label(self) -> &'static str {
        match self {
            Resource::Produce => "produce",
            Resource::Grain => "grain",
            Resource::Flour => "flour",
            Resource::BakedGoods => "baked goods",
            Resource::Meat => "meat",
            Resource::Dairy => "dairy",
            Resource::Groceries => "groceries",
            Resource::Parts => "parts",
            Resource::Electronics => "electronics",
            Resource::Goods => "goods",
            Resource::Crude => "crude oil",
            Resource::Fuel => "fuel",
            Resource::Plastics => "plastics",
            Resource::Fish => "fish",
            Resource::Livestock => "livestock",
            Resource::Ore => "ore",
            Resource::Fillets => "fish fillets",
            Resource::Art => "art",
            Resource::Metal => "metal",
        }
    }

    /// Reference price per unit; the outer market trades around it.
    pub fn base_price(self) -> f32 {
        match self {
            Resource::Produce => 2.0,
            Resource::Grain => 1.0,
            Resource::Flour => 2.4,
            Resource::BakedGoods => 4.5,
            Resource::Meat => 7.0,
            Resource::Dairy => 3.0,
            Resource::Groceries => 6.0,
            Resource::Parts => 9.0,
            Resource::Electronics => 45.0,
            Resource::Goods => 60.0,
            Resource::Crude => 4.0,
            Resource::Fuel => 12.0,
            Resource::Plastics => 16.0,
            Resource::Fish => 3.0,
            Resource::Livestock => 3.5,
            Resource::Ore => 6.0,
            Resource::Fillets => 6.0,
            Resource::Art => 30.0,
            // Dear enough that a smelter on local ore pays its way shipping
            // ingots out of town at the export discount.
            Resource::Metal => 75.0,
        }
    }

    /// Hours of its own output a business keeps before shipping the rest
    /// out of town. [dirty-industry] Ingots have no local buyer: they go
    /// as soon as a lorryload stands in the yard.
    pub fn export_keep_hours(self) -> f32 {
        match self {
            Resource::Metal => 8.0,
            _ => 48.0,
        }
    }

    /// [dirty-industry] How much harder than an ordinary load a lorry full
    /// of this is on the road: ore, ingots and crude are dense, livestock
    /// lorries heavy and sloshing (`crate::road_wear::HEAVY` times this).
    pub fn road_wear_factor(self) -> f32 {
        match self {
            Resource::Ore | Resource::Metal => 2.5,
            Resource::Crude => 2.0,
            Resource::Livestock => 1.6,
            _ => 1.0,
        }
    }

    /// Sold to citizens directly, not to other businesses.
    pub fn is_retail(self) -> bool {
        matches!(self, Resource::Groceries | Resource::BakedGoods | Resource::Goods)
    }
}
