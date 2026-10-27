//! [districts] The District tool's sub-pane, and City Data's City,
//! Districts page (every district with its policies and figures) and City,
//! Tourism page ([tourism] the visitors, what draws them, the hotels and
//! what they bring in).

use bevy_egui::egui;
use nd_sim::districts::COLORS;
use nd_sim::stories::{Policy, Story};
use nd_sim::tourism::{Arrival, AttractionKind};
use nd_sim::world::PolicyChange;

use super::pane::{self, heading, row};
use super::{hint, UiCtx, GOLD, IVORY};
use crate::district_tool::{DistrictMode, BRUSH_MAX, BRUSH_MIN};
use crate::sim::Sim;
use crate::tools::ToolState;

fn color_of(c: u8) -> egui::Color32 {
    let c = COLORS[c as usize % COLORS.len()];
    super::theme::rgb_f(c)
}

pub const TOOL_HINT: &str = "Drag to paint the selected district (a new one starts on the first stroke when none is picked); right-drag erases. Shift-click picks the district under the pointer. Polygon: click corners, Enter or right-click fills. A district's story sets what grows there and how it looks (standing buildings follow over a few days) and its own taxes, which you can fine-tune.";

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
        super::city_data::link(ui, "Figures", super::city_data::Page::CityDistricts, "City Data, City, Districts: every district's figures and policies");
    });
    if let Some(id) = tools.district.selected.filter(|id| sim.world.districts.get(*id).is_some()) {
        policy_editor(ui, sim, id, &mut tools.district.rename);
    }
    hint(ui, TOOL_HINT);
}

/// [stories] Name, story and policies of one district, editable. The story
/// sets the building types; the policy dials and the height limit are the
/// player's to fine-tune.
fn policy_editor(ui: &mut egui::Ui, sim: &mut Sim, id: u16, rename: &mut String) {
    let Some(d) = sim.world.districts.get(id).cloned() else { return };
    let mut change = PolicyChange::default();
    ui.horizontal_wrapped(|ui| {
        let r = ui.add(egui::TextEdit::singleline(rename).desired_width(130.0).hint_text("name"));
        if r.lost_focus() && *rename != d.name && !rename.trim().is_empty() {
            change.name = Some(rename.clone());
        }
        ui.label(egui::RichText::new("story").weak());
        egui::ComboBox::from_id_salt(("story", id)).width(190.0).selected_text(egui::RichText::new(d.story_label()).color(GOLD)).show_ui(ui, |ui| {
            if ui.selectable_label(d.story.is_none() && !d.is_custom(), "No story").on_hover_text("Grows like the city around it; policies off").clicked() {
                change.story = Some(None);
            }
            for s in Story::ALL {
                if ui.selectable_label(d.story == Some(s), s.label()).on_hover_text(format!("{}\n\nSets {}", s.backstory(), s.sets())).clicked() {
                    change.story = Some(Some(s));
                }
            }
        });
        // [supertall] The district's height limit.
        let mut capped = d.height_limit.is_some();
        if ui.checkbox(&mut capped, "Height limit").on_hover_text("Nothing new rises over it here: towers and ordinary buildings alike (standing ones stay)").changed() {
            change.height_limit = Some(capped.then_some(d.height_limit.unwrap_or(120.0)));
        }
        if let Some(mut m) = d.height_limit {
            if ui.add(egui::DragValue::new(&mut m).range(10.0..=1000.0).speed(5.0).suffix(" m")).changed() {
                change.height_limit = Some(Some(m));
            }
        }
        if ui.button("Delete").on_hover_text("Remove the district and its policies (buildings stay)").clicked() {
            sim.world.remove_district(id);
        }
    });
    match d.story {
        Some(s) => {
            ui.label(egui::RichText::new(s.backstory()).italics().color(IVORY));
            ui.label(egui::RichText::new(format!("Builds: {}", architecture(&d))).weak()).on_hover_text(format!("Its own look ({}) comes later; until then it builds in today's looks. Switch the story to change the building types.", s.planned_look()));
        }
        None if d.is_custom() => {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("Custom, from an older save: {}.", architecture(&d))).weak());
                if let Some(c) = Story::closest(&d) {
                    if ui.button(format!("Switch to {}", c.label())).on_hover_text(format!("{}\n\nSets {}", c.backstory(), c.sets())).clicked() {
                        change.story = Some(Some(c));
                    }
                }
            });
        }
        None => hint(ui, "No story: it grows like the city around it. Pick a story to give it a character, its own looks and its own taxes."),
    }
    // The policy dials.
    ui.horizontal_wrapped(|ui| {
        for p in Policy::ALL {
            let v = d.policies.get(p);
            if p == Policy::LateHours {
                let mut on = d.policies.late_hours;
                if ui.checkbox(&mut on, p.label()).on_hover_text(p.help()).changed() {
                    change.policies.push((p, on as u8 as f32));
                }
                continue;
            }
            let (lo, hi) = p.range();
            ui.label(egui::RichText::new(p.label()).weak()).on_hover_text(p.help());
            if p.is_share() {
                let mut pct = v * 100.0;
                if ui.add(egui::DragValue::new(&mut pct).range(lo * 100.0..=hi * 100.0).speed(0.5).suffix("%")).on_hover_text(p.help()).changed() {
                    change.policies.push((p, pct / 100.0));
                }
            } else {
                let mut x = v;
                if ui.add(egui::DragValue::new(&mut x).range(lo..=hi).speed(1.0).prefix("$").suffix("/day")).on_hover_text(p.help()).changed() {
                    change.policies.push((p, x));
                }
            }
        }
    });
    if change.name.is_some() || change.story.is_some() || !change.policies.is_empty() || change.height_limit.is_some() {
        let _ = sim.world.set_district_policy(id, change);
    }
}

/// [stories] What a district builds, in words.
fn architecture(d: &nd_sim::districts::District) -> String {
    let mut v = vec![format!("homes {}", d.residential.map_or("any", |r| r.label())), format!("shops {}", d.commercial.map_or("any", |c| c.label())), format!("industry {}", d.industry.map_or("by the ground", |i| i.label()))];
    if d.historic {
        v.push("Historic freeze".into());
    }
    v.join(", ")
}

/// [stories] The story picker: every story with its backstory and what it
/// sets, for the picked district.
fn story_cards(ui: &mut egui::Ui, sim: &mut Sim, id: u16) {
    let Some(cur) = sim.world.districts.get(id).map(|d| d.story) else { return };
    let mut pick = None;
    for s in Story::ALL {
        let on = cur == Some(s);
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(on, egui::RichText::new(s.label()).strong().color(if on { GOLD } else { IVORY })).on_hover_text("Pick this story for the district").clicked() && !on {
                pick = Some(s);
            }
            ui.label(egui::RichText::new(s.sets()).small().weak());
        });
        ui.add(egui::Label::new(egui::RichText::new(s.backstory()).small().italics()).wrap());
        ui.add_space(4.0);
    }
    if let Some(s) = pick {
        let _ = sim.world.set_district_policy(id, PolicyChange { story: Some(Some(s)), ..Default::default() });
    }
}

/// City, Districts: every district's figures; the picked one's detail and
/// policies.
pub fn page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let stats = p.sim.world.district_stats();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "districts", egui::RichText::new(format!("{}", stats.len())).color(GOLD));
        pane::kpi(ui, "hectares in districts", egui::RichText::new(format!("{:.1}", stats.iter().map(|s| s.area_m2).sum::<f32>() / 10_000.0 + 0.0)));
        pane::kpi(ui, "residents in districts", egui::RichText::new(format!("{}", stats.iter().map(|s| s.residents).sum::<u32>())));
        pane::kpi(ui, "visitors staying", egui::RichText::new(format!("{}", stats.iter().map(|s| s.tourists_staying).sum::<u32>())));
    });
    heading(ui, "Every district");
    if stats.is_empty() {
        hint(ui, "No districts yet. Pick the District tool (9) and paint one over the map.");
    } else {
        hint(ui, "Click a district to pick it: its detail and policies show below and the District tool paints it.");
        // [pt-ux] Sortable by any column.
        let key = |s: &&nd_sim::world::DistrictStats, c: usize| match c {
            0 => pane::SortKey::text(s.name.clone()),
            1 => pane::SortKey::text(s.policy.clone()),
            2 => pane::SortKey::num(s.area_m2),
            3 => pane::SortKey::num(s.buildings as f64),
            4 => pane::SortKey::num(s.residents as f64),
            5 => pane::SortKey::num(s.workers as f64),
            _ => pane::SortKey::num(s.tourists_staying as f64),
        };
        let rows: Vec<&nd_sim::world::DistrictStats> = stats.iter().collect();
        pane::sortable_plain(ui, "district_table", &["district", "story", "ha", "buildings (+sites)", "residents (homes)", "workers/jobs", "visitors"], &rows, key, |ui, s| {
            let color = p.sim.world.districts.get(s.id).map_or(IVORY, |d| color_of(d.color));
            let picked = p.tools.district.selected == Some(s.id);
            if ui.selectable_label(picked, egui::RichText::new(&s.name).color(color).strong()).on_hover_text("Pick it to see its detail and edit its policies").clicked() {
                p.tools.district.selected = Some(s.id);
                p.tools.district.rename = s.name.clone();
            }
            ui.label(&s.policy);
            ui.label(format!("{:.1}", s.area_m2 / 10_000.0));
            ui.label(format!("{} (+{})", s.buildings, s.under_construction));
            ui.label(format!("{} ({})", s.residents, s.homes));
            ui.label(format!("{}/{}", s.workers, s.jobs));
            ui.label(format!("{}", s.tourists_staying));
        });
        if let Some(s) = p.tools.district.selected.and_then(|id| stats.iter().find(|s| s.id == id)) {
            heading(ui, &s.name);
            row(ui, "land value", format!("{:.2}   rent {:.0}/home/day", s.land_value_avg, s.rent_avg));
            row(ui, "tiers", format!("low {}   mid {}   high {}", s.by_tier[0], s.by_tier[1], s.by_tier[2]));
            row(ui, "in the district's look", format!("{} of {}", s.in_look, s.buildings));
            row(ui, "lots", format!("{} ({} vacant)", s.lots, s.vacant_lots));
            row(ui, "businesses", format!("{} open   hotels {} with {} beds", s.businesses_open, s.hotels, s.beds));
            row(ui, "draws visitors", format!("{:.1} points", s.attraction));
            let (id, at) = (s.id, s.centroid);
            // [stories] What its policies moved and what they do to growth.
            if let Some(d) = p.sim.world.districts.get(id) {
                let b = &d.books;
                let by: Vec<String> = Policy::ALL.iter().filter(|q| b.yesterday[q.index()] != 0.0).map(|q| format!("{} {:+.0}", q.label(), b.yesterday[q.index()])).collect();
                // (+ 0.0: no "-0" for an empty day)
                row(ui, "district taxes yesterday", format!("levied ${:.0}   paid out ${:.0}   net {:+.0}{}", b.levied_yesterday() + 0.0, b.spent_yesterday() + 0.0, b.levied_yesterday() - b.spent_yesterday() + 0.0, if by.is_empty() { String::new() } else { format!("   ({})", by.join(", ")) }));
                let today: f32 = b.today.iter().sum::<f32>() + 0.0;
                row(ui, "so far today", format!("net {today:+.0} to the city"));
                let pulls: Vec<String> = nd_sim::LandUse::ALL.iter().filter(|lu| d.policies.pull(**lu) != 1.0).map(|lu| format!("{} x{:.2}", lu.label(), d.policies.pull(*lu))).collect();
                row(ui, "new buildings here", if pulls.is_empty() { "as anywhere in town".to_string() } else { pulls.join("   ") });
            }
            heading(ui, "Story and policies");
            policy_editor(ui, p.sim, id, &mut p.tools.district.rename);
            egui::CollapsingHeader::new("Stories").id_salt(("stories_open", id)).default_open(true).show(ui, |ui| story_cards(ui, p.sim, id));
            if ui.button("Show on the map").on_hover_text("Close City Data and fly there").clicked() {
                p.fly_to = Some(bevy::math::Vec2::new(at[0], at[1]));
            }
        }
    }
}

/// City, Tourism: who is here, what draws them, what they spend.
pub fn tourism_page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let t = &w.tourism;
    let s = &t.stats;
    pane::cards(ui, |ui| {
        pane::kpi(ui, "attractiveness", egui::RichText::new(format!("{:.0}", t.attractiveness)).color(GOLD));
        pane::kpi(ui, "visitors in town", egui::RichText::new(format!("{}", t.count())));
        pane::kpi(ui, "arrivals today", egui::RichText::new(format!("{}", s.arrivals_today)));
        pane::kpi(ui, "no bed for them today", super::tools::warn_if(s.turned_away_today > 0, format!("{}", s.turned_away_today)));
        pane::kpi(ui, "spending today", egui::RichText::new(format!("{:.0}", s.spend_today())));
        pane::kpi(ui, "tourist tax yesterday", egui::RichText::new(format!("{:.0}", s.tax_yesterday)));
    });
    heading(ui, "Visitors");
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
    heading(ui, "What draws them");
    let by_kind = t.score_by_kind();
    pane::row_with(ui, "by kind", |ui| {
        ui.horizontal_wrapped(|ui| {
            for k in AttractionKind::ALL {
                if by_kind[k.index()] > 0.0 {
                    ui.label(format!("{} {:.0}", k.label(), by_kind[k.index()]));
                }
            }
        });
    });
    let mut top: Vec<_> = t.attractions.iter().collect();
    top.sort_by(|a, b| b.score.total_cmp(&a.score));
    if !top.is_empty() {
        top.truncate(12);
        let key = |a: &&nd_sim::tourism::Attraction, c: usize| match c {
            0 => pane::SortKey::text(a.name.clone()),
            1 => pane::SortKey::text(a.kind.label()),
            _ => pane::SortKey::num(a.score),
        };
        pane::sortable_plain(ui, "tourism_top", &["attraction", "kind", "points"], &top, key, |ui, a| {
            ui.label(&a.name);
            ui.label(egui::RichText::new(a.kind.label()).weak());
            ui.label(format!("{:.1}", a.score));
        });
    }
    hint(ui, "Landmark attractions (museums, the pier, the grand park, the zoo, the observatory, the aquarium, the botanical garden, the opera), monuments, stadiums, good parks, nightlife, landmark districts (historic, neon strip, night market, tourist quarter) and the waterfront draw visitors. They need a way in and a bed: hotels open on commercial land when visitors are turned away.");
}
