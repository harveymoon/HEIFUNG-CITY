//! The citizen read-out shared by the inspector and the person pane, and the
//! person pane itself.

use bevy_egui::egui;
use nd_minds::Personality;
use nd_sim::citizens::TravelMode;
use nd_sim::{Activity, Citizen, CitizenId, World};

use super::hint;
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
    ui.label(match destination {
        Some(d) => format!("{}{how}  ·  to {d}", c.activity.label()),
        None => format!("{}{how}", c.activity.label()),
    });
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
pub fn pane(ctx: &egui::Context, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, flags: &mut super::UiFlags, content: egui::Rect) {
    let Selection::Citizen(id) = *selection else { return };
    if !sim.world.citizens.contains_key(id) {
        return;
    }
    let is_fav = tools.favorites.contains(&id);
    let mut toggle_fav = false;
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
            ui.separator();
            let w = &sim.world;
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
}
