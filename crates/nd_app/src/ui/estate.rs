//! [land] The inspector's OWNERSHIP section: who owns a building, what it
//! would sell for, the plots it spans and the form it was built in, the
//! owner's other properties (each a click away; the map highlights them
//! all while it is selected), and what the pressure on its lot allows.

use bevy_egui::egui;
use super::{hint, section, GOLD, IVORY};
use super::fmt::money;
use crate::tools::Selection;
use nd_sim::estate::OwnerKind;

pub fn building_section(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId, selection: &mut Selection) {
    let Some(b) = w.buildings.get(id) else { return };
    // [megacorp] The corp or university behind it, with its logo.
    brand_line(ui, w, id);
    if b.is_special() || b.land_use == nd_sim::LandUse::Park {
        return;
    }
    section(ui, "OWNERSHIP");
    let owner = w.owner_of(id);
    let who = match owner.map(|(_, o)| (o.name.clone(), &o.kind)) {
        Some((name, OwnerKind::Elite { .. })) => format!("{name} (billionaire)"),
        Some((name, OwnerKind::Developer)) => format!("{name} (developer)"),
        Some((name, OwnerKind::Household { .. })) => format!("{name} (owner-occupied)"),
        Some((name, OwnerKind::Person { .. })) => format!("{name}, who founded it"),
        Some((name, OwnerKind::MegaCorp { .. })) => format!("{name} (mega-corp)"),
        Some((name, OwnerKind::University { .. })) => format!("{name} (university)"),
        Some((name, OwnerKind::Firm { kind })) => {
            let n = owner.map_or(0, |(_, o)| o.properties.len());
            if n > 1 { format!("{name} ({} chain of {n})", kind.label()) } else { format!("{name} (the {} firm)", kind.label()) }
        }
        None => if b.homes > 0 { "an independent landlord".to_string() } else { "independently owned".to_string() },
    };
    ui.label(egui::RichText::new(format!("owned by {who}  ·  for sale at {}", money(w.sale_price(id)))).color(IVORY));
    let mut bits: Vec<String> = Vec::new();
    if let Some(f) = b.plot.form {
        bits.push(f.label().to_string());
    }
    if b.plot.merged() {
        bits.push(format!("spans {} plots", 1 + b.plot.extra_lots.len()));
    }
    if !b.plot.pending_lots.is_empty() {
        bits.push(format!("{} bought plot{} still clearing", b.plot.pending_lots.len(), super::fmt::s(b.plot.pending_lots.len())));
    }
    if let Some(lot) = w.lots.get(b.lot) {
        // [pt-growth] The land density (people present and out on foot).
        let d = w.land_density_at(lot.center);
        bits.push(format!("land density {d:.0} (foot traffic {:.0}) supports {} floors here (height cap {})", w.estate.pressure.foot_at(lot.center), w.pressure_floors(b.lot), w.height_cap(b.lot)));
    }
    if w.wants_to_expand(id) {
        bits.push("doing well enough to expand".to_string());
    }
    hint(ui, &bits.join("  ·  "));
    let others: Vec<nd_sim::BuildingId> = w.owned_with(id).into_iter().filter(|p| *p != id).collect();
    if !others.is_empty() {
        ui.label(egui::RichText::new(format!("also owns {} other propert{} (highlighted on the map):", others.len(), if others.len() == 1 { "y" } else { "ies" })).small().color(GOLD));
        ui.horizontal_wrapped(|ui| {
            for p in others.iter().take(12) {
                let Some(o) = w.buildings.get(*p) else { continue };
                let kind = if o.homes > 0 && o.business.is_none() { "home" } else { o.business.as_ref().map_or("building", |z| z.kind.label()) };
                if ui.small_button(format!("{} ({kind})", o.name)).clicked() {
                    *selection = Selection::Building(*p);
                }
            }
        });
    }
}

/// The super-elite line in a citizen's panel: fortune, mansion, garage
/// and the convoy.
pub fn citizen_line(ui: &mut egui::Ui, w: &nd_sim::World, cid: nd_sim::CitizenId) {
    let Some(e) = w.estate.elite_of(cid) else { return };
    let mansion = e.mansion.and_then(|m| w.buildings.get(m)).map_or("no mansion yet".to_string(), |m| if m.construction.is_some() { format!("{} (being built)", m.name) } else { m.name.clone() });
    let props = w.estate.owners.get(e.owner).map_or(0, |o| o.properties.len());
    ui.label(egui::RichText::new(format!("SUPER-ELITE  ·  worth {}  ·  {mansion}  ·  {props} properties  ·  {} supercars  ·  {} escort cars{}", money(e.net_worth), e.cars.len(), e.escorts, if e.police { " and a police escort" } else { "" })).strong().color(GOLD));
}

// ── [megacorp] brands, corps and campuses ───────────────────────────────────

/// The brand colour as egui's.
fn brand_color(b: &nd_sim::corp::Brand) -> egui::Color32 {
    let [r, g, bl] = b.rgb();
    egui::Color32::from_rgb(r, g, bl)
}

/// Paint a brand's logo (the same pixel mark as on the buildings), `size`
/// points square.
fn paint_logo(ui: &mut egui::Ui, b: &nd_sim::corp::Brand, size: f32) -> egui::Response {
    use nd_sim::corp::{LogoPixel, LOGO_N};
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(10, 10, 14));
    let px = (size - 2.0) / LOGO_N as f32;
    let col = brand_color(b);
    for (r, row) in b.logo_bitmap().iter().enumerate() {
        for (c, p) in row.iter().enumerate() {
            let fill = match p {
                LogoPixel::Empty => continue,
                LogoPixel::Shape => col,
                LogoPixel::LetterOnShape => egui::Color32::from_rgb(12, 12, 16),
                LogoPixel::LetterAlone => egui::Color32::from_rgb(240, 240, 235),
            };
            let min = rect.min + egui::vec2(1.0 + c as f32 * px, 1.0 + r as f32 * px);
            painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(px + 0.3, px + 0.3)), 0.0, fill);
        }
    }
    resp
}

/// The owner line in the inspector for a corp's or a campus's building.
fn brand_line(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) {
    let (brand, what) = if let Some(c) = w.corp_of(id) {
        let role = if c.hq == Some(id) {
            "headquarters".to_string()
        } else if w.buildings.get(id).is_some_and(|b| b.business.is_none() && b.homes > 0) {
            format!("staff housing ({:.0}% off the rent for its staff)", nd_sim::corp::STAFF_RENT_DISCOUNT * 100.0)
        } else {
            "holding".to_string()
        };
        let n = w.estate.owners.get(c.owner).map_or(0, |o| o.properties.len());
        (c.brand.clone(), format!("MEGA-CORP  ·  {role}  ·  {n} properties  ·  {:.1}% of the town's takings", w.corp_market_share(c.owner) * 100.0))
    } else if let Some(k) = w.campus_of(id) {
        let role = if k.building == id { "main hall".to_string() } else { k.annex_of(id).map_or("campus".to_string(), |a| a.label().to_string()) };
        (k.brand.clone(), format!("UNIVERSITY  ·  {role}  ·  {} students  ·  {} campus buildings", k.students.len(), k.annexes.len()))
    } else {
        return;
    };
    ui.horizontal(|ui| {
        paint_logo(ui, &brand, 30.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&brand.name).strong().color(brand_color(&brand)));
            ui.label(egui::RichText::new(what).small().color(IVORY));
        });
    });
}

/// City Data › Economy › Corporations: every mega-corp with its logo, HQ,
/// holdings, staff, housing and market share (a click selects its HQ and
/// the map outlines all it owns), the universities, the biggest companies
/// against the thresholds, and the player's rules.
pub fn corporations_page(ui: &mut egui::Ui, p: &mut super::UiCtx) {
    use nd_sim::corp::*;
    let mut select: Option<nd_sim::BuildingId> = None;
    {
        let w = &p.sim.world;
        let total = w.ordinary_buildings();
        super::pane::cards(ui, |ui| {
            super::pane::kpi(ui, "mega-corps", egui::RichText::new(format!("{} of {}", w.corps.corps.len(), w.corp_room())).color(GOLD));
            super::pane::kpi(ui, "universities", egui::RichText::new(format!("{}", w.corps.campuses.len())));
            super::pane::kpi(ui, "bought by corps (all time)", egui::RichText::new(format!("{}", w.corps.stats.purchases_total)));
            super::pane::kpi(ui, "levy yesterday", egui::RichText::new(format!("{:.0}", w.corps.stats.levy_today)));
        });
        ui.add_space(6.0);
        if w.corps.corps.is_empty() {
            hint(ui, &format!("No mega-corps yet. A company becomes one when its businesses take {:.0} a day with {} staff over {} properties, in a town of {} people or more.", PROMOTE_REVENUE, PROMOTE_STAFF, PROMOTE_HOLDINGS, CORP_MIN_POP));
        }
        for c in &w.corps.corps {
            let props: Vec<nd_sim::BuildingId> = w.estate.owners.get(c.owner).map(|o| o.properties.clone()).unwrap_or_default();
            let staff = w.corp_staff(c.owner).len();
            let (homes, blocks) = w.corp_homes(c.owner);
            let mut kinds: Vec<&str> = props.iter().filter_map(|b| w.buildings.get(*b)?.business.as_ref().map(|z| z.kind.label())).collect();
            kinds.sort();
            kinds.dedup();
            let uses = [nd_sim::LandUse::Residential, nd_sim::LandUse::Commercial, nd_sim::LandUse::Industrial, nd_sim::LandUse::Entertainment].map(|lu| (lu, props.iter().filter(|b| w.buildings.get(**b).is_some_and(|x| x.land_use == lu)).count()));
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    paint_logo(ui, &c.brand, 44.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&c.brand.name).heading().color(brand_color(&c.brand)));
                        ui.label(egui::RichText::new(format!("grew out of {} ({}) on day {}", c.origin_name, c.origin.label(), c.founded_day + 1)).small().color(IVORY));
                        let hq = c.hq.and_then(|h| w.buildings.get(h)).map_or("no HQ yet".to_string(), |h| format!("HQ {} ({} floors{})", h.name, h.floors, if h.construction.is_some() { ", going up" } else { "" }));
                        ui.label(egui::RichText::new(hq).color(GOLD));
                    });
                });
                ui.label(format!(
                    "{} properties of {} allowed ({:.1}% of the town)  ·  {} staff  ·  {} blocks of staff housing, {homes} homes  ·  takings yesterday {:.0}  ·  {:.1}% of all business takings  ·  levy paid {:.0}",
                    props.len(),
                    w.corp_cap(),
                    props.len() as f32 / total.max(1) as f32 * 100.0,
                    staff,
                    blocks.len(),
                    c.revenue_yesterday,
                    w.corp_market_share(c.owner) * 100.0,
                    c.levy_total
                ));
                ui.label(egui::RichText::new(format!("{}  ·  industries: {}", uses.iter().map(|(lu, n)| format!("{} {n}", lu.label())).collect::<Vec<_>>().join(", "), kinds.join(", "))).small().color(IVORY));
                if let Some(h) = c.hq.or(props.first().copied()) {
                    if ui.button("Show on the map (selects the HQ, outlines every holding)").clicked() {
                        select = Some(h);
                    }
                }
            });
        }
        if !w.corps.campuses.is_empty() {
            section(ui, "UNIVERSITIES");
            for (ki, k) in w.corps.campuses.iter().enumerate() {
                let name = w.buildings.get(k.building).map_or(String::new(), |b| b.name.clone());
                ui.horizontal(|ui| {
                    paint_logo(ui, &k.brand, 30.0);
                    let mut counts: Vec<String> = Vec::new();
                    for a in Annex::ALL {
                        let n = k.annexes.iter().filter(|(_, x)| *x == a).count();
                        if n > 0 {
                            counts.push(format!("{n} {}", a.label()));
                        }
                    }
                    ui.label(egui::RichText::new(format!("{name}  ·  {} of {} places taken  ·  {} dorm rooms  ·  {}", k.students.len(), w.campus_places(ki), w.campus_dorm_homes(ki), if counts.is_empty() { "no annexes yet".to_string() } else { counts.join(", ") })).color(brand_color(&k.brand)));
                    if ui.small_button("show").clicked() {
                        select = Some(k.building);
                    }
                });
            }
            hint(ui, &format!("Students out tonight {}  ·  to class today {}  ·  student trade at pizza places, cafés and bookshops today {:.0}", w.corps.stats.students_out_today, w.corps.stats.students_to_class_today, w.corps.stats.student_trade_today));
        }
        section(ui, "BIGGEST COMPANIES");
        for c in w.companies().into_iter().take(6) {
            ui.label(egui::RichText::new(format!("{}  ·  {}  ·  {} propert{}  ·  {} staff  ·  takes {:.0} a day{}", c.name, c.kind.label(), c.holdings, if c.holdings == 1 { "y" } else { "ies" }, c.staff, c.revenue, if c.qualifies() { "  ·  big enough to become a mega-corp" } else { "" })).small());
        }
    }
    section(ui, "RULES");
    let r = &mut p.sim.world.corps.rules;
    ui.checkbox(&mut r.allowed, "Companies may grow into mega-corps");
    ui.add(egui::Slider::new(&mut r.max_share, MIN_MAX_SHARE..=MAX_MAX_SHARE).text("most of the town one corp may own").custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    ui.add(egui::Slider::new(&mut r.levy, 0.0..=MAX_LEVY).text("antitrust levy on corp takings").custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    hint(ui, "A corp over the cap sells its newest holdings two a day (never its HQ). The levy is booked as business tax.");
    if let Some(b) = select {
        *p.selection = Selection::Building(b);
        if let Some(l) = p.sim.world.building_lot(b) {
            p.fly_to = Some(l.center);
        }
    }
}
