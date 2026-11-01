//! [attractions] The city's landmark attractions: museums, the pleasure
//! pier, the grand park, the zoo, the observatory, the aquarium, the
//! botanical garden and the opera house (`ServiceKind::LANDMARKS`).
//!
//! Each is one per city, stands on a site of several plots joined into one
//! (`World::joined_site`, like a stadium's), lifts the land value of the
//! quarter round it through its own channel (`Channel::Attractions`) and
//! draws visitors: tourists from out of town (`crate::tourism`) and the
//! town's own people on an afternoon or a night out (`World::choose_place`).
//!
//! Pure data and geometry here; the world side (placement rules, grounds,
//! visits, the daily books) is in `world/attractions.rs`.

use crate::buildings::{BuildingId, Tier};
use crate::geom::{inset_ring, point_in_polygon, polygon_area, project_on_segment};
use crate::landscape::{Landscape, WaterKind};
use crate::park::{ParkFeature, ParkLayout};
use crate::services::ServiceKind;
use crate::zoning::Lot;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How far a pleasure pier reaches out over the sea at most, and at least
/// (shallower bays take a shorter one), metres.
pub const PIER_LENGTH: f32 = 120.0;
pub const PIER_MIN_LENGTH: f32 = 60.0;
/// Width of the pier's walk, and the square head at its end where the
/// rides stand.
pub const PIER_WIDTH: f32 = 14.0;
pub const PIER_HEAD: f32 = 36.0;
/// How near the sea a pier's site must come, metres.
pub const PIER_SHORE_REACH: f32 = 14.0;
/// How far a pier's boards stand above its lot's pad, metres.
pub const PIER_DECK_RISE: f32 = 0.35;
/// Metres an observatory's site must stand above the middle of the town
/// (the median ground height of every lot).
pub const OBSERVATORY_RISE: f32 = 8.0;

/// A pleasure pier's deck out over the water: from `root` on the shore to
/// `tip`, `width` wide, with a head `head` metres square at the far end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PierDeck {
    pub root: Vec2,
    pub tip: Vec2,
    /// Unit vector out over the water.
    pub seaward: Vec2,
    pub width: f32,
    pub head: f32,
}

impl PierDeck {
    pub fn length(&self) -> f32 {
        self.root.distance(self.tip)
    }

    /// Across the pier (to the left looking out to sea).
    pub fn side(&self) -> Vec2 {
        self.seaward.perp()
    }

    /// The middle of the head, where the ferris wheel stands.
    pub fn head_centre(&self) -> Vec2 {
        self.tip - self.seaward * (self.head * 0.5)
    }

    /// The deck's outline: the walk and its head, as one polygon (world).
    pub fn outline(&self) -> Vec<Vec2> {
        let (s, w, h) = (self.side(), self.width * 0.5, self.head * 0.5);
        let neck = self.tip - self.seaward * self.head;
        vec![self.root - s * w, neck - s * w, neck - s * h, self.tip - s * h, self.tip + s * h, neck + s * h, neck + s * w, self.root + s * w]
    }

    /// Is `p` on the deck?
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.outline())
    }

    /// The deck as straight decks for the built ground (`Ground::decks`):
    /// the walk and the head, each as a centre line and a half width.
    pub fn spans(&self) -> [(Vec2, Vec2, f32); 2] {
        let neck = self.tip - self.seaward * self.head;
        [(self.root, neck, self.width * 0.5 + 0.3), (neck, self.tip, self.head * 0.5 + 0.3)]
    }
}

/// Where a pleasure pier goes off a site (`poly`, world): from the sample
/// of its outline nearest the sea, square to the shore, as far out as open
/// sea allows (`PIER_LENGTH`, at least `PIER_MIN_LENGTH`). `Err` says why
/// not, for the Build window's hover.
pub fn plan_pier(poly: &[Vec2], land: &Landscape) -> Result<PierDeck, String> {
    if land.is_flat() || poly.len() < 3 {
        return Err("a pier goes on the sea shore: this map has no sea".into());
    }
    // Every few metres round the outline, the nearest waterline.
    let n = poly.len();
    let mut best: Option<(f32, crate::landscape::Shore)> = None;
    let mut other: Option<WaterKind> = None;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let steps = (a.distance(b) / 3.0).ceil().max(1.0) as usize;
        for k in 0..steps {
            let p = a.lerp(b, (k as f32 + 0.5) / steps as f32);
            let Some(s) = land.nearest_shore(p, PIER_SHORE_REACH) else { continue };
            if s.water.kind != WaterKind::Sea {
                other.get_or_insert(s.water.kind);
                continue;
            }
            if best.as_ref().is_none_or(|(d, _)| s.distance < *d - 1e-3) {
                best = Some((s.distance, s));
            }
        }
    }
    let Some((_, shore)) = best else {
        return Err(match other {
            Some(k) => format!("a pier goes on the sea shore, not by {}", crate::names::a(k.label())),
            None => "a pier goes on the sea shore: this site does not reach the sea".into(),
        });
    };
    let sea = shore.seaward;
    let side = sea.perp();
    let open = |p: Vec2| land.water_at(p).is_some_and(|w| w.kind == WaterKind::Sea);
    let mut len = PIER_LENGTH;
    while len >= PIER_MIN_LENGTH {
        let tip = shore.point + sea * len;
        let deck = PierDeck { root: shore.point - sea * 4.0, tip, seaward: sea, width: PIER_WIDTH, head: PIER_HEAD };
        // Open sea all along the walk and under every corner of the head.
        let walk_ok = (1..=((len / 8.0) as usize)).all(|k| open(shore.point + sea * (k as f32 * 8.0).min(len)));
        let neck = tip - sea * PIER_HEAD;
        let head_ok = [-0.5f32, 0.0, 0.5].iter().all(|&s| open(neck + side * (s * PIER_HEAD)) && open(tip + side * (s * PIER_HEAD)));
        if walk_ok && head_ok {
            return Ok(deck);
        }
        len -= 10.0;
    }
    Err("open sea is too far out or too narrow here for a pier".into())
}

/// One attraction's figures.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AttractionBook {
    /// Visits by the town's own people today and yesterday.
    pub visits_today: u32,
    pub visits_yesterday: u32,
    /// Visits by tourists today and yesterday.
    pub tourists_today: u32,
    pub tourists_yesterday: u32,
    /// Admissions taken at the door (to the treasury).
    pub takings_today: f32,
    pub takings_yesterday: f32,
    pub visits_total: u64,
}

impl AttractionBook {
    pub fn roll(&mut self) {
        self.visits_yesterday = std::mem::take(&mut self.visits_today);
        self.tourists_yesterday = std::mem::take(&mut self.tourists_today);
        self.takings_yesterday = std::mem::take(&mut self.takings_today);
    }
}

/// The landmark attractions' state: the piers' decks and every
/// attraction's books. Old saves load with none.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Attractions {
    pub piers: HashMap<BuildingId, PierDeck>,
    pub books: HashMap<BuildingId, AttractionBook>,
}

impl Attractions {
    pub fn book(&self, b: BuildingId) -> AttractionBook {
        self.books.get(&b).cloned().unwrap_or_default()
    }
}

/// What a landmark is and does, in a sentence or two (the inspector card
/// and the Build window).
pub fn about(kind: ServiceKind) -> &'static str {
    match kind {
        ServiceKind::ArtMuseum => "Galleries of painting, light and neon art round a sculpture court. Draws visitors by day; lifts the land value of the quarter round it.",
        ServiceKind::ScienceMuseum => "Dinosaur skeletons, a planetarium dome and a hall of machines. A big draw for families and visitors; lifts the land value round it.",
        ServiceKind::HistoryMuseum => "The story of the city and its design: old street signs, models, the first tram. A steady draw; lifts the land value round it.",
        ServiceKind::Pier => "A pleasure pier on piles out over the sea: a ferris wheel, a coaster, an arcade and food stands, all neon after dark. Open till one in the morning; people walk its boardwalk; a great draw for tourists.",
        ServiceKind::GrandPark => "A central park over many plots: lakes, lawns, a loop of paths, a boathouse and a glass pavilion. Open all hours, free; lifts the land value of a wide district more than anything else.",
        ServiceKind::Zoo => "Enclosures round a visitors' loop: savanna, elephants, big cats, a penguin pool and an aviary. Families come by day; a big draw for visitors.",
        ServiceKind::Observatory => "Telescope domes on high ground with a terrace over the city. Open from the afternoon into the night; stargazers and visitors come up the hill.",
        ServiceKind::Aquarium => "Tanks of reef fish, a shark tunnel and a jellyfish hall glowing blue. A big draw by day and into the evening.",
        ServiceKind::BotanicalGarden => "Gardens of flower beds and pools round a great glass conservatory of palms. People stroll and sit; it lifts the land value round it.",
        ServiceKind::ConcertHall => "An opera house of white sails over a glass foyer. Evening performances draw a crowd that dines and drinks nearby.",
        ServiceKind::AmusementPark => "A coaster, a ferris wheel, a drop tower, a carousel and a midway of food stands round a loop. Families by day, the lights till eleven; the biggest draw for visitors, but noisy neighbours.",
        ServiceKind::ConventionCentre => "Exhibition halls under a long glass-fronted roof and a plaza for the crowds. Trade fairs and expos bring visitors who fill the hotels.",
        ServiceKind::Cathedral => "A nave of stone under twin towers and spires, a rose window lit at night, and a square before it. Free to visit; pilgrims and sightseers come from far off.",
        ServiceKind::CasinoResort => "A hotel tower over a neon casino floor and a pool deck. Open all hours: visitors come for the tables and the shows; the land round it gains less than you might think.",
        // [r6-architects]
        ServiceKind::LedgeHouse => "A house of stone and pale concrete terraces cantilevered out over the water or down the slope, round a great hearth. Guided tours by day; lifts the land value round it.",
        ServiceKind::PrairieHall => "Long, low craft halls under deep hipped eaves, ribbon windows of art glass and a broad brick chimney. Workshops and shows by day; lifts the land value round it.",
        ServiceKind::HillChapel => "A white chapel of thick curved walls pierced with coloured slots under a dark concrete roof that sails up to the view. Free to visit; pilgrims and architecture lovers come from far off.",
        ServiceKind::SkySlab => "A housing slab raised on sculpted pillars: coloured loggias behind a concrete sun-screen, a shopping street half way up and a roof terrace with a track and a cafe. Tours by day, the roof bar till late.",
        ServiceKind::FanLibrary => "A white library with a timber wave at its door and a reading hall that fans out to the view under round skylights. Free; open into the evening; lifts the land value round it.",
        ServiceKind::CultureHouse => "A red-brick concert hall shaped like a fan, stepped clerestories over its roof, round a courtyard of grass steps. Concerts and talks into the night.",
        _ => "",
    }
}

/// The walkways and benches of a landmark people walk round (the grand
/// park, the zoo, the botanical garden, the pier), in world coordinates;
/// the walk graph joins them like a park's (`WalkGraph::build`). The pier's
/// run out along its deck.
pub fn plan_grounds(kind: ServiceKind, lot: &Lot, pier: Option<&PierDeck>) -> Option<ParkLayout> {
    let area = polygon_area(&lot.polygon).abs();
    let gate = {
        let g = lot.entrance() - lot.front_dir * 0.8;
        if point_in_polygon(g, &lot.polygon) { g } else { crate::park::pull_inside(&lot.polygon, lot.entrance(), lot.center) }
    };
    let mut layout = match kind {
        // A grand park: the high-tier plan of the whole site (lakes, rides,
        // a bandstand, fountains), and a loop round it.
        ServiceKind::GrandPark | ServiceKind::BotanicalGarden => crate::park::plan_single(lot, Tier::High),
        _ => ParkLayout { tier: Tier::High, paths: Vec::new(), benches: Vec::new(), features: Vec::new(), entrances: vec![gate], shared_edges: Vec::new(), cluster_lots: 1, cluster_area: area, anchor: true, seams: Vec::new(), preserve: false },
    };
    match kind {
        ServiceKind::GrandPark => {
            add_loop(&mut layout, lot, 7.0, gate, 18.0);
        }
        ServiceKind::BotanicalGarden => {
            // The conservatory stands in the middle: rides and courts are
            // not a botanical garden's, and nothing stands where it does.
            let hub = conservatory_spot(lot);
            layout.features.retain(|f| matches!(f.kind, ParkFeature::Pond | ParkFeature::Lake | ParkFeature::Fountain | ParkFeature::FlowerBed | ParkFeature::Statue | ParkFeature::Lawn | ParkFeature::Kiosk) && f.pos.distance(hub) > conservatory_radius(lot) + f.radius + 2.0);
            add_loop(&mut layout, lot, 6.0, gate, 16.0);
        }
        ServiceKind::Zoo => {
            // The visitors' loop well inside the fence, a way in from the
            // gate and a cross path; benches along the loop.
            add_loop(&mut layout, lot, 11.0, gate, 22.0);
            if let Some(ring) = layout.paths.first().cloned() {
                let mid = crate::geom::polygon_centroid(&lot.polygon);
                let near = |p: Vec2| ring.iter().copied().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap_or(p);
                let far = ring.iter().copied().max_by(|a, b| a.distance(gate).total_cmp(&b.distance(gate))).unwrap_or(mid);
                layout.paths.push(vec![gate, near(gate)]);
                if point_in_polygon(mid, &lot.polygon) {
                    layout.paths.push(vec![near(gate), mid, far]);
                }
            }
        }
        ServiceKind::AmusementPark => {
            // (D4) The midway: a loop round the rides, a way in from the
            // gate across the middle, benches along it.
            add_loop(&mut layout, lot, 9.0, gate, 20.0);
            if let Some(ring) = layout.paths.first().cloned() {
                let mid = crate::geom::polygon_centroid(&lot.polygon);
                let far = ring.iter().copied().max_by(|a, b| a.distance(gate).total_cmp(&b.distance(gate))).unwrap_or(mid);
                if point_in_polygon(mid, &lot.polygon) {
                    layout.paths.push(vec![gate, mid, far]);
                }
            }
        }
        ServiceKind::Pier => {
            let deck = pier?;
            // From the gate down to the shore end, out along the walk to
            // the head, and round the head.
            let neck = deck.tip - deck.seaward * deck.head;
            let s = deck.side();
            let h = deck.head * 0.5 - 4.0;
            let ring = [neck + deck.seaward * 4.0 + s * h, deck.tip - deck.seaward * 4.0 + s * h, deck.tip - deck.seaward * 4.0 - s * h, neck + deck.seaward * 4.0 - s * h];
            layout.paths.push(vec![gate, deck.root, neck + deck.seaward * 4.0]);
            layout.paths.push(vec![neck + deck.seaward * 4.0, ring[0], ring[1], ring[2], ring[3], neck + deck.seaward * 4.0]);
            // Benches along both rails of the walk, looking out to sea.
            let len = deck.length() - deck.head;
            let mut d = 12.0;
            while d < len - 4.0 {
                for side in [-1.0f32, 1.0] {
                    let p = deck.root + deck.seaward * d + s * side * (deck.width * 0.5 - 1.2);
                    layout.benches.push((p, (s * side).to_angle()));
                }
                d += 14.0;
            }
            for c in ring {
                let out = (c - deck.head_centre()).normalize_or_zero();
                layout.benches.push((c + out * 1.5, out.to_angle()));
            }
        }
        _ => return None,
    }
    layout.entrances = layout.paths.iter().filter_map(|p| p.first().copied()).collect();
    layout.entrances.dedup_by(|a, b| a.distance(*b) < 0.3);
    layout.cluster_lots = 1;
    layout.cluster_area = area;
    layout.anchor = true;
    Some(layout)
}

/// A loop path `inset` metres in from the site's edge, starting at its
/// point nearest the gate, with a bench every `every` metres facing in.
fn add_loop(layout: &mut ParkLayout, lot: &Lot, inset: f32, gate: Vec2, every: f32) {
    let ring = inset_ring(&lot.polygon, inset);
    if ring.len() < 3 {
        return;
    }
    let n = ring.len();
    let start = (0..n).min_by(|&a, &b| ring[a].distance(gate).total_cmp(&ring[b].distance(gate))).unwrap_or(0);
    let mut path: Vec<Vec2> = (0..=n).map(|k| ring[(start + k) % n]).collect();
    // From the gate onto the loop.
    path.insert(0, gate);
    let centre = crate::geom::polygon_centroid(&lot.polygon);
    let mut acc = every * 0.5;
    for w in path[1..].windows(2) {
        let (a, b) = (w[0], w[1]);
        let l = a.distance(b);
        while acc < l {
            let p = a.lerp(b, acc / l);
            let inward = (centre - p).normalize_or_zero();
            let seat = p + inward * 1.6;
            if point_in_polygon(seat, &lot.polygon) {
                layout.benches.push((seat, (-inward).to_angle()));
            }
            acc += every;
        }
        acc -= l;
    }
    layout.paths.push(path);
}

/// Where the botanical garden's conservatory stands: the site's deepest
/// point, and how big it is.
pub fn conservatory_spot(lot: &Lot) -> Vec2 {
    crate::geom::polygon_centroid(&lot.polygon)
}

pub fn conservatory_radius(lot: &Lot) -> f32 {
    (crate::park::inradius(&lot.polygon) * 0.55).clamp(8.0, 18.0)
}

/// The point on a pier's walk nearest `p`, for visitors standing on it.
pub fn on_deck(deck: &PierDeck, p: Vec2) -> Vec2 {
    project_on_segment(p, deck.root, deck.tip).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pier_deck_outline_holds_its_walk_and_head() {
        let d = PierDeck { root: Vec2::ZERO, tip: Vec2::new(0.0, 100.0), seaward: Vec2::Y, width: 14.0, head: 36.0 };
        assert!(d.contains(Vec2::new(0.0, 10.0)));
        assert!(!d.contains(Vec2::new(12.0, 10.0)), "beside the walk is water");
        assert!(d.contains(Vec2::new(15.0, 90.0)), "the head is wide");
        assert!((d.length() - 100.0).abs() < 1e-3);
        assert!(d.head_centre().distance(Vec2::new(0.0, 82.0)) < 1e-3);
    }

    #[test]
    fn every_landmark_says_what_it_is() {
        for k in ServiceKind::LANDMARKS {
            assert!(!about(k).is_empty(), "{k:?}");
            assert!(k.admission() >= 0.0 && k.tourist_pull() > 0.0, "{k:?}");
        }
    }
}
