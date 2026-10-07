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
use crate::ui::UiFlags;

#[derive(Component)]
pub(super) struct LandLayerTag;

/// Metres the wash floats over the graded ground: enough to clear the
/// terrain mesh between the survey's 16 m corners on gentle slopes.
const WASH_Y: f32 = 1.1;
/// Seconds between rebuilds of a layer whose inputs changed.
const MIN_REBUILD_SECS: f32 = 3.0;

/// The survey half-size: the buildable square and a margin.
fn survey_half(w: &World) -> f32 {
    w.config.half_extent + 300.0
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
        _ => {
            let g = grid(s, w.survey_values(s, layer, None));
            let hue = land_hue(layer);
            g.mesh_by(WASH_Y, |k| land_color(hue, g.values[k]), |k| g.values[k] < 0.04, height)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_land_layer(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    keys: Res<ButtonInput<KeyCode>>,
    flags: Res<UiFlags>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    time: Res<Time>,
    mut overlays: ResMut<Overlays>,
    mut st: Local<SurveyState>,
    q: Query<Entity, With<LandLayerTag>>,
) {
    if keys.just_pressed(KeyCode::KeyK) && !flags.keyboard_captured {
        overlays.cycle_land();
    }
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
            Some((p, h, rx)) if *p == params && *h == half => rx.try_recv().ok(),
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
    let contamination = w.contamination.version;
    let stale = match st.shown {
        None => true,
        Some((l, k, c)) => l != layer || sim.full_rebuild || (layer.is_industry() && k != key) || (layer == LandLayer::Farmland && c != contamination),
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
