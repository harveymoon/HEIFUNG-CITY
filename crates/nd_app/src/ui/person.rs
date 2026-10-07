//! The citizen read-out shared by the inspector and the person pane, and the
//! person pane itself.

use bevy_egui::egui;
use nd_minds::Personality;
use nd_sim::citizens::TravelMode;
use nd_sim::{Activity, Citizen, CitizenId, World};

use super::hint;
use crate::camera::{CameraFly, CameraFollow, CameraMode};
use crate::sim::Sim;
use crate::tools::{Selection, ToolState};

/// Who they are, what they are doing and where they are going, their means
/// and their current thought: the block both the inspector and the person
/// pane open with.
pub fn citizen_summary(ui: &mut egui::Ui, w: &World, c: &Citizen) {
    let name_of = |b: Option<nd_sim::BuildingId>| b.and_then(|b| w.buildings.get(b)).map(|b| b.name.as_str());
    let home = name_of(c.home).unwrap_or("none");
    let work = name_of(c.work).unwrap_or("unemployed");
    let via_transit = name_of(c.transit.map(|t| t.target));
    let destination = match &c.activity {
        Activity::Traveling { to, .. } | Activity::Driving { to, .. } => name_of(*to).or(via_transit),
        Activity::Waiting { .. } | Activity::Riding { .. } | Activity::OnTrain { .. } => via_transit,
        _ => None,
    };
    let rent = c.home.and_then(|h| w.buildings.get(h)).map_or(0.0, |b| b.rent);
    let how = if c.mode == TravelMode::Bike && c.activity.is_traveling() { "  ·  by bike" } else { "" };

    ui.label(egui::RichText::new(Personality::from_seed(c.seed).summary()).small());
    look_line(ui, c);
    let _ = (destination, how);
    ui.label(format!("home: {home}  ·  work: {work}"));
    // [economy] the art scene's people
    if nd_sim::world::is_artist(c) {
        let studio = c.work.and_then(|b| w.buildings.get(b)).and_then(|b| b.business.as_ref()).is_some_and(|z| matches!(z.kind, nd_sim::BusinessKind::ArtStudio | nd_sim::BusinessKind::Gallery));
        ui.label(egui::RichText::new(if studio { "Artist, working in the art scene" } else { "Artist, painting after a day job" }).small().color(egui::Color32::from_rgb(220, 150, 255)));
    }
    ui.label(format!(
        "mood {:.0}%  ·  satiety {:.0}%  ·  {} class  ·  arrived day {}",
        c.mood * 100.0,
        c.satiety * 100.0,
        c.class().label(),
        c.arrived_tick / 1440 + 1
    ));
    ui.label(format!(
        "income {:.0}/day  ·  rent {:.0}/day  ·  savings {:.0}{}",
        c.income,
        rent,
        c.savings,
        if c.rent_stress_days > 0 { format!("  ·  rent stress {}d", c.rent_stress_days) } else { String::new() }
    ));
    ui.label(format!(
        "meals at home {:.0}{}  ·  {}  ·  {}",
        c.groceries,
        if c.hungry_days > 0 { format!("  ·  hungry {}d", c.hungry_days) } else { String::new() },
        if c.owns_car { "owns a car" } else { "no car" },
        if c.owns_bike { "owns a bike" } else { "no bike" }
    ));
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("“{}”", c.thought)).italics().color(egui::Color32::from_rgb(220, 190, 140)));
}

/// [clarity] What they are doing, large: "Going home to Habitat 21F", the
/// place a button that selects it, and where they set off from. Returns
/// the building picked.
pub fn activity_heading(ui: &mut egui::Ui, w: &World, id: CitizenId) -> Option<nd_sim::BuildingId> {
    let line = w.activity_line(id);
    if line.is_empty() {
        return None;
    }
    let mut pick = None;
    let col = egui::Color32::from_rgb(120, 220, 255);
    ui.label(egui::RichText::new(capital(&line)).size(16.0).color(col).strong());
    let from = w.citizen_origin(id);
    let to = w.citizen_destination(id);
    if from.is_some() || to.is_some() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(f) = &from {
                ui.label(egui::RichText::new("coming from").small().weak());
                if place_button(ui, f) {
                    pick = f.building;
                }
            }
            if let Some(t) = &to {
                ui.label(egui::RichText::new(if from.is_some() { "— going to" } else { "going to" }).small().weak());
                if place_button(ui, t) {
                    pick = t.building;
                }
            }
        });
    }
    pick
}

/// [clarity] A journey's end as a small button (selectable when it is a
/// building or a vehicle), or plain text. True when clicked.
pub fn place_button(ui: &mut egui::Ui, p: &nd_sim::world::Place) -> bool {
    if p.building.is_some() || p.car.is_some() {
        ui.add(egui::Button::new(egui::RichText::new(&p.label).small().color(egui::Color32::from_rgb(230, 225, 210))).small()).on_hover_text("Select it").clicked()
    } else {
        ui.label(egui::RichText::new(&p.label).small());
        false
    }
}

/// [looks] A small painted figure in their colours beside a line saying
/// what they look like.
pub fn look_line(ui: &mut egui::Ui, c: &Citizen) {
    use nd_sim::looks::{skin_srgb, Outfit, HairStyle, COLORS};
    let a = c.appearance();
    let rgb = |c: [u8; 3]| egui::Color32::from_rgb(c[0], c[1], c[2]);
    let fav = a.fav.map(|i| rgb(COLORS[(i as usize).min(COLORS.len() - 1)].srgb));
    let skin = rgb(skin_srgb(a.genes.skin));
    let hair = rgb(a.shown_hair(c.age).srgb());
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 56.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let s = (a.stature(c.age) / 1.1).clamp(0.4, 1.0);
        let h = rect.height() * s / 1.1;
        let foot = rect.bottom();
        let cx = rect.center().x;
        let y = |f: f32| foot - h * f;
        let w = |f: f32| rect.width() * f;
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(egui::pos2(cx + x0, y(y0)), egui::pos2(cx + x1, y(y1)));
        let legs = match a.outfit { Outfit::Suit => fav[0], Outfit::Dress => skin, _ => fav[1] };
        let arms = if matches!(a.outfit, Outfit::Tee | Outfit::Dress) { skin } else { fav[0] };
        p.rect_filled(r(-w(0.2), 0.48, w(0.2), 0.0), 1.0, legs);
        p.rect_filled(r(-w(0.36), 0.82, -w(0.22), 0.47), 1.0, arms);
        p.rect_filled(r(w(0.22), 0.82, w(0.36), 0.47), 1.0, arms);
        let torso_bottom = match a.outfit { Outfit::Coat => 0.27, Outfit::Dress => 0.25, _ => 0.47 };
        p.rect_filled(r(-w(0.24), 0.82, w(0.24), torso_bottom), 1.0, fav[0]);
        p.rect_filled(r(-w(0.25), 0.5, w(0.25), 0.46), 0.0, fav[1]);
        p.rect_filled(r(-w(0.15), 1.0, w(0.15), 0.84), 2.0, skin);
        match a.style {
            HairStyle::Bald => {}
            HairStyle::Afro => { p.rect_filled(r(-w(0.24), 1.08, w(0.24), 0.92), 4.0, hair); }
            HairStyle::Mohawk => { p.rect_filled(r(-w(0.04), 1.06, w(0.04), 0.97), 0.0, hair); }
            HairStyle::Long => { p.rect_filled(r(-w(0.17), 1.03, w(0.17), 0.97), 2.0, hair); p.rect_filled(r(-w(0.19), 0.99, -w(0.13), 0.8), 0.0, hair); p.rect_filled(r(w(0.13), 0.99, w(0.19), 0.8), 0.0, hair); }
            HairStyle::Bun => { p.rect_filled(r(-w(0.16), 1.02, w(0.16), 0.96), 2.0, hair); p.circle_filled(egui::pos2(cx, y(1.05)), w(0.08), hair); }
            HairStyle::Buzz => { p.rect_filled(r(-w(0.155), 1.005, w(0.155), 0.97), 1.0, hair); }
            HairStyle::Short => { p.rect_filled(r(-w(0.165), 1.03, w(0.165), 0.95), 2.0, hair); }
        }
        ui.add(egui::Label::new(egui::RichText::new(a.describe(c.age, c.class())).small()).wrap());
    });
}

/// The spotlight checkbox, written back only when it changes.
pub fn spotlight_toggle(ui: &mut egui::Ui, w: &mut World, id: CitizenId, label: &str) {
    let Some(was) = w.citizens.get(id).map(|c| c.spotlight) else { return };
    let mut spotlight = was;
    if ui.checkbox(&mut spotlight, label).changed() {
        if let Some(c) = w.citizens.get_mut(id) {
            c.spotlight = spotlight;
        }
    }
}

/// One memory line: day, time, text.
pub fn memory_line(ui: &mut egui::Ui, m: &nd_sim::Memory) {
    ui.label(egui::RichText::new(format!("d{} {:02}:{:02}  {}", m.tick / 1440 + 1, (m.tick % 1440) / 60, m.tick % 60, m.text)).small());
}

/// A draggable window on the selected citizen: who they are, what they are
/// doing, where they are going, and everything they remember. The star keeps
/// them in the People window's favourites; closing the pane deselects them.
/// `content` is the map area between the bars the window stays inside.
pub fn pane(ctx: &egui::Context, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, flags: &mut super::UiFlags, content: egui::Rect, cam: (&mut CameraFollow, &mut CameraFly, &mut CameraMode)) {
    let (follow, fly, mode) = cam;
    let Selection::Citizen(id) = *selection else { return };
    if !sim.world.citizens.contains_key(id) {
        return;
    }
    let is_fav = tools.favorites.contains(&id);
    let mut toggle_fav = false;
    let mut go_to: Option<nd_sim::BuildingId> = None;
    let mut open = true;
    let title = sim.world.citizens[id].name.clone();
    egui::Window::new(egui::RichText::new(title).color(egui::Color32::from_rgb(230, 225, 210)).strong())
        .id(egui::Id::new("person_pane"))
        .open(&mut open)
        .default_pos(egui::pos2(content.right() - 372.0, content.top() + 12.0))
        .default_width(340.0)
        .constrain_to(content)
        .resizable(false)
        .show(ctx, |ui| {
            let star = if is_fav { "★ favourite" } else { "☆ favourite" };
            ui.horizontal(|ui| {
                if ui.selectable_label(is_fav, star).on_hover_text("Keep this person in the favourites list of the People window").clicked() {
                    toggle_fav = true;
                }
                if ui.small_button("Flag for Claude (F8)").on_hover_text("Bookmark this person with a note; the remote API's `flags` command lists it").clicked() {
                    flags.want_flag = true;
                }
            });
            // [camera] Follow, zoom to, a gift; their places as buttons.
            ui.horizontal(|ui| {
                super::follow::camera_buttons(ui, sim, Selection::Citizen(id), follow, fly, mode);
                if let Some(msg) = super::follow::gift_button(ui, sim, id, false) {
                    flags.message = msg;
                }
            });
            if let Some(b) = places_row(ui, &sim.world, id) {
                go_to = Some(b);
            }
            ui.separator();
            let w = &sim.world;
            if let Some(b) = activity_heading(ui, w, id) {
                go_to = Some(b);
            }
            citizen_summary(ui, w, &w.citizens[id]);
            super::estate::citizen_line(ui, w, id);
            spotlight_toggle(ui, &mut sim.world, id, "Spotlight: route through the LLM mind");
            ui.separator();
            super::places::citizen_places(ui, &sim.world, id);
            let c = &sim.world.citizens[id];
            hint(ui, &format!("memories, newest first ({})", c.memories.len()));
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for m in c.memories.iter().rev() {
                    memory_line(ui, m);
                }
            });
        });
    if toggle_fav {
        if is_fav {
            tools.favorites.retain(|f| *f != id);
        } else {
            tools.favorites.push(id);
        }
    }
    if !open {
        *selection = Selection::None;
    }
    if let Some(b) = go_to {
        *selection = Selection::Building(b);
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// [camera] Home, work and anything they own as buttons in the pins'
/// colours, each selecting its building (the same as clicking its pin).
fn places_row(ui: &mut egui::Ui, w: &World, id: CitizenId) -> Option<nd_sim::BuildingId> {
    let holdings = w.holdings_of(id);
    if holdings.is_empty() {
        return None;
    }
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (b, h) in holdings {
            let Some(name) = w.buildings.get(b).map(|x| x.name.as_str()) else { continue };
            let [r, g, bl] = crate::marks::holding_rgb(h);
            let col = egui::Color32::from_rgb(r, g, bl);
            let icon = if h == nd_sim::Holding::Owns { "★ " } else { "" };
            let text = egui::RichText::new(format!("{icon}{}: {name}", capital(h.label()))).small().color(col);
            if ui.add(egui::Button::new(text).stroke(egui::Stroke::new(1.0, col.gamma_multiply(0.6)))).on_hover_text("Select this building (its pin is on the map)").clicked() {
                pick = Some(b);
            }
        }
    });
    pick
}
