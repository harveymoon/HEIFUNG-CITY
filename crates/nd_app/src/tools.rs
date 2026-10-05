//! Player tools: draw roads, zone polygons, inspect things.

use bevy::prelude::*;
use nd_sim::zoning::point_in_polygon;
use nd_sim::{BuildingId, CitizenId, LandUse, Vec2 as SVec2};

use crate::camera::Cursor;
use crate::sim::{Sim, SimSet};
use crate::ui::UiFlags;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Inspect,
    Road,
    Zone,
}

#[derive(Resource)]
pub struct ToolState {
    pub tool: Tool,
    pub lanes: u8,
    pub land_use: LandUse,
    pub points: Vec<SVec2>,
    pub status: String,
}

impl Default for ToolState {
    fn default() -> Self {
        Self { tool: Tool::Road, lanes: 2, land_use: LandUse::Residential, points: Vec::new(), status: String::new() }
    }
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Citizen(CitizenId),
    Building(BuildingId),
}

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolState>()
            .init_resource::<Selection>()
            .configure_sets(Update, (SimSet::Input, SimSet::Step, SimSet::Sync).chain())
            .add_systems(Update, (tool_input, draw_preview).chain().in_set(SimSet::Input));
    }
}

fn tool_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    flags: Res<UiFlags>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut sim: ResMut<Sim>,
) {
    // Hotkeys.
    if keys.just_pressed(KeyCode::Digit1) || keys.just_pressed(KeyCode::KeyI) {
        tools.tool = Tool::Inspect;
        tools.points.clear();
    }
    if keys.just_pressed(KeyCode::Digit2) || keys.just_pressed(KeyCode::KeyR) {
        tools.tool = Tool::Road;
        tools.points.clear();
    }
    if keys.just_pressed(KeyCode::Digit3) || keys.just_pressed(KeyCode::KeyZ) {
        tools.tool = Tool::Zone;
        tools.points.clear();
    }
    if keys.just_pressed(KeyCode::Escape) {
        tools.points.clear();
        *selection = Selection::None;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        tools.points.pop();
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let finish = keys.just_pressed(KeyCode::Enter) || buttons.just_pressed(MouseButton::Right);
    let click = buttons.just_pressed(MouseButton::Left) && !shift && !flags.pointer_over_ui;

    match tools.tool {
        Tool::Road => {
            if click {
                if let Some(p) = cursor.ground {
                    // Snap to an existing node so drawn roads connect cleanly.
                    let p = sim
                        .world
                        .roads
                        .node_near(p, 8.0)
                        .map(|n| sim.world.roads.node(n).pos)
                        .unwrap_or(p);
                    tools.points.push(p);
                }
            }
            if finish {
                if tools.points.len() >= 2 {
                    let pts = tools.points.clone();
                    let lanes = tools.lanes;
                    sim.world.add_road(&pts, lanes);
                    tools.status = format!("Road built with {} nodes", pts.len());
                }
                tools.points.clear();
            }
        }
        Tool::Zone => {
            if click {
                if let Some(p) = cursor.ground {
                    tools.points.push(p);
                }
            }
            if finish {
                if tools.points.len() >= 3 {
                    let poly = tools.points.clone();
                    let lu = tools.land_use;
                    let before = sim.world.lots.len();
                    sim.world.add_zone(poly, lu);
                    let added = sim.world.lots.len() - before;
                    tools.status = if added == 0 {
                        "Zone added but no lots: zones need a road running through them".to_string()
                    } else {
                        format!("{} zone: {added} lots", lu.label())
                    };
                }
                tools.points.clear();
            }
        }
        Tool::Inspect => {
            if click {
                if let Some(p) = cursor.ground {
                    *selection = pick(&sim, p);
                }
            }
        }
    }
}

fn pick(sim: &Sim, p: SVec2) -> Selection {
    let w = &sim.world;
    let mut best: Option<(f32, CitizenId)> = None;
    for (id, c) in w.citizens.iter() {
        if !c.is_visible() {
            continue;
        }
        let d = c.pos.distance(p);
        if d < 7.0 && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, id));
        }
    }
    if let Some((_, id)) = best {
        return Selection::Citizen(id);
    }
    for (_, lot) in w.lots.iter() {
        if let Some(b) = lot.building {
            if point_in_polygon(p, &lot.corners) {
                return Selection::Building(b);
            }
        }
    }
    Selection::None
}

fn draw_preview(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>) {
    let color = Color::linear_rgb(0.0, 2.5, 3.0);
    let lift = |p: SVec2| Vec3::new(p.x, 0.6, p.y);
    match tools.tool {
        Tool::Road | Tool::Zone => {
            let mut pts: Vec<Vec3> = tools.points.iter().map(|&p| lift(p)).collect();
            if let Some(c) = cursor.ground {
                let c = if tools.tool == Tool::Road {
                    sim.world.roads.node_near(c, 8.0).map(|n| sim.world.roads.node(n).pos).unwrap_or(c)
                } else {
                    c
                };
                gizmos.circle(Isometry3d::new(lift(c), Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), 3.0, color);
                pts.push(lift(c));
            }
            if pts.len() >= 2 {
                if tools.tool == Tool::Zone && pts.len() >= 3 {
                    pts.push(pts[0]);
                }
                gizmos.linestrip(pts, color);
            }
        }
        Tool::Inspect => {}
    }
}
