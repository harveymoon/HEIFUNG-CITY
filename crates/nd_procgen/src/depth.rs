//! Named heights and decal depths: how far each flat layer stands off the
//! surface it is drawn on. Two layers on the same plane z-fight, so every
//! layer that can overlap another has its own value here, and new detail
//! should take a layer from this list (or add one between two) rather than
//! a fresh literal. Many decals still sit on the whole-centimetre grid
//! (frame 0.03, pane 0.05); the off-grid values below exist because
//! something else was already on the grid there.

// ── the street (heights over the street's datum) ─────────────────────────

/// The pavement's top: the kerb's height.
pub const KERB_H: f32 = 0.16;
/// The carriageway's surface.
pub const ASPHALT_Y: f32 = 0.05;
/// Lane lines and arrows, over the asphalt.
pub const MARK_Y: f32 = 0.085;
/// Walk detail's pavement items stand on this: 5 mm under the kerb top
/// (`KERB_H`), as they always have; moving it would lift every cart and
/// sign on the pavement by that much.
pub const WALK_PAVE_Y: f32 = 0.155;
/// Walk detail on the carriageway: the asphalt.
pub const WALK_ROAD_Y: f32 = ASPHALT_Y;

// ── facades (metres proud of the wall) ───────────────────────────────────

/// Grime and streaks.
pub const GRIME: f32 = 0.02;
/// Graffiti: its own layers between the grime and the window frames, off
/// the centimetre grid. Outline under the fill, strokes between.
pub const GRAFFITI_OUTLINE: f32 = 0.024;
pub const GRAFFITI_STROKE: f32 = 0.0262;
pub const GRAFFITI_FILL: f32 = 0.0276;
/// Window frames.
pub const FRAME: f32 = 0.03;
/// Panes, lights and boarding in a frame.
pub const PANE: f32 = 0.05;
/// How far the dark glass under a switchable window light sits behind it:
/// 13 mm, off the centimetre grid (at 2 cm it landed in the plane of the
/// frame and the two z-fought with the light off).
pub const LIGHT_GLASS_BEHIND: f32 = 0.013;
