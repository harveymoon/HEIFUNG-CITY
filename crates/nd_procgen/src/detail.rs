//! [r13-detail] The one set of distance bands every close-up detail pass
//! is drawn within (`docs/rendering_detail.md`).
//!
//! Three bands, from the camera (straight-line distance, metres):
//!
//! - [`DetailBand::Mid`]: what reads from a few blocks away (awnings, cages,
//!   tanks, cores' steel, park benches' frames): building styles'
//!   [`Lod::Mid`] layer.
//! - [`DetailBand::Near`]: the grain seen from the street (AC units, frames,
//!   pipes, railings, bikes in racks, park benches and bins, vacant lots'
//!   junk, garden beds): building styles' [`Lod::Near`] layer, and the
//!   near layers of parks, vacant lots and community gardens.
//! - [`DetailBand::Close`]: what says who lives there or what the weather
//!   is (washing, toys, bikes by the door, pots, grills): the household
//!   layer (`nd_procgen::yards`, drawn by `render/yards.rs`).
//!
//! Every band fades out over its last [`FADE_FROM`] .. 100 % (a Bevy
//! `VisibilityRange`). The household layer is also not placed at all past
//! [`CLOSE_REACH`] along the ground, or when the eye is more than
//! [`CLOSE_EYE`] over it. Nothing else may keep a private fade: a new
//! detail pass picks a band here.

use crate::surface::Lod;

/// A band of camera distance a piece of close-up detail is drawn within.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DetailBand {
    Mid,
    Near,
    Close,
}

/// Every band fades out from this share of its range to its end.
pub const FADE_FROM: f32 = 0.8;

/// The household layer is placed only for homes this near the eye along
/// the ground: a little past where [`DetailBand::Close`] has faded out.
pub const CLOSE_REACH: f32 = DetailBand::Close.range() + 10.0;

/// An eye higher than this over the ground places no household things.
pub const CLOSE_EYE: f32 = 220.0;

impl DetailBand {
    pub const ALL: [DetailBand; 3] = [DetailBand::Mid, DetailBand::Near, DetailBand::Close];

    /// Distance past which the band is not drawn, metres.
    pub const fn range(self) -> f32 {
        match self {
            DetailBand::Mid => 700.0,
            DetailBand::Near => 320.0,
            DetailBand::Close => 140.0,
        }
    }

    /// The distances it fades out between.
    pub fn fade(self) -> std::ops::Range<f32> {
        self.range() * FADE_FROM..self.range()
    }

    /// The band a geometry detail layer is drawn in (None: the base mesh,
    /// always drawn).
    pub fn of(lod: Lod) -> Option<DetailBand> {
        match lod {
            Lod::Base => None,
            Lod::Mid => Some(DetailBand::Mid),
            Lod::Near => Some(DetailBand::Near),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DetailBand::Mid => "mid",
            DetailBand::Near => "near",
            DetailBand::Close => "close",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_nest_and_fade_inside_their_range() {
        let r: Vec<f32> = DetailBand::ALL.iter().map(|b| b.range()).collect();
        assert!(r[0] > r[1] && r[1] > r[2], "mid beyond near beyond close: {r:?}");
        for b in DetailBand::ALL {
            let f = b.fade();
            assert!(f.start < f.end && f.end == b.range());
        }
        assert_eq!(DetailBand::of(Lod::Near).map(|b| b.range()), Some(Lod::NEAR_RANGE));
        assert_eq!(DetailBand::of(Lod::Mid).map(|b| b.range()), Some(Lod::MID_RANGE));
        assert!(CLOSE_REACH > DetailBand::Close.range());
    }
}
