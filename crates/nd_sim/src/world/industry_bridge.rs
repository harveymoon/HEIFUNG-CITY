//! The planning tools' side of the industry sub-zones (feat/industry):
//! the one place the Zone tool's sub-type reaches the industry model. Keys
//! are `IndustryKind::key()` strings, or "auto" to let the ground decide.
//!
//! Wired to feat/industry: `World::set_industry_bias` for the choice,
//! `industry_weights` for the ground's suggestion.

use super::*;

/// The sub-types the Zone tool offers for industrial land: (key, label).
/// "auto" clears any bias so the ground decides, then every
/// `IndustryKind::ALL` in order by its `key()`. [review] Sized from the
/// kinds so a new one cannot be left out (Mining, Smelting and
/// FactoryFarming were); a test checks the keys match.
pub const INDUSTRY_CHOICES: [(&str, &str); crate::industry::IndustryKind::COUNT + 1] = [
    ("auto", "Auto (ground)"),
    ("farming", "Farming"),
    ("oil", "Oil"),
    ("refinery", "Refinery"),
    ("manufacturing", "Manufacturing"),
    ("logistics", "Logistics"),
    ("fishing", "Fishing"),
    ("tech", "Tech"),
    ("mining", "Mining"),
    ("smelting", "Smelting"),
    ("factory_farming", "Factory farming"),
];

impl World {
    /// Set the industry sub-zone of an area (`IndustryArea::Polygon`):
    /// `choice` is a kind key, or "auto" for none.
    pub(super) fn apply_industry_choice(&mut self, polygon: Vec<Vec2>, choice: &str) {
        let kind = crate::industry::IndustryKind::from_key(choice);
        let _ = self.set_industry_bias(super::industry::IndustryArea::Polygon(polygon), kind);
    }

    /// The industry kind the ground under a parcel suggests (its label),
    /// for the Zone tool's read-out. None off industrial land.
    pub fn suggested_industry(&self, parcel: usize) -> Option<String> {
        let center = self.parcels.parcels.get(parcel)?.center;
        let lot = self.lots.iter().find(|(_, l)| point_in_polygon(center, &l.polygon)).map(|(id, _)| id)?;
        let w = self.industry_weights(lot)?;
        let best = (0..w.len()).max_by(|a, b| w[*a].total_cmp(&w[*b]))?;
        crate::industry::IndustryKind::ALL.get(best).map(|k| k.label().to_string())
    }

    /// The sub-zone the player set over a point, if any (its key).
    pub fn industry_choice_at(&self, p: Vec2) -> Option<String> {
        self.industry_bias_at(p).map(|k| k.map_or("auto", |k| k.key()).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::industry::IndustryKind;

    #[test]
    fn the_zone_tool_offers_every_industry_kind() {
        assert_eq!(INDUSTRY_CHOICES[0].0, "auto");
        for (i, k) in IndustryKind::ALL.into_iter().enumerate() {
            assert_eq!(INDUSTRY_CHOICES[i + 1].0, k.key(), "{k:?}");
            assert_eq!(IndustryKind::from_key(INDUSTRY_CHOICES[i + 1].0), Some(k));
        }
    }
}
