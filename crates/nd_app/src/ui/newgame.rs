//! The New Game window: pick a landscape preset, adjust its levers (water,
//! height variation, river branching, islands, seed, the starting highway)
//! and watch a mini-map of the result before starting over on it.

use bevy_egui::egui;
use nd_sim::landscape::{Landscape, LandscapeParams, LandscapePreset, LAND_HALF};
use nd_sim::Vec2 as SVec2;

use super::{hint, section, UiCtx, GOLD, WARN};

const PREVIEW: usize = 168;

fn open_id() -> egui::Id {
    egui::Id::new("new_game_open")
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The top-bar toggle.
pub fn toggle(ui: &mut egui::Ui) {
    let open = is_open(ui.ctx());
    if ui.selectable_label(open, "New game…").on_hover_text("Start a new city on a landscape of your choosing").clicked() {
        set_open(ui.ctx(), !open);
    }
}

/// The levers being edited and the mini-map drawn for them.
#[derive(Clone)]
struct Draft {
    params: LandscapeParams,
    drawn: Option<(LandscapeParams, egui::TextureHandle, String)>,
    confirm: bool,
}

pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    if !is_open(ctx) {
        return;
    }
    let id = egui::Id::new("new_game_draft");
    let mut draft: Draft = ctx.data(|d| d.get_temp(id)).unwrap_or_else(|| Draft { params: p.sim.world.landscape.params.clone(), drawn: None, confirm: false });
    let mut open = true;
    egui::Window::new(egui::RichText::new("New game").color(GOLD))
        .open(&mut open)
        .constrain_to(content)
        .default_pos(content.center() - egui::vec2(220.0, 200.0))
        .resizable(false)
        .show(ctx, |ui| body(ui, p, &mut draft));
    if !open {
        set_open(ctx, false);
        draft.confirm = false;
    }
    ctx.data_mut(|d| d.insert_temp(id, draft));
}

fn body(ui: &mut egui::Ui, p: &mut UiCtx, draft: &mut Draft) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            section(ui, "LANDSCAPE");
            for preset in LandscapePreset::ALL {
                let on = draft.params.preset == preset;
                if ui.selectable_label(on, preset.label()).on_hover_text(preset.blurb()).clicked() && !on {
                    // A preset sets its own levers; the seed stays.
                    draft.params = LandscapeParams::preset(preset, draft.params.seed);
                }
            }
            hint(ui, draft.params.preset.blurb());
            if draft.params.preset != LandscapePreset::Flat {
                section(ui, "LEVERS");
                ui.add(egui::Slider::new(&mut draft.params.water, 0.0..=1.0).text("water"));
                ui.add(egui::Slider::new(&mut draft.params.relief, 0.0..=1.0).text("height variation"));
                ui.add(egui::Slider::new(&mut draft.params.branching, 0.0..=1.0).text("river branching"));
                if matches!(draft.params.preset, LandscapePreset::Islands | LandscapePreset::Coast | LandscapePreset::Delta) {
                    ui.add(egui::Slider::new(&mut draft.params.islands, 0..=8).text("islands"));
                }
            }
            ui.horizontal(|ui| {
                ui.label("seed");
                let mut s = draft.params.seed as i64;
                if ui.add(egui::DragValue::new(&mut s).range(0..=999_999)).changed() {
                    draft.params.seed = s.max(0) as u64;
                }
                if ui.small_button("🎲").on_hover_text("another seed").clicked() {
                    draft.params.seed = (draft.params.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 20) % 1_000_000;
                }
            });
            hint(ui, "Every map starts with the highway and the trunk line, edge to edge; the rest is yours to build.");
        });
        ui.vertical(|ui| {
            refresh_preview(ui.ctx(), draft);
            if let Some((_, tex, summary)) = &draft.drawn {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(PREVIEW as f32 * 1.4, PREVIEW as f32 * 1.4)));
                hint(ui, summary);
            }
        });
    });
    ui.separator();
    if draft.confirm {
        ui.colored_label(WARN, "This throws the current city away.");
        ui.horizontal(|ui| {
            if ui.button("Start the new city").clicked() {
                start(p, draft.params.clone());
                draft.confirm = false;
                set_open(ui.ctx(), false);
            }
            if ui.button("Cancel").clicked() {
                draft.confirm = false;
            }
        });
    } else if ui.button(egui::RichText::new("Start").strong()).clicked() {
        if p.sim.world.buildings.is_empty() && p.sim.world.citizens.is_empty() {
            start(p, draft.params.clone());
            set_open(ui.ctx(), false);
        } else {
            draft.confirm = true;
        }
    }
}

fn start(p: &mut UiCtx, params: LandscapeParams) {
    let label = params.preset.label();
    p.sim.new_game(params);
    *p.tools = crate::tools::ToolState::default();
    *p.selection = crate::tools::Selection::None;
    p.plan.invalidate();
    p.flags.message = format!("New city: {label} landscape");
}

/// Draw the mini-map for the current levers when they changed: water blue,
/// land shaded by height, the highway and the railway, hot spots in yellow.
fn refresh_preview(ctx: &egui::Context, draft: &mut Draft) {
    if draft.drawn.as_ref().is_some_and(|(p, ..)| *p == draft.params) {
        return;
    }
    let (image, summary) = preview_image(&draft.params);
    let tex = ctx.load_texture("new_game_preview", image, egui::TextureOptions::LINEAR);
    draft.drawn = Some((draft.params.clone(), tex, summary));
}

/// The mini-map for a set of levers, and a one-line summary of the map.
pub fn preview_image(params: &LandscapeParams) -> (egui::ColorImage, String) {
    let land = Landscape::new(params);
    let mut pixels = Vec::with_capacity(PREVIEW * PREVIEW);
    let span = LAND_HALF * 2.0;
    for j in 0..PREVIEW {
        for i in 0..PREVIEW {
            // North (+y) at the top.
            let p = SVec2::new(-LAND_HALF + (i as f32 + 0.5) / PREVIEW as f32 * span, LAND_HALF - (j as f32 + 0.5) / PREVIEW as f32 * span);
            let c = if let Some(hit) = land.water_at(p) {
                match hit.kind {
                    nd_sim::WaterKind::Stream => [70, 140, 190],
                    nd_sim::WaterKind::River => [40, 105, 170],
                    nd_sim::WaterKind::Lake => [45, 110, 160],
                    nd_sim::WaterKind::Sea => [25, 70, 130],
                }
            } else if land.hot_spot_at(p).is_some_and(|h| h.pos.distance(p) < h.radius) {
                [220, 190, 60]
            } else if params.highway && ((p.y - nd_sim::landscape::HIGHWAY_Y).abs() < 14.0) {
                [235, 180, 90]
            } else if (p.y - nd_sim::rail::TRUNK_Y).abs() < 8.0 {
                [150, 150, 160]
            } else {
                let h = land.base_height(p);
                let t = (h / 60.0).clamp(0.0, 1.0);
                let basin = if p.length() < 900.0 { 1.0 } else { 0.85 };
                let g = [(70.0 + 90.0 * t) * basin, (95.0 + 70.0 * t) * basin, (55.0 + 60.0 * t) * basin];
                [g[0] as u8, g[1] as u8, g[2] as u8]
            };
            pixels.push(egui::Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    let streams = land.rivers.iter().filter(|r| r.stream).count();
    let rivers = land.rivers.len() - streams;
    let islands = land.sea.as_ref().map_or(0, |s| s.islands.len());
    let mut summary = format!("{rivers} river{}, {streams} stream{}, {} lake{}", if rivers == 1 { "" } else { "s" }, if streams == 1 { "" } else { "s" }, land.lakes.len(), if land.lakes.len() == 1 { "" } else { "s" });
    if land.sea.is_some() {
        summary.push_str(&format!(", sea, {islands} island{}", if islands == 1 { "" } else { "s" }));
    }
    summary.push_str(&format!(", {} hot spot{}", land.hot_spots.len(), if land.hot_spots.len() == 1 { "" } else { "s" }));
    if let Some(r) = land.rivers.first() {
        summary.push_str(&format!(". Main river {:.0} m wide, {:.1} m/s.", r.width, r.current));
    }
    (egui::ColorImage { size: [PREVIEW, PREVIEW], source_size: egui::vec2(PREVIEW as f32, PREVIEW as f32), pixels }, summary)
}
