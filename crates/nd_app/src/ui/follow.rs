//! [camera] Following and zooming in the HUD: the Follow and Zoom to
//! buttons the inspector, the person pane and the favourites share, the
//! gift button, and the banner (with a gold frame round the map) that says
//! the camera is locked on someone and how to let go.

use bevy_egui::egui;
use nd_sim::CitizenId;

use super::{GOLD, IVORY};
use crate::camera::{chase_subject, thing_pose, zoom_distance, CameraFly, CameraFollow, CameraMode, FlyGoal, FollowView};
use crate::sim::Sim;
use crate::tools::Selection;

/// What the banner and the buttons call a thing.
pub fn label(sim: &Sim, sel: Selection) -> String {
    let w = &sim.world;
    match sel {
        Selection::None => String::new(),
        Selection::Toll(id) => w.tolls.get(id).map_or_else(|| "a toll booth".into(), |b| b.name.clone()),
        Selection::Road(sid) => crate::road_select::name(w, sid),
        Selection::Citizen(id) => w.citizens.get(id).map_or_else(|| "someone".into(), |c| c.name.clone()),
        Selection::Car(id) => w.car_label(id),
        Selection::Unit(id) => crate::fire_scene::unit_title(w, id),
        Selection::Ship(id) => crate::ferry_select::ship_title(w, id),
        Selection::Drone(id) => crate::air_select::title(w, id),
        Selection::Rv(seed) => crate::rv_select::title(w, seed),
        Selection::Parked(id) => w.vehicles.cars.get(id).and_then(|v| w.citizens.get(v.owner)).map_or_else(|| "a parked car".into(), |c| format!("{}'s parked car", c.name)),
        Selection::Building(id) => w.buildings.get(id).map_or_else(|| "a building".into(), |b| b.name.clone()),
        Selection::Bot(id) => nd_sim::bots::bot_name(w, id),
        Selection::Train(id) => w.rail.trains.get(id).map_or_else(|| "a train".into(), |t| format!("train {}", crate::train_select::headcode(id, t.kind))),
        Selection::Metro(id) => w.subway.trains.get(id).and_then(|t| w.subway.lines.get(t.line)).map_or_else(|| "a metro train".into(), |l| format!("{} train {}", l.name, crate::train_select::metro_code(id))),
    }
}

/// Start following `sel`, flying in smoothly first.
pub fn start_follow(sim: &Sim, sel: Selection, follow: &mut CameraFollow, fly: &mut CameraFly, cam_distance: Option<f32>) {
    follow.start(sel, sim.generation);
    let d = cam_distance.unwrap_or(90.0).min(zoom_distance(sim, sel) * 2.0);
    fly.to(FlyGoal::Thing(sel), Some(d), None);
}

/// A smooth move onto `sel`; a follow of anything else ends.
pub fn zoom_to(sim: &Sim, sel: Selection, follow: &mut CameraFollow, fly: &mut CameraFly, mode: &mut CameraMode) {
    if !follow.is(sel) {
        follow.stop();
    }
    *mode = CameraMode::Orbit;
    fly.to(FlyGoal::Thing(sel), Some(zoom_distance(sim, sel)), Some(0.62));
}

/// The camera row: Follow (or Stop following) and Zoom to. A building
/// cannot be followed, only zoomed to.
pub fn camera_buttons(ui: &mut egui::Ui, sim: &Sim, sel: Selection, follow: &mut CameraFollow, fly: &mut CameraFly, mode: &mut CameraMode) {
    if sel == Selection::None || thing_pose(sim, sel).is_none() {
        return;
    }
    let movable = !matches!(sel, Selection::Building(_) | Selection::Toll(_) | Selection::Road(_));
    ui.horizontal(|ui| {
        if movable {
            if follow.is(sel) {
                if ui.add(egui::Button::new(egui::RichText::new("Following - stop").color(egui::Color32::BLACK).strong()).fill(GOLD)).on_hover_text("The camera is locked on them. Click or press Esc to let go.").clicked() {
                    follow.stop();
                    if *mode == CameraMode::Walk {
                        *mode = CameraMode::Orbit;
                    }
                }
            } else if ui.button("Follow").on_hover_text("Keep the camera on them wherever they go: low behind them, looking where they go, with whatever is in the way cut away (right-drag looks round, the wheel zooms). F: the high view. Tab: their point of view.").clicked() {
                start_follow(sim, sel, follow, fly, None);
            }
        }
        if ui.button("Zoom to").on_hover_text("Fly the camera smoothly to this").clicked() {
            zoom_to(sim, sel, follow, fly, mode);
        }
    });
}

/// "Gift $1000": money from the treasury to their household. Returns the
/// toast to show.
pub fn gift_button(ui: &mut egui::Ui, sim: &mut Sim, id: CitizenId, small: bool) -> Option<String> {
    let amount = nd_sim::GIFT_AMOUNT;
    let can = sim.world.economy.treasury >= amount;
    let text = format!("Gift ${amount:.0}");
    let button = if small { egui::Button::new(egui::RichText::new("$").strong()).small() } else { egui::Button::new(text) };
    let hover = if can { format!("Give ${amount:.0} from the city treasury to their household") } else { format!("The treasury has less than ${amount:.0}") };
    if !ui.add_enabled(can, button).on_hover_text(hover).on_disabled_hover_text("The treasury cannot spare it").clicked() {
        return None;
    }
    let name = sim.world.citizens.get(id).map(|c| c.name.clone()).unwrap_or_default();
    Some(match sim.world.gift(id, amount) {
        Ok(g) => format!("Gave ${:.0} to {name}'s {}. Treasury {:.0}.", g.amount, if g.household.is_some() { "household" } else { "savings" }, g.treasury),
        Err(e) => format!("No gift: {e}."),
    })
}

/// While the camera follows something: a gold frame round the map and a
/// banner at its top naming who, saying the camera is locked, with their
/// point of view and Stop following (Esc).
pub fn banner(ctx: &egui::Context, sim: &Sim, follow: &mut CameraFollow, fly: &mut CameraFly, mode: &mut CameraMode, selection: &mut Selection, content: egui::Rect) {
    let Some(sel) = follow.target else { return };
    let pose = thing_pose(sim, sel);
    let pov_ok = pose.is_some_and(|p| p.heading.is_some());
    let chase_ok = chase_subject(sim, sel).is_some();
    let pulse = ((ctx.input(|i| i.time) * 2.0).sin() * 0.5 + 0.5) as f32;
    ctx.request_repaint();
    // The frame: the whole map is held.
    let frame_col = GOLD.gamma_multiply(0.55 + 0.35 * pulse);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("follow_frame")));
    painter.rect_stroke(content.shrink(3.0), 0.0, egui::Stroke::new(3.0, frame_col), egui::StrokeKind::Inside);
    // Corner brackets, like a camera's viewfinder.
    let r = content.shrink(10.0);
    let k = 28.0;
    for (c, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        painter.line_segment([c, c + egui::vec2(k * dx, 0.0)], egui::Stroke::new(3.0, GOLD));
        painter.line_segment([c, c + egui::vec2(0.0, k * dy)], egui::Stroke::new(3.0, GOLD));
    }
    let who = label(sim, sel);
    let pov = *mode == CameraMode::Walk;
    egui::Area::new(egui::Id::new("follow_banner"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(content.center().x, content.top() + 10.0))
        .pivot(egui::Align2::CENTER_TOP)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgb(14, 12, 6))
                .stroke(egui::Stroke::new(2.0, GOLD))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("CAMERA LOCKED").color(GOLD).strong());
                        ui.label(egui::RichText::new(format!("{} {who}", if pov { "seeing through the eyes of" } else { "following" })).color(IVORY));
                        ui.separator();
                        if pov_ok {
                            let (text, hover) = if pov { ("Map view", "Back to the follow camera (Tab)") } else { ("Their view", "See what they see (Tab). Right-drag looks around.") };
                            if ui.button(text).on_hover_text(hover).clicked() {
                                *mode = if pov { CameraMode::Orbit } else { CameraMode::Walk };
                            }
                        }
                        // [chase-cam] Behind them, or the high orbit.
                        if chase_ok && !pov {
                            let (text, hover, next) = match follow.view {
                                FollowView::Chase => ("High view", "The orbit camera high above them, still following (F)", FollowView::High),
                                FollowView::High => ("Chase view", "Low behind them, looking where they go (F)", FollowView::Chase),
                            };
                            if ui.button(text).on_hover_text(hover).clicked() {
                                follow.set_view(next);
                                if next == FollowView::High {
                                    fly.to(FlyGoal::Thing(sel), Some(zoom_distance(sim, sel) * 1.3), Some(0.62));
                                }
                            }
                        }
                        if *selection != sel && ui.button("Select").on_hover_text("Open them in the inspector").clicked() {
                            *selection = sel;
                        }
                        if ui.add(egui::Button::new(egui::RichText::new("Stop following (Esc)").color(egui::Color32::BLACK).strong()).fill(GOLD)).clicked() {
                            follow.stop();
                            if *mode == CameraMode::Walk {
                                *mode = CameraMode::Orbit;
                            }
                        }
                    });
                    let hint = if pov {
                        "Their eyes lead; right-drag to look around."
                    } else if chase_ok && follow.view == FollowView::Chase {
                        "Right-drag or Q/E looks round them (it swings back), the wheel zooms. F: high view."
                    } else {
                        "Pan is locked to them; rotate and zoom still work."
                    };
                    ui.label(egui::RichText::new(hint).small().weak());
                });
        });
}
