//! Parcels, zones (land-use polygons) and lots.
//!
//! Parcels are cut along every street and path as soon as it is built,
//! zoned or not ([`generate_parcels_with`]): the road corridors are subtracted
//! from the land within reach of the roads with real polygon booleans, so a
//! curved road removes exactly its own strip and nothing else, and each
//! remaining block is sliced along its road frontage into parcels of a
//! home's size. A parcel is usable only when one of its edges lies along a
//! road; land with no road access is kept as landlocked backland, and
//! slivers are never built on. Zoning then gives parcels a land use
//! ([`crate::plan::parcel_zoning`]) and every zoned parcel becomes a lot;
//! uses that want wider lots join neighbouring parcels ([`join_parcels`]).
//!
//! [`generate_lots`] still cuts a single zone polygon on its own, at the
//! land use's own lot size: tests and the building previews use it.

use crate::buildings::BuildingId;
use crate::geom::{self, bounds as bounds_of, boxes_overlap, perimeter, Grid};
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use geo::{Area, BooleanOps, Coord, LineString, MultiPolygon, Polygon};
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;

pub use crate::geom::{distance_to_outline, point_in_polygon, polygon_area, polygon_centroid};

mod frontage;
mod join;
mod lots;
mod parcels;
mod poly;
mod recut;
mod subdivide;
mod water_mask;

pub use frontage::*;
pub use join::*;
pub use lots::*;
pub use parcels::*;
pub use poly::*;
pub use recut::*;
use subdivide::*;
pub use subdivide::{MAX_PARCEL_AREA, MAX_SHORE_PARCEL_AREA};
pub use water_mask::*;

/// Pavement between the kerb and the parcel line, in metres.
pub const SIDEWALK: f32 = 3.2;
/// Parcels smaller than this are dropped as slivers.
const MIN_AREA: f32 = 60.0;
/// Area over squared perimeter below which a parcel is a sliver (a square is
/// 0.0625, an equilateral triangle 0.048, a 1:4 strip 0.04).
const MIN_COMPACTNESS: f32 = 0.030;
/// Shortest edge that counts as road frontage.
const MIN_FRONTAGE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LandUse {
    Residential,
    Commercial,
    Industrial,
    /// [r4-leisure] Commercial land of the leisure sub-zone (bars, clubs,
    /// venues): the old Entertainment zone, kept as its own value so saves,
    /// clubs and nights out work as before. The player zones it as
    /// Commercial with the leisure sub-zone ([`crate::commercial`]).
    Entertainment,
    Civic,
    Park,
}

impl LandUse {
    pub const ALL: [LandUse; 6] = [
        LandUse::Residential,
        LandUse::Commercial,
        LandUse::Industrial,
        LandUse::Entertainment,
        LandUse::Civic,
        LandUse::Park,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LandUse::Residential => "Residential",
            LandUse::Commercial => "Commercial",
            LandUse::Industrial => "Industrial",
            LandUse::Entertainment => "Leisure",
            LandUse::Civic => "Civic",
            LandUse::Park => "Park",
        }
    }

    pub fn index(self) -> usize {
        LandUse::ALL.iter().position(|&l| l == self).unwrap()
    }

    /// [r4-leisure] The land uses the player zones (the Build window, the
    /// Zone tool, the Demand panel): leisure is a sub-zone of Commercial.
    pub const ZONED: [LandUse; 5] = [LandUse::Residential, LandUse::Commercial, LandUse::Industrial, LandUse::Civic, LandUse::Park];

    /// [r4-leisure] The zone as the player sees it: leisure land is
    /// commercial.
    pub fn zoned_as(self) -> LandUse {
        if self == LandUse::Entertainment {
            LandUse::Commercial
        } else {
            self
        }
    }

    /// [r4-leisure] Commercial land of any sub-zone, leisure included.
    pub fn is_commercial(self) -> bool {
        matches!(self, LandUse::Commercial | LandUse::Entertainment)
    }

    /// [hillside] May go on a steep hillside, terraced: homes (the
    /// cliffside villa), shops and clubs, and parks; not works yards or
    /// civic grounds, which want level ground.
    pub fn terraces(self) -> bool {
        matches!(self, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment | LandUse::Park)
    }

    /// Homes and parks may line a footpath; everything else needs a street.
    pub fn fronts_paths(self) -> bool {
        matches!(self, LandUse::Residential | LandUse::Park)
    }

    /// Typical frontage along the road, in metres.
    pub fn lot_width(self) -> f32 {
        match self {
            LandUse::Residential => 14.0,
            LandUse::Commercial => 18.0,
            LandUse::Industrial => 30.0,
            LandUse::Entertainment => 20.0,
            LandUse::Civic => 30.0,
            LandUse::Park => 24.0,
        }
    }

    pub fn lot_depth(self) -> f32 {
        match self {
            LandUse::Industrial | LandUse::Civic => 36.0,
            LandUse::Park => 28.0,
            _ => 24.0,
        }
    }

    /// Smallest parcel this use may take: no works yard or civic site on a
    /// corner scrap.
    pub fn min_parcel_area(self) -> f32 {
        match self {
            LandUse::Industrial | LandUse::Civic => 200.0,
            _ => MIN_AREA,
        }
    }

    /// Does this use join neighbouring parcels into bigger lots? Every use
    /// whose lots are bigger than a home's does; parks join up as clusters
    /// instead ([`crate::park`]).
    pub fn joins_parcels(self) -> bool {
        self != LandUse::Park && self.lot_area() > PARCEL_USE.lot_area() * 1.1
    }

    /// Uses whose lots run deeper than a parcel (works yards, civic
    /// grounds) take the backland behind their lots, as the deeper rows
    /// they were cut to always did.
    pub fn takes_backland(self) -> bool {
        self != LandUse::Park && self.lot_depth() > PARCEL_USE.lot_depth()
    }

    /// The area of a typical lot: its width by a row's usual depth.
    pub fn lot_area(self) -> f32 {
        self.lot_width() * self.lot_depth() * 1.15
    }

    /// Parse a label or key ("residential", "Park", ...). [r4-leisure]
    /// "entertainment" and "nightlife", the old zone's names, are leisure.
    pub fn parse(s: &str) -> Option<LandUse> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("entertainment") || s.eq_ignore_ascii_case("nightlife") {
            return Some(LandUse::Entertainment);
        }
        LandUse::ALL.into_iter().find(|l| l.label().eq_ignore_ascii_case(s))
    }
}

new_key_type! {
    pub struct ZoneId;
    pub struct LotId;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub polygon: Vec<Vec2>,
    pub land_use: LandUse,
}

impl Zone {
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.polygon)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lot {
    pub zone: ZoneId,
    pub land_use: LandUse,
    /// Exact parcel outline, counter-clockwise.
    pub polygon: Vec<Vec2>,
    /// Bounding rectangle aligned to the frontage: front-left, front-right,
    /// back-right, back-left. The polygon fits inside it.
    pub corners: [Vec2; 4],
    /// The polygon edge that faces the road.
    pub front_edge: [Vec2; 2],
    /// Area centroid.
    pub center: Vec2,
    /// Direction from the lot towards the road.
    pub front_dir: Vec2,
    pub front_segment: SegmentId,
    pub front_t: f32,
    /// Road node citizens use to enter and leave the lot.
    pub access_node: NodeId,
    pub building: Option<BuildingId>,
    /// No edge touches a road, or the shape is a sliver: nothing can be
    /// built here.
    #[serde(default)]
    pub landlocked: bool,
    /// Pedestrians who passed this lot today and in the current hour.
    #[serde(default)]
    pub footfall_today: u32,
    #[serde(default)]
    pub footfall_hour: u32,
    /// The street a car can reach this lot from and where it stops. Lots
    /// that line a path have no car access; residents park nearby.
    #[serde(default)]
    pub car_segment: Option<SegmentId>,
    #[serde(default)]
    pub car_point: Vec2,
    /// This lot's share of a park: paths, benches and features.
    #[serde(default)]
    pub park: Option<crate::park::ParkLayout>,
    /// [waterfront] Where the lot meets the water, if it does: frontage
    /// on the shoreline, which way the water lies, and whether the lot is
    /// a public beach (`crate::shore`).
    #[serde(default)]
    pub shore: LotShore,
}

/// [waterfront] A lot's edge on the water. Zero frontage: not waterfront.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LotShore {
    /// Metres of the outline that run along the waterline.
    pub frontage: f32,
    /// Unit vector from the lot out over the water (zero when inland).
    pub seaward: Vec2,
    /// A public beach (a park painted as one with the Zone tool).
    pub beach: bool,
}

impl LotShore {
    /// Frontage that makes a lot a waterfront lot, metres.
    pub const MIN_FRONTAGE: f32 = 6.0;

    pub fn is_waterfront(&self) -> bool {
        self.frontage >= Self::MIN_FRONTAGE
    }
}

impl Lot {
    /// Frontage of the bounding rectangle.
    pub fn width(&self) -> f32 {
        self.corners[0].distance(self.corners[1])
    }

    /// Depth of the bounding rectangle.
    pub fn depth(&self) -> f32 {
        self.corners[1].distance(self.corners[2])
    }

    pub fn area(&self) -> f32 {
        polygon_area(&self.polygon).abs()
    }

    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.polygon)
    }

    /// Where people step off the pavement onto the lot.
    pub fn entrance(&self) -> Vec2 {
        self.front_edge[0].lerp(self.front_edge[1], 0.5)
    }

    pub fn usable(&self) -> bool {
        !self.landlocked
    }

    /// Does the lot front a street (as opposed to a footpath)?
    pub fn car_access(&self) -> bool {
        self.car_segment == Some(self.front_segment)
    }
}

use crate::geom::segments_cross;

use crate::geom::extents;

// ── geo bridges ──────────────────────────────────────────────────────────────

// ── parcels cut before zoning ───────────────────────────────────────────────

#[cfg(test)]
thread_local! {
    static DEAD_REASONS: std::cell::RefCell<Vec<(Vec2, &'static str, f32, f32)>> = std::cell::RefCell::new(Vec::new());
}

#[cfg(test)]
mod tests;
