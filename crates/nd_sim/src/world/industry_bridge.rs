//! The planning tools' side of the industry sub-zones (feat/industry):
//! the one place the Zone tool's sub-type reaches the industry model. Keys
//! are `IndustryKind::key()` strings, or "auto" to let the ground decide.
//!
//! Until feat/industry is merged these are inert; at merge they call
//! `World::set_industry_bias` and read `industry_weights` / the ground.

use super::*;

/// The sub-types the Zone tool offers for industrial land: (key, label).
/// "auto" clears any bias so the ground decides.
pub const INDUSTRY_CHOICES: [(&str, &str); 8] = [
    ("auto", "Auto (ground)"),
    ("farming", "Farming"),
    ("oil", "Oil"),
    ("refinery", "Refinery"),
    ("manufacturing", "Manufacturing"),
    ("logistics", "Logistics"),
    ("fishing", "Fishing"),
    ("tech", "Tech"),
];

impl World {
    /// Set the industry sub-zone of an area (`IndustryArea::Polygon`):
    /// `choice` is a kind key, or "auto" for none.
    pub(super) fn apply_industry_choice(&mut self, polygon: Vec<Vec2>, choice: &str) {
        // feat/industry: `let kind = crate::industry::IndustryKind::from_key(choice);`
        // `let _ = self.set_industry_bias(IndustryArea::Polygon(polygon), kind);`
        let _ = (polygon, choice);
    }

    /// The industry kind the ground under a parcel suggests (its label),
    /// for the Zone tool's read-out. None until the industry model is in.
    pub fn suggested_industry(&self, parcel: usize) -> Option<String> {
        let _ = self.parcels.parcels.get(parcel)?;
        None
    }

    /// The sub-zone the player set over a point, if any (its key).
    pub fn industry_choice_at(&self, p: Vec2) -> Option<String> {
        let _ = p;
        None
    }
}
