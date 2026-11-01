//! [r4-tiles] Buying land: with the Land for sale layer on, every tile
//! round the city carries a label on the map (its price, or "yours"), and
//! the Land for sale window lists the tiles for sale with what each holds
//! (ground, water, relief, soil and oil, the ways through it) and buys the
//! one picked. The outlines are `render::land_tiles`.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_sim::landscape::{tile_center, tile_direction, tile_rect, Tile, GROWN_HALF, TILE};
use nd_sim::world::land_tiles::LandTile;

use crate::camera::{CameraFly, CameraMode, FlyGoal, OrbitCamera};
use crate::render::land_tiles::{PickedTile, FOR_SALE, LATER, OWNED};
use crate::render::Overlays;
use crate::sim::Sim;

fn rgb(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from(egui::Rgba::from_rgb(c[0], c[1], c[2]))
}

/// What the last purchase said (shown in the window until another).
#[derive(Default, Clone)]
pub struct Said(String);

fn money(v: f32) -> String {
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-${out}") } else { format!("${out}") }
}

#[allow(clippy::too_many_arguments)]
pub fn land_window(
    mut contexts: EguiContexts,
    mut sim: ResMut<Sim>,
    mut overlays: ResMut<Overlays>,
    mut picked: ResMut<PickedTile>,
    mode: Res<CameraMode>,
    mut fly: ResMut<CameraFly>,
    cams: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    mut flags: ResMut<super::UiFlags>,
    mut said: Local<Said>,
) -> Result {
    if !overlays.land_tiles || mode.street() || sim.world.landscape.is_flat() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let tiles = sim.world.land_tiles();
    let treasury = sim.world.economy.treasury;
    // The labels on the map.
    if let Some((camera, gt)) = cams.iter().next() {
        let w = &sim.world;
        for t in &tiles {
            let c = tile_center(t.tile());
            let top = Vec3::new(c.x, w.ground_height(c).max(0.0) + 40.0, c.y);
            let Ok(at) = camera.world_to_viewport(gt, top) else { continue };
            let pos = crate::ui::theme::to_points(ctx, at);
            if !ctx.content_rect().contains(pos) {
                continue;
            }
            let (col, text) = if t.owned {
                (rgb(OWNED), "YOURS".to_string())
            } else if t.buyable {
                (rgb(FOR_SALE), money(t.price))
            } else {
                (rgb(LATER), format!("later · {}", money(t.price)))
            };
            let on = picked.0 == Some(t.tile());
            egui::Area::new(egui::Id::new(("land_tile_label", t.tile())))
                .order(egui::Order::Background)
                .fixed_pos(pos)
                .pivot(egui::Align2::CENTER_CENTER)
                .show(ctx, |ui| {
                    let frame = egui::Frame::new().fill(egui::Color32::from_black_alpha(if on { 230 } else { 170 })).stroke(egui::Stroke::new(if on { 2.0 } else { 1.0 }, col)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 4));
                    let resp = frame
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new(text).strong().color(col));
                                if !t.owned {
                                    ui.label(egui::RichText::new(tile_direction(t.tile())).small().color(egui::Color32::from_gray(190)));
                                }
                            });
                        })
                        .response;
                    flags.panel_rects.push(resp.rect);
                    let resp = resp.interact(egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.on_hover_text(t.summary()).clicked() && !t.owned {
                        picked.0 = Some(t.tile());
                    }
                });
        }
    }
    // The window.
    let mut open = true;
    let mut buy: Option<Tile> = None;
    let mut go: Option<Tile> = None;
    egui::Window::new("Land for sale").id(egui::Id::new("land_for_sale")).open(&mut open).default_width(360.0).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 120.0)).resizable(false).show(ctx, |ui| {
        let bought = sim.world.landscape.tiles.len();
        ui.label(egui::RichText::new(format!("Treasury {}", money(treasury))).strong());
        ui.label(
            egui::RichText::new(format!(
                "The land round the city in tiles of {:.1} km. {bought} of {} bought; land can be bought out to {:.0} km from the centre, each tile beside land the city has. Each costs more than the last.",
                TILE / 1000.0,
                tiles.len(),
                GROWN_HALF / 1000.0
            ))
            .small()
            .weak(),
        );
        ui.separator();
        let pick = picked.0.and_then(|p| tiles.iter().find(|t| t.tile() == p && !t.owned));
        match pick {
            Some(t) => detail(ui, t, treasury, &mut buy, &mut go),
            None => {
                ui.label(egui::RichText::new("Pick a tile below, or click its price on the map.").small());
            }
        }
        if !said.0.is_empty() {
            ui.label(egui::RichText::new(&said.0).small().color(egui::Color32::from_rgb(255, 210, 140)));
        }
        ui.separator();
        let mut sale: Vec<&LandTile> = tiles.iter().filter(|t| t.buyable).collect();
        sale.sort_by(|a, b| a.price.total_cmp(&b.price));
        if sale.is_empty() {
            ui.label(egui::RichText::new("Every tile is bought.").small());
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for t in sale {
                let on = picked.0 == Some(t.tile());
                let text = format!("{:<11} {:>10}   {}", tile_direction(t.tile()), money(t.price), short(t));
                if ui.add(egui::Button::selectable(on, egui::RichText::new(text).small().color(if t.price <= treasury { rgb(FOR_SALE) } else { egui::Color32::from_gray(150) }))).on_hover_text(t.summary()).clicked() {
                    picked.0 = Some(t.tile());
                }
            }
        });
    });
    if !open {
        overlays.land_tiles = false;
        picked.0 = None;
    }
    if let Some(t) = go {
        let c = tile_center(t);
        fly.to(FlyGoal::Point(Vec2::new(c.x, c.y)), Some(1700.0), Some(0.95));
    }
    if let Some(t) = buy {
        said.0 = match sim.world.buy_tile(t) {
            Ok(price) => {
                picked.0 = None;
                format!("Bought the {} tile for {}: it is the city's to build on.", tile_direction(t), money(price))
            }
            Err(why) => format!("Not bought: {why}."),
        };
    }
    Ok(())
}

/// A few words for the list.
fn short(t: &LandTile) -> String {
    let g = &t.ground;
    let mut bits = Vec::new();
    if g.sea > 0.5 {
        bits.push("mostly sea");
    } else if g.sea > 0.05 {
        bits.push("coast");
    }
    if g.river_crosses {
        bits.push("river");
    }
    if g.high - g.low > 40.0 || g.slope > 0.12 {
        bits.push("hilly");
    } else if g.land > 0.5 {
        bits.push("level");
    }
    if g.fertility > 0.45 {
        bits.push("fertile");
    }
    if g.oil > 0.05 {
        bits.push("oil");
    }
    if t.highway {
        bits.push("highway");
    }
    if t.rail {
        bits.push("rail");
    }
    bits.join(", ")
}

fn detail(ui: &mut egui::Ui, t: &LandTile, treasury: f32, buy: &mut Option<Tile>, go: &mut Option<Tile>) {
    let g = &t.ground;
    let (lo, hi) = tile_rect(t.tile());
    ui.label(egui::RichText::new(format!("The {} tile", tile_direction(t.tile()))).heading().color(rgb(FOR_SALE)));
    ui.label(egui::RichText::new(format!("x {:.1} to {:.1} km, y {:.1} to {:.1} km", lo.x / 1000.0, hi.x / 1000.0, lo.y / 1000.0, hi.y / 1000.0)).small().weak());
    let pc = |x: f32| format!("{:.0}%", x * 100.0);
    egui::Grid::new("land_tile_detail").num_columns(2).spacing(egui::vec2(12.0, 2.0)).show(ui, |ui| {
        let mut row = |k: &str, v: String| {
            ui.label(egui::RichText::new(k).small().weak());
            ui.label(egui::RichText::new(v).small());
            ui.end_row();
        };
        row("land", format!("{} dry, {} buildable ({} level)", pc(g.land), pc(g.buildable), pc(g.level)));
        row("water", {
            let mut v = Vec::new();
            if g.sea > 0.005 {
                v.push(format!("{} sea", pc(g.sea)));
            }
            if g.river > 0.005 || g.river_crosses || g.stream_crosses {
                v.push(format!("{} river{}", pc(g.river), if g.river_crosses { " (a river crosses)" } else if g.stream_crosses { " (a stream crosses)" } else { "" }));
            }
            if g.lake > 0.005 {
                v.push(format!("{} lake", pc(g.lake)));
            }
            if v.is_empty() { "none".into() } else { v.join(", ") }
        });
        if g.land > 0.0 {
            row("relief", format!("{:.0} to {:.0} m (mean {:.0}), slope {:.0}%", g.low, g.high, g.mean, g.slope * 100.0));
            row("soil", format!("fertility {:.0}%, rock {:.0}%", g.fertility * 100.0, g.rock * 100.0));
            row("resources", {
                let mut v = Vec::new();
                if g.oil > 0.0 {
                    v.push(format!("oil under {}", pc(g.oil)));
                }
                if g.hot_spots > 0 {
                    v.push(format!("{} geothermal hot spot{}", g.hot_spots, if g.hot_spots == 1 { "" } else { "s" }));
                }
                if v.is_empty() { "none to speak of".into() } else { v.join(", ") }
            });
        }
        let mut ways = Vec::new();
        if t.highway {
            ways.push("the highway");
        }
        if t.rail {
            ways.push("the railway");
        }
        let roads = if t.roads > 0 { format!("{} road piece{}", t.roads, if t.roads == 1 { "" } else { "s" }) } else { String::new() };
        let mut v = ways.join(" and ");
        if !roads.is_empty() {
            if !v.is_empty() {
                v.push_str(", ");
            }
            v.push_str(&roads);
        }
        row("runs through", if v.is_empty() { "nothing yet".into() } else { v });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let can = t.price <= treasury;
        let b = egui::Button::new(egui::RichText::new(format!("Buy for {}", money(t.price))).strong());
        let resp = ui.add_enabled(can, b);
        let resp = if can { resp } else { resp.on_disabled_hover_text(format!("the treasury has {}", money(treasury))) };
        if resp.clicked() {
            *buy = Some(t.tile());
        }
        if ui.button("Show").clicked() {
            *go = Some(t.tile());
        }
    });
}
