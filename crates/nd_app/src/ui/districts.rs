//! [districts] The District tool's sub-pane and the Districts window:
//! every district with its policies and figures, and [tourism] the
//! visitors, what draws them, the hotels and what they bring in.

use bevy_egui::egui;
use nd_sim::districts::{CommercialSpec, ResidentialSpec, COLORS};
use nd_sim::industry::IndustryKind;
use nd_sim::tourism::{Arrival, AttractionKind};
use nd_sim::world::PolicyChange;

use super::{hint, section, UiCtx, GOLD, IVORY};
use crate::district_tool::{DistrictMode, BRUSH_MAX, BRUSH_MIN};
use crate::sim::Sim;
use crate::tools::ToolState;

fn color_of(c: u8) -> egui::Color32 {
    let c = COLORS[c as usize % COLORS.len()];
    egui::Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8)
}

pub const TOOL_HINT: &str = "Drag to paint the selected district (a new one starts on the first stroke when none is picked); right-drag erases. Shift-click picks the district under the pointer. Polygon: click corners, Enter or right-click fills. Policies change what grows there and how it looks; standing buildings follow over a few days.";

/// The District tool's parameters.
pub fn sub_pane(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    ui.horizontal_wrapped(|ui| {
        for (m, label) in [(DistrictMode::Brush, "Brush"), (DistrictMode::Polygon, "Polygon")] {
            if ui.selectable_label(tools.district.mode == m, label).clicked() {
                tools.district.mode = m;
                tools.district.points.clear();
            }
        }
        ui.separator();
        if tools.district.mode == DistrictMode::Brush {
            ui.add(egui::Slider::new(&mut tools.district.radius, BRUSH_MIN..=BRUSH_MAX).suffix(" m").text("brush"));
        }
        ui.checkbox(&mut tools.district.erase, "Erase");
        ui.separator();
        let current = tools.district.selected.and_then(|id| sim.world.districts.get(id)).map_or("(new district)".to_string(), |d| d.name.clone());
        egui::ComboBox::from_id_salt("district_pick").selected_text(current).show_ui(ui, |ui| {
            if ui.selectable_label(tools.district.selected.is_none(), "(new district)").clicked() {
                tools.district.selected = None;
            }
            for d in &sim.world.districts.list {
                if ui.selectable_label(tools.district.selected == Some(d.id), egui::RichText::new(&d.name).color(color_of(d.color))).clicked() {
                    tools.district.selected = Some(d.id);
                    tools.district.rename = d.name.clone();
                }
            }
        });
        if ui.button("New").on_hover_text("Start a new, empty district and paint it next").clicked() {
            if let Ok(id) = sim.world.create_district(None) {
                tools.district.selected = Some(id);
                tools.district.rename = sim.world.districts.get(id).map(|d| d.name.clone()).unwrap_or_default();
            }
        }
        let mut open = super::windows::Info::Districts.is_open(ui.ctx());
        if ui.toggle_value(&mut open, "Window").on_hover_text("The Districts window: figures, policies and tourism").changed() {
            super::windows::Info::Districts.set_open(ui.ctx(), open);
        }
    });
    if let Some(id) = tools.district.selected.filter(|id| sim.world.districts.get(*id).is_some()) {
        policy_editor(ui, sim, id, &mut tools.district.rename);
    }
    hint(ui, TOOL_HINT);
}

/// Name and policies of one district, editable.
fn policy_editor(ui: &mut egui::Ui, sim: &mut Sim, id: u16, rename: &mut String) {
    let Some(d) = sim.world.districts.get(id).cloned() else { return };
    let mut change = PolicyChange::default();
    ui.horizontal_wrapped(|ui| {
        let r = ui.add(egui::TextEdit::singleline(rename).desired_width(130.0).hint_text("name"));
        if r.lost_focus() && *rename != d.name && !rename.trim().is_empty() {
            change.name = Some(rename.clone());
        }
        ui.label(egui::RichText::new("homes").weak());
        egui::ComboBox::from_id_salt(("res", id)).selected_text(d.residential.map_or("any", |s| s.label())).show_ui(ui, |ui| {
            if ui.selectable_label(d.residential.is_none(), "any").clicked() {
                change.residential = Some(None);
            }
            for s in ResidentialSpec::ALL {
                if ui.selectable_label(d.residential == Some(s), s.label()).clicked() {
                    change.residential = Some(Some(s));
                }
            }
        });
        ui.label(egui::RichText::new("shops").weak());
        egui::ComboBox::from_id_salt(("com", id)).selected_text(d.commercial.map_or("any", |s| s.label())).show_ui(ui, |ui| {
            if ui.selectable_label(d.commercial.is_none(), "any").clicked() {
                change.commercial = Some(None);
            }
            for s in CommercialSpec::ALL {
                if ui.selectable_label(d.commercial == Some(s), s.label()).clicked() {
                    change.commercial = Some(Some(s));
                }
            }
        });
        ui.label(egui::RichText::new("industry").weak());
        egui::ComboBox::from_id_salt(("ind", id)).selected_text(d.industry.map_or("by the ground", |s| s.label())).show_ui(ui, |ui| {
            if ui.selectable_label(d.industry.is_none(), "by the ground").clicked() {
                change.industry = Some(None);
            }
            for k in IndustryKind::ALL {
                if ui.selectable_label(d.industry == Some(k), k.label()).clicked() {
                    change.industry = Some(Some(k));
                }
            }
        });
        let mut historic = d.historic;
        if ui.checkbox(&mut historic, "Historic").on_hover_text("Freeze growth: no upgrades, no redevelopment; new homes and shops in old brick; draws visitors").changed() {
            change.historic = Some(historic);
        }
        if ui.button("Delete").on_hover_text("Remove the district and its policies (buildings stay)").clicked() {
            sim.world.remove_district(id);
        }
    });
    if change.name.is_some() || change.residential.is_some() || change.commercial.is_some() || change.industry.is_some() || change.historic.is_some() {
        let _ = sim.world.set_district_policy(id, change);
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::RichText>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.into());
    });
}

/// The Districts window.
pub fn window(ui: &mut egui::Ui, p: &mut UiCtx) {
    let stats = p.sim.world.district_stats();
    section(ui, "DISTRICTS");
    if stats.is_empty() {
        hint(ui, "No districts yet. Pick the District tool (9) and paint one over the map.");
    } else {
        egui::Grid::new("district_table").num_columns(7).spacing(egui::vec2(10.0, 2.0)).striped(true).show(ui, |ui| {
            for h in ["district", "policy", "ha", "buildings (+sites)", "residents (homes)", "workers/jobs", "visitors"] {
                ui.label(egui::RichText::new(h).small().color(GOLD));
            }
            ui.end_row();
            for s in &stats {
                let color = p.sim.world.districts.get(s.id).map_or(IVORY, |d| color_of(d.color));
                let picked = p.tools.district.selected == Some(s.id);
                if ui.selectable_label(picked, egui::RichText::new(&s.name).color(color).strong()).on_hover_text("Select it to paint and edit; the camera flies there").clicked() {
                    p.tools.district.selected = Some(s.id);
                    p.tools.district.rename = s.name.clone();
                    p.fly_to = Some(bevy::math::Vec2::new(s.centroid[0], s.centroid[1]));
                }
                ui.label(egui::RichText::new(&s.policy).small());
                ui.label(format!("{:.1}", s.area_m2 / 10_000.0));
                ui.label(format!("{} (+{})", s.buildings, s.under_construction));
                ui.label(format!("{} ({})", s.residents, s.homes));
                ui.label(format!("{}/{}", s.workers, s.jobs));
                ui.label(format!("{}", s.tourists_staying));
                ui.end_row();
            }
        });
        if let Some(s) = p.tools.district.selected.and_then(|id| stats.iter().find(|s| s.id == id)) {
            section(ui, &format!("{}", s.name.to_uppercase()));
            row(ui, "land value", format!("{:.2}   rent {:.0}/home/day", s.land_value_avg, s.rent_avg));
            row(ui, "tiers", format!("low {}   mid {}   high {}", s.by_tier[0], s.by_tier[1], s.by_tier[2]));
            row(ui, "in the district's look", format!("{} of {}", s.in_look, s.buildings));
            row(ui, "lots", format!("{} ({} vacant)", s.lots, s.vacant_lots));
            row(ui, "businesses", format!("{} open   hotels {} with {} beds", s.businesses_open, s.hotels, s.beds));
            row(ui, "draws visitors", format!("{:.1} points", s.attraction));
            let id = s.id;
            policy_editor(ui, p.sim, id, &mut p.tools.district.rename);
        }
    }
    tourism(ui, p);
}

/// Tourism: who is here, what draws them, what they spend.
fn tourism(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let t = &w.tourism;
    let s = &t.stats;
    section(ui, "TOURISM");
    row(ui, "attractiveness", egui::RichText::new(format!("{:.0}", t.attractiveness)).color(IVORY).strong());
    row(ui, "would come per day", format!("{:.0}   ways in: {}", t.demand_per_day, Arrival::ALL.iter().filter(|a| t.ways_in[a.index()]).map(|a| a.label()).collect::<Vec<_>>().join(", ")));
    row(ui, "visitors in town", format!("{}   out and about {}", t.count(), t.tourists.values().filter(|x| x.is_visible()).count()));
    row(ui, "arrivals", format!("{} today ({}), {} yesterday", s.arrivals_today, Arrival::ALL.iter().map(|a| format!("{} {}", a.label(), s.arrivals_by_mode_today[a.index()])).collect::<Vec<_>>().join(" · "), s.arrivals_yesterday));
    row(ui, "no bed for them", format!("{} today, {} yesterday   hotel demand {:.1}", s.turned_away_today, s.turned_away_yesterday, t.hotel_demand));
    let hotels = w.hotels();
    let beds: u32 = hotels.iter().map(|h| h.1 as u32).sum();
    let guests: u32 = hotels.iter().map(|h| h.2 as u32).sum();
    row(ui, "hotels", format!("{}   beds {}   guests {}", hotels.len(), beds, guests));
    row(ui, "spending today", format!("{:.0}  (hotels {:.0} · shops {:.0} · venues {:.0})   yesterday {:.0}", s.spend_today(), s.spend_hotels_today, s.spend_shops_today, s.spend_venues_today, s.spend_yesterday));
    row(ui, "tourist tax", format!("{:.0} today, {:.0} yesterday", s.tax_today, s.tax_yesterday));
    let by_kind = t.score_by_kind();
    ui.horizontal_wrapped(|ui| {
        for k in AttractionKind::ALL {
            if by_kind[k.index()] > 0.0 {
                ui.label(egui::RichText::new(format!("{} {:.0}", k.label(), by_kind[k.index()])).small());
            }
        }
    });
    let mut top: Vec<_> = t.attractions.iter().collect();
    top.sort_by(|a, b| b.score.total_cmp(&a.score));
    for a in top.into_iter().take(6) {
        hint(ui, &format!("{:.1}  {} ({})", a.score, a.name, a.kind.label()));
    }
    hint(ui, "Monuments, stadiums, good parks, nightlife, landmark districts (historic, neon strip, night market, tourist quarter) and the waterfront draw visitors. They need a way in and a bed: hotels open on commercial land when visitors are turned away.");
}
