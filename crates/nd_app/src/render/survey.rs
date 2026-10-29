//! The land survey layer (`Overlays::land`, K steps through them): a
//! coloured wash over the ground showing a resource (farmland, oil,
//! geothermal vents, rock, sand, water, deep water for ports) or where each
//! industry would do best, read from `nd_sim::survey` so the map shows the
//! sim's own figures.
//!
//! The static ground survey depends only on the landscape and takes a
//! moment to build, so it is built on a worker thread the first time a
//! layer is wanted on a map and kept until the landscape changes. The
//! industry weights move with the city and are recomputed on the main
//! thread when the layer comes on and then at most once a sim day (or when
//! roads or the building count change), never more often than every few
//! real seconds.

use std::sync::mpsc::{channel, Receiver};

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::overlay::{industry_hue, industry_wash, land_color, land_hue, HeatGrid};
use nd_sim::survey::{self, LandLayer, LandSurvey};
use nd_sim::{IndustryKind, LandscapeParams, World};

use super::{despawn_all, to_mesh, Materials, Overlays};
use crate::sim::Sim;

#[derive(Component)]
pub(super) struct LandLayerTag;

/// Metres the wash floats over the graded ground: enough to clear the
/// terrain mesh between the survey's 16 m corners on gentle slopes.
const WASH_Y: f32 = 1.1;
/// Seconds between rebuilds of a layer whose inputs changed.
const MIN_REBUILD_SECS: f32 = 3.0;

/// The survey half-size: the buildable square and a margin.
fn survey_half(w: &World) -> f32 {
    // [r4-tiles] Over the land bought too.
    if w.landscape.is_grown() { w.landscape.land_half() } else { w.config.half_extent + 300.0 }
}

#[derive(Default)]
pub(super) struct SurveyState {
    /// The landscape and size the survey was built for, and the survey.
    survey: Option<(LandscapeParams, f32, LandSurvey)>,
    /// A survey being built on a worker thread.
    pending: Option<(LandscapeParams, f32, Receiver<LandSurvey>)>,
    /// Industry weights per corner and what they were computed from.
    weights: Option<(WeightsKey, Vec<[f32; nd_sim::IndustryKind::COUNT]>)>,
    /// What is on screen: the layer and the inputs it was drawn from.
    shown: Option<(LandLayer, WeightsKey, u64)>,
    last_build: f32,
}

/// What the industry weights depend on, coarsely: the day (land value and
/// contamination update daily), the roads and the buildings.
type WeightsKey = (u64, u64, usize);

fn weights_key(w: &World) -> WeightsKey {
    (w.clock.day(), w.roads.version, w.buildings.len())
}

/// The survey's corner grid as a heat grid, for its mesh.
fn grid(s: &LandSurvey, values: Vec<f32>) -> HeatGrid {
    HeatGrid { origin: s.origin, cell: s.cell, cols: s.cols, rows: s.rows, values }
}

/// The wash for one layer over the survey.
pub(super) fn land_mesh(w: &World, s: &LandSurvey, layer: LandLayer, weights: &[[f32; nd_sim::IndustryKind::COUNT]]) -> nd_procgen::MeshData {
    let height = |p: nd_sim::Vec2| if w.landscape.is_flat() { 0.0 } else { w.ground_height(p) };
    match layer {
        LandLayer::BestIndustry => {
            // Each corner takes the favourite kind's hue, as solid as its lead.
            let best: Vec<(IndustryKind, f32)> = weights
                .iter()
                .map(|wt| {
                    let sh = survey::shares(wt);
                    let k = nd_sim::industry::favourite(wt);
                    (k, sh[k.index()])
                })
                .collect();
            let g = grid(s, best.iter().map(|b| b.1).collect());
            g.mesh_by(WASH_Y, |k| land_color(industry_hue(best[k].0), industry_wash(best[k].1)), |k| best[k].1 <= 0.0, height)
        }
        LandLayer::Industry(_) => {
            let v: Vec<f32> = w.survey_values(s, layer, Some(weights)).into_iter().map(industry_wash).collect();
            let g = grid(s, v);
            let hue = land_hue(layer);
            g.mesh_by(WASH_Y, |k| land_color(hue, g.values[k]), |k| g.values[k] < 0.04, height)
        }
        // [dirty-industry] Contamination on its own ramp, trace to poison.
        LandLayer::Contamination => {
            let g = grid(s, w.survey_values(s, layer, None));
            g.mesh_by(WASH_Y + 0.05, |k| nd_procgen::overlay::contamination_color(g.values[k]), |k| g.values[k] < 0.02, height)
        }
        // [grid-weather] Wind on its own ramp so the field's pattern reads.
        LandLayer::Wind => {
            let g = grid(s, w.survey_values(s, layer, None));
            g.mesh_by(WASH_Y, |k| nd_procgen::overlay::wind_color(g.values[k]), |k| s.wet[k], height)
        }
        _ => {
            let g = grid(s, w.survey_values(s, layer, None));
            let hue = land_hue(layer);
            let mut m = g.mesh_by(WASH_Y, |k| land_color(hue, g.values[k]), |k| g.values[k] < 0.04, height);
            if layer == LandLayer::Water {
                fishing_grounds(w, &mut m);
            }
            m
        }
    }
}

/// [fishing] The fishing ports' grounds on the water layer: a dashed ring
/// round each, brighter the richer it is, and a dot at its middle.
fn fishing_grounds(w: &World, m: &mut nd_procgen::MeshData) {
    let y = nd_sim::landscape::WATER_LEVEL + WASH_Y + 0.2;
    for f in w.fishing.fleets.values() {
        for g in &f.grounds {
            let a = (0.35 + 0.4 * (g.richness - 0.75) / 0.55).clamp(0.3, 0.8);
            let col = [0.55, 0.95, 1.0, a];
            let r = g.radius + 10.0;
            let n = 36;
            for k in (0..n).step_by(2) {
                let (t0, t1) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
                let (p0, p1) = (g.center + nd_sim::Vec2::from_angle(t0) * r, g.center + nd_sim::Vec2::from_angle(t1) * r);
                m.ground_strip(&[p0, p1], 3.0, y, col);
            }
            m.ground_polygon(&(0..10).map(|k| g.center + nd_sim::Vec2::from_angle(k as f32 * 0.628) * 5.0).collect::<Vec<_>>(), y, col);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_land_layer(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    time: Res<Time>,
    overlays: Res<Overlays>,
    mut st: Local<SurveyState>,
    q: Query<Entity, With<LandLayerTag>>,
) {
    let w = &sim.world;
    let Some(layer) = overlays.land else {
        if st.shown.take().is_some() {
            despawn_all(&mut commands, &q);
        }
        return;
    };
    let params = w.landscape.params.clone();
    let half = survey_half(w);
    // The static survey: current, being built, or to be started.
    let current = st.survey.as_ref().is_some_and(|(p, h, _)| *p == params && *h == half);
    if !current {
        let done = match &st.pending {
            Some((p, h, rx)) if *p == params && *h == half => match rx.try_recv() {
                Ok(s) => Some(s),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                // [fix/review-app] The worker died (it panicked): try again
                // on the next frame rather than wait for ever.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    warn!("the land survey worker stopped; starting it again");
                    st.pending = None;
                    return;
                }
            },
            _ => {
                let (tx, rx) = channel();
                let land = w.landscape.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(LandSurvey::square(&land, half));
                });
                st.pending = Some((params.clone(), half, rx));
                None
            }
        };
        let Some(s) = done else { return };
        st.pending = None;
        st.survey = Some((params, half, s));
        st.weights = None;
        st.shown = None;
    }
    let now = time.elapsed_secs();
    let key = weights_key(w);
    // [fishing] The water layer shows the fishing grounds too.
    let contamination = w.contamination.version ^ w.fishing.key;
    let stale = match st.shown {
        None => true,
        Some((l, k, c)) => l != layer || sim.full_rebuild || (layer.is_industry() && k != key) || (matches!(layer, LandLayer::Farmland | LandLayer::Contamination | LandLayer::Water) && c != contamination),
    };
    let changed_layer = st.shown.is_none_or(|(l, _, _)| l != layer) || sim.full_rebuild;
    if !stale || (!changed_layer && now - st.last_build < MIN_REBUILD_SECS) {
        return;
    }
    let st = &mut *st;
    let Some((_, _, s)) = st.survey.as_ref() else { return };
    if layer.is_industry() && st.weights.as_ref().is_none_or(|(k, _)| *k != key || sim.full_rebuild) {
        st.weights = Some((key, w.survey_industry_weights(s)));
    }
    let empty = Vec::new();
    let weights = st.weights.as_ref().map_or(&empty, |(_, v)| v);
    let m = land_mesh(w, s, layer, weights);
    despawn_all(&mut commands, &q);
    if !m.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, LandLayerTag));
    }
    st.shown = Some((layer, key, contamination));
    st.last_build = now;
}
