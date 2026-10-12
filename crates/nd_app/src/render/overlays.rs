//! The map overlay toggles, the traffic overlay and the path layer.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Data layers drawn over the ground, one at a time (`crate::map_layers`
/// keeps them exclusive and lists them for the UI and the API).
#[derive(Resource, Clone)]
pub struct Overlays {
    pub land_value: bool,
    /// Pedestrian network: lot loops, alleys and gates.
    pub paths: bool,
    /// Zone outline glows and vacant-lot outlines. The ground fills stay.
    pub zoning: bool,
    /// Draw the parked (inactive) projects as dim holograms.
    pub projects: bool,
    /// Load per lane: fullness and speed, green to red.
    pub traffic: bool,
    /// A coloured disc under every vehicle by type.
    pub traffic_types: bool,
    /// Where people are right now, as a heat map.
    pub people: bool,
    /// Pollution and noise from plants, industry, nightlife and traffic.
    pub pollution: bool,
    /// The land survey layer shown, if any: a resource (farmland, oil,
    /// geothermal, ...) or where each industry would do best. One at a time:
    /// two washes over each other would read as neither.
    pub land: Option<nd_sim::survey::LandLayer>,
    /// [districts] District areas, borders and names.
    pub districts: bool,
    /// [layers] Crime: a heat map of offences, the police cars lit up over
    /// a dimmed city and the routes they are driving (`layers`, `focus`).
    pub crime: bool,
    /// [layers] Map view: buildings, people and traffic hidden, the roads
    /// drawn flat like a street map with their names, the zones tinted and
    /// pins on the special buildings (`mapview`).
    pub map: bool,
    /// [clarity] Traffic danger: crashes on the record per street segment,
    /// drawn over everything (`layers::load_gizmos`).
    pub danger: bool,
    /// [events] Road condition: every street piece green to red by its
    /// wear (`nights::sync_road_layer`).
    pub road_condition: bool,
    /// [metro-ux] Metro: the city dimmed, the metro's lines in their
    /// colours over it and every station a big named marker
    /// (`metro_ux`, `focus::Focus::Metro`). The metro line tool shows it.
    pub metro: bool,
    /// [pt-ux] Parking: streets by the legal parking a driver finds there,
    /// public lots by how full they are (`pt_layers`).
    pub parking: bool,
    /// [pt-ux] Trails: a long exposure of everything moving over a dark
    /// city (`pt_layers`, `focus::Focus::Trails`).
    pub trails: bool,
    /// [pt-ux] Land density: how densely each plot is built (`pt_layers`).
    pub density: bool,
    /// [w1] Construction: building sites going up and coming down, over a
    /// dimmed city (`sites_layer`, `focus::Focus::Sites`).
    pub construction: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self { land_value: false, paths: false, zoning: true, projects: true, traffic: false, traffic_types: false, people: false, pollution: false, land: None, districts: false, crime: false, map: false, danger: false, road_condition: false, metro: false, parking: false, trails: false, density: false, construction: false }
    }
}

impl Overlays {
    /// The heat-map style layers the L key steps through, one at a time.
    pub const CYCLE: [&'static str; 7] = ["land_value", "traffic", "traffic_types", "people", "pollution", "crime", "map"];

    /// A layer's flag by API name (`crate::map_layers::MAP_LAYERS`), or the
    /// "show other projects" switch, which is not a layer.
    pub fn flag(&mut self, layer: &str) -> Option<&mut bool> {
        if layer == "projects" {
            return Some(&mut self.projects);
        }
        let entry = crate::map_layers::MAP_LAYERS.iter().find(|l| l.key == layer)?;
        Some((entry.flag)(self))
    }

    pub fn get(&self, layer: &str) -> Option<bool> {
        self.clone().flag(layer).map(|f| *f)
    }

    /// Step to the next land survey layer (K); after the last, none.
    pub fn cycle_land(&mut self) {
        use nd_sim::survey::LandLayer;
        let next = match self.land {
            None => 0,
            Some(l) => LandLayer::ALL.iter().position(|x| *x == l).map_or(0, |i| i + 1),
        };
        self.land = LandLayer::ALL.get(next).copied();
    }

    /// Step to the next of the [`Overlays::CYCLE`] layers, alone; after the
    /// last, none.
    pub fn cycle(&mut self) {
        let current = Self::CYCLE.iter().rposition(|l| self.get(l) == Some(true));
        for l in Self::CYCLE {
            if let Some(f) = self.flag(l) {
                *f = false;
            }
        }
        let next = current.map_or(0, |i| i + 1);
        if let Some(l) = Self::CYCLE.get(next) {
            if let Some(f) = self.flag(l) {
                *f = true;
            }
        }
    }
}

/// Land value heat map over the ground. Toggled with V.
pub(super) fn sync_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<OverlayTag>>,
) {
    let lv = &sim.world.land_value;
    if overlays.land_value == state.overlay_shown && lv.version == state.overlay_version && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.overlay_shown = overlays.land_value;
    state.overlay_version = lv.version;
    despawn_all(&mut commands, &existing);
    if !overlays.land_value {
        return;
    }
    let pal = &assets.palette;
    let mut mesh = MeshData::default();
    let half = lv.cell * 0.5;
    for iy in 0..lv.n {
        for ix in 0..lv.n {
            let v = lv.values[iy * lv.n + ix];
            let c = lv.cell_center(ix, iy);
            let t = ((v - 0.15) / 0.7).clamp(0.0, 1.0);
            let col = lerp3(pal.overlay_low, pal.overlay_high, t);
            let poly = [SVec2::new(c.x - half, c.y - half), SVec2::new(c.x + half, c.y - half), SVec2::new(c.x + half, c.y + half), SVec2::new(c.x - half, c.y + half)];
            mesh.ground_polygon(&poly, 0.3, [col[0], col[1], col[2], 0.42]);
        }
    }
    spawn_overlay(&mut commands, &mut meshes, &mats, &mesh, OverlayTag);
}

/// The pedestrian network inside blocks: loops, alleys and gates. Toggled with P.
pub(super) fn sync_paths(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<PathsTag>>,
) {
    let walk = &sim.world.walk;
    if overlays.paths == state.paths_shown && walk.version == state.paths_version && !sim.full_rebuild {
        return;
    }
    state.paths_shown = overlays.paths;
    state.paths_version = walk.version;
    despawn_all(&mut commands, &existing);
    if !overlays.paths {
        return;
    }
    let pal = &assets.palette;
    let mut mesh = MeshData::default();
    // [pt-ux] Every walkway, not only the ones inside blocks: pavements
    // faint teal, crossings white, park paths green, lot loops, alleys and
    // gates in the preview colour (brightest), footbridges and raised
    // walks violet. People glow over a dimmed city meanwhile
    // (`focus::Focus::Walk`).
    for (i, edges) in walk.adj.iter().enumerate() {
        for e in edges {
            if (i as u32) >= e.to {
                continue;
            }
            let (Some(a), Some(b)) = (walk.nodes.get(i), walk.nodes.get(e.to as usize)) else { continue };
            let (col, width) = path_style(e, pal.preview);
            mesh.ground_strip(&[a.pos, b.pos], width, 0.25, col);
        }
    }
    if !mesh.is_empty() {
        let data = mesh;
        commands.spawn((Mesh3d(meshes.add(to_mesh(&data, false))), MeshMaterial3d(mats.glow()), Transform::IDENTITY, bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver, PathsTag, super::focus::FocusKeep));
    }
}

/// [pt-ux] A walkway's colour and width in the Pedestrian network layer.
pub(super) fn path_style(e: &nd_sim::walk::WalkEdge, preview: [f32; 3]) -> ([f32; 4], f32) {
    if e.interior || e.gate {
        (scale(preview, 0.9), 0.8)
    } else if e.raised {
        ([0.55, 0.3, 1.1, 1.0], 0.8)
    } else if e.cross != nd_sim::walk::NO_CROSSING {
        ([0.9, 0.9, 0.95, 1.0], 0.7)
    } else if e.park {
        ([0.25, 0.9, 0.35, 1.0], 0.7)
    } else {
        ([0.12, 0.45, 0.55, 1.0], 0.5)
    }
}
