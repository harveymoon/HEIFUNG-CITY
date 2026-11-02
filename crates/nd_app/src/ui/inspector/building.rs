//! The inspector on a building: its header, figures, people, money and
//! services, and the dirty-industry lines.

use super::*;

/// A selected building, most important first: its name, then what it is
/// (zone, business, district, tier) as coloured tags, then only the
/// sections that apply to it. A business shows its books and a cash-flow
/// sparkline and no homes; a block of flats shows its homes; the people in
/// it are a grid of faces that select them.
pub(super) fn building(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, assets: &GameAssets, id: nd_sim::BuildingId) {
    // [free-placement] A bulldoze confirmed last frame: it goes, and its
    // ground back to the zoning plan.
    // [fix/review-app] Keyed by the city too: a loaded city reuses ids, and
    // its building must not come up already armed.
    let bulldoze = egui::Id::new(("inspector_bulldoze", sim.generation, id));
    if ui.ctx().data_mut(|d| d.remove_temp::<bool>(bulldoze.with("go"))).unwrap_or(false) {
        match sim.world.bulldoze_building(id) {
            Ok(_) => {
                *selection = Selection::None;
                return;
            }
            // Say why, beside the button (the armed state is already gone).
            Err(e) => {
                ui.ctx().data_mut(|d| d.insert_temp(bulldoze.with("err"), e.to_string()));
            }
        }
    }
    let w = &sim.world;
    let b = &w.buildings[id];
    // ── Header ──
    // [plant-groups] A member of a Sunfield or Wind Park shows the park.
    let group = w.plant_group_of(id);
    match &group {
        Some(g) => {
            ui.label(egui::RichText::new(&g.name).font(crate::ui::theme::title()).color(IVORY).strong());
            hint(ui, &format!("{} lots run as one; this one is {}", g.members.len(), b.name));
        }
        None => {
            ui.label(egui::RichText::new(&b.name).font(crate::ui::theme::title()).color(IVORY).strong());
        }
    }
    // [free-placement] A city building reads Civic (its ground is out of the zoning).
    let shown = w.shown_use(id);
    let zone_col = crate::ui::theme::rgb_f(assets.palette.zone_color(shown));
    let style = nd_procgen::building_style(w, id, &assets.rules, Some(&sim.styles));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, shown.label(), zone_col);
        if let Some(bz) = &b.business {
            tag(ui, &capitalise(bz.kind.label()), GOLD);
            if let Some(ind) = bz.kind.industry() {
                tag(ui, ind.label(), egui::Color32::from_gray(150));
            }
        } else if let Some(s) = b.service {
            tag(ui, &capitalise(s.kind.label()), crate::ui::theme::SERVICE_BLUE);
        } else if let Some(k) = b.plant {
            tag(ui, &capitalise(k.label()), crate::ui::detail::plant_color(k));
        } else if b.depot {
            tag(ui, "Bus depot", GOLD);
        } else if b.homes > 0 {
            tag(ui, "Homes", egui::Color32::from_gray(170));
        }
        // [attractions] A landmark comes in one size: no tier to show.
        if !b.service.is_some_and(|s| s.kind.is_landmark()) {
            tag(ui, &format!("{} tier", b.tier.label()), egui::Color32::from_gray(130));
        }
        if let Some(d) = w.district_of_lot(b.lot) {
            tag(ui, &d.name, egui::Color32::from_rgb(200, 140, 230));
        }
    });
    // [pt-growth] (F25) While the old building comes down the fields
    // already describe its successor: say which is which, so a hut being
    // pulled down does not read as "9 floors".
    // [w1] With the planned height, and on a plain demolition whatever is
    // planned for the plot.
    let next = w.site_plan(id).and_then(|p| p.next);
    match &b.demolition {
        Some(d) if d.clear_lot => {
            let then = next.as_ref().map_or(String::new(), |n| format!("  ·  then {}{}, {} floor{}, about {:.0} m", if n.joins { "part of " } else { "" }, n.name, n.floors, crate::ui::fmt::s(n.floors), n.height_m));
            hint(ui, &format!("coming down: {} floor{} {} {}{then}", d.floors, crate::ui::fmt::s(d.floors), d.tier.label().to_lowercase(), d.land_use.label().to_lowercase()));
        }
        Some(d) => hint(ui, &format!("coming down: {} floor{} {} {}  ·  then {}, {} floor{}, about {:.0} m", d.floors, crate::ui::fmt::s(d.floors), d.tier.label().to_lowercase(), d.land_use.label().to_lowercase(), style.map_or("a new building", |s| s.label()), b.floors, crate::ui::fmt::s(b.floors), w.building_height(id))),
        None => {
            let started = if b.construction.is_some() { "started" } else { "built" };
            hint(ui, &format!("{}  ·  {} floor{}  ·  {started} day {}", style.map_or("unknown style", |s| s.label()), b.floors, crate::ui::fmt::s(b.floors), fmt::day(b.built_tick)));
        }
    }
    // [pt-walk] Cut off from the rest of the city on foot or by road.
    for line in w.lot_link_warnings(b.lot) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&line).color(egui::Color32::from_rgb(255, 130, 80)).strong());
            // [walk-networks] Which places it is joined to on foot.
            if line.contains("pedestrian") {
                crate::map_layers::walk_networks_button(ui);
            }
        });
    }
    // [feat/pt-ux] What it is and what it does for the city, first.
    if let Some((what, gives)) = super::about::about(w, id) {
        egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(40, 34, 20, 140)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(what).color(IVORY));
            ui.label(egui::RichText::new(gives).small().color(egui::Color32::from_gray(190)));
        });
    }
    // [p2-look] A mall's shops, anchors, food court, owner, car park and
    // reach; a site grown over the plots next door.
    // [megablocks] An arcology: its stage, or what is inside and its tallies.
    if let Some(t) = super::site::megablock_section(ui, w, id) {
        *selection = Selection::Building(t);
        return;
    }
    if let Some(t) = super::site::mall_section(ui, w, id) {
        *selection = Selection::Building(t);
        return;
    }
    super::site::joined_site_section(ui, w, id);
    // [free-placement] The city's own: its ground is out of the zoning, and
    // the player may bulldoze it (a second click confirms).
    if w.is_city_building(id) && b.rail.is_none() && b.port.is_none() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("City ground, out of the zoning").small().weak());
            let armed = ui.ctx().data(|d| d.get_temp::<bool>(bulldoze)).unwrap_or(false);
            if !armed {
                if ui.small_button("Bulldoze").on_hover_text("Pull it down; its ground goes back to whatever the zoning plan has there").clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(bulldoze, true));
                }
            } else {
                if ui.small_button(egui::RichText::new("Confirm bulldoze").color(WARN)).clicked() {
                    ui.ctx().data_mut(|d| {
                        d.remove::<bool>(bulldoze);
                        d.insert_temp(bulldoze.with("go"), true);
                    });
                }
                if ui.small_button("Cancel").clicked() {
                    ui.ctx().data_mut(|d| d.remove::<bool>(bulldoze));
                }
            }
            if let Some(err) = ui.ctx().data(|d| d.get_temp::<String>(bulldoze.with("err"))) {
                ui.label(egui::RichText::new(format!("Not bulldozed: {err}")).small().color(WARN));
            }
        });
    }
    // [events] A rave on tonight (or the scene round an old warehouse).
    crate::ui::nights::rave_line(ui, w, id);
    // Alarms, before anything else.
    if w.services.is_burning(id) {
        ui.label(egui::RichText::new("ON FIRE").strong().color(egui::Color32::from_rgb(255, 110, 60)));
    }
    if b.is_derelict() {
        ui.label(egui::RichText::new("DERELICT").strong().color(WARN));
    }
    // [pt-growth] A demolition: why, how far, and when the lot is clear.
    if let Some(d) = &b.demolition {
        let ready = w.clock.after_work_hours(d.work_hours_left());
        ui.add(egui::ProgressBar::new(d.progress).desired_height(18.0).fill(egui::Color32::from_rgb(190, 90, 60)).text(format!("BEING DEMOLISHED {:.0}%, clear about day {} {}", d.progress * 100.0, fmt::day(ready), fmt::hhmm(ready))));
        let then = match next.as_ref() {
            _ if !d.clear_lot => format!("then the new building goes up: {} floor{}, about {:.0} m", b.floors, crate::ui::fmt::s(b.floors), w.building_height(id)),
            Some(n) if n.joins => format!("then part of {}: {} floor{}, about {:.0} m", n.name, n.floors, crate::ui::fmt::s(n.floors), n.height_m),
            Some(n) => format!("then {}: {} floor{}, about {:.0} m", n.name, n.floors, crate::ui::fmt::s(n.floors), n.height_m),
            None => "then a vacant lot (nothing planned yet)".to_string(),
        };
        ui.label(egui::RichText::new(format!("{}  ·  {then}  ·  wrecking crew {}/{}  ·  rubble {} of {} loads hauled  ·  crews work 08:00-17:00", capitalise(d.reason.label()), b.workers.len(), b.jobs, d.loads, nd_sim::Demolition::loads_for(d.floors))).small());
    }
    if let Some(c) = b.construction.as_ref().filter(|_| b.demolition.is_none()) {
        let (homes, jobs) = b.planned_capacity();
        // [pt-growth] Sites work the working day only: when it should be done.
        let ready = w.clock.after_work_hours(c.work_hours_left());
        let left = format!("done about day {} {}", fmt::day(ready), fmt::hhmm(ready));
        let status = if c.blocked.is_some() { ", on hold" } else if c.waiting_for_materials() { if c.ordered { ", waiting for materials (truck on its way)" } else { ", waiting for materials" } } else { "" };
        // [r2-traffic] A site no truck can get to says so (K6).
        if let Some(k) = c.blocked {
            ui.label(egui::RichText::new(format!("! {}: the site waits until a road leads here", capitalise(k.text()))).strong().color(egui::Color32::from_rgb(255, 130, 80)));
        }
        ui.add(egui::ProgressBar::new(c.progress).desired_height(18.0).fill(egui::Color32::from_rgb(200, 150, 60)).text(format!("UNDER CONSTRUCTION {:.0}%, {left}{status}", c.progress * 100.0)));
        // [w1] What it will be when it tops out.
        if let Some(n) = next.as_ref() {
            ui.label(egui::RichText::new(format!("Planned: {} floor{}, about {:.0} m high when finished", n.floors, crate::ui::fmt::s(n.floors), n.height_m)).strong().color(egui::Color32::from_rgb(120, 220, 245)));
        }
        ui.label(egui::RichText::new(format!("{} of {} deliveries in  ·  {:.1} days of work in all  ·  site crew {}/{}  ·  will have {} homes and {} jobs", c.deliveries, nd_sim::Construction::DELIVERIES, c.days, b.workers.len(), b.jobs, homes, jobs)).small());
    }
    if b.plant.is_none() && b.powered < 0.98 && b.construction.is_none() {
        ui.label(egui::RichText::new(format!("UNDERPOWERED: {:.0}% of {:.1} MW, {} dark hours today", b.powered * 100.0, nd_sim::power::demand_mw(b), b.unpowered_hours_today)).color(WARN));
    }
    // [dirty-industry] Dirty itself, dirty neighbours, poisoned ground.
    let pick = dirt_lines(ui, w, id);
    if let Some(src) = pick {
        *selection = Selection::Building(src);
        return;
    }

    // [supertall] height, rank, and a league tower's record, up top
    crate::ui::skyline::building_section(ui, w, id, selection, &sim.building_tops, &assets.rules);

    // ── The business ──
    if let Some(bz) = &b.business {
        section(ui, if bz.closed { "BUSINESS  ·  CLOSED" } else { "BUSINESS" });
        ui.horizontal_wrapped(|ui| {
            figure(ui, "cash", format!("{:.0}", bz.money), if bz.money < 0.0 { WARN } else { IVORY });
            figure(ui, "today in", format!("+{:.0}", bz.revenue_today), GOOD);
            figure(ui, "today out", format!("-{:.0}", bz.spend_today), WARN);
            figure(ui, "staff", format!("{}/{}", b.workers.len(), b.jobs), IVORY);
        });
        cashflow(ui, &bz.books, (bz.revenue_today, bz.spend_today));
        // [ledger] Its rent and its account.
        if bz.rent_day > 0.0 {
            hint(ui, &format!("rent {:.0}/day for {:.0} m² to {}", bz.rent_day, w.business_area(id), crate::remote::ledger::account_name(w, w.landlord_of(w.host_of(id)))));
        }
        crate::ui::ledger::account_section(ui, w, nd_sim::bank::Account::Business(id), "account");
        // [relocate] A business that lost its old building to a buyout.
        if let Some(from) = &bz.moved_from {
            ui.label(egui::RichText::new(format!("moved from {from}")).small().color(GOLD));
        }
        // [economy] why it is here and how it is doing
        crate::ui::pressure::business_story(ui, w, id);
        let recipe = bz.kind.recipe();
        if !recipe.inputs.is_empty() || !recipe.outputs.is_empty() {
            egui::CollapsingHeader::new(egui::RichText::new(format!("stock  ·  {} deals so far", bz.deals)).small()).id_salt(("inspector_stock", id)).show(ui, |ui| {
                for (r, _) in recipe.inputs {
                    ui.label(egui::RichText::new(format!("needs {}: {:.0} in stock (target {:.0})", r.label(), bz.stock_of(*r), bz.target_input(*r, b.jobs))).small());
                }
                for (r, _) in recipe.outputs {
                    ui.label(egui::RichText::new(format!("sells {}: {:.0} in stock at {:.1}", r.label(), bz.stock_of(*r), bz.sell_price(*r, b.jobs))).small());
                }
            });
        }
    }

    // ── [tenants] A tenant's building, a building's units ──
    if let Some(host) = b.unit_of.filter(|h| w.buildings.contains_key(*h)) {
        let h = &w.buildings[host];
        let place = h.units.iter().filter(|u| u.occupant == nd_sim::Occupant::Tenant(id)).map(|u| u.place()).collect::<Vec<_>>().join(", ");
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("Rents {place} of")).small());
            if ui.link(egui::RichText::new(&h.name).small()).clicked() {
                *selection = Selection::Building(host);
            }
        });
    }
    // [p2-look] A mall's units are in its own section above.
    if !b.units.is_empty() && !b.plot.form.is_some_and(|f| f.is_mall()) {
        let vacant = b.vacant_units();
        section(ui, &format!("UNITS  ·  {} let, {} to let", b.units.len() - vacant, vacant));
        let mut pick = None;
        egui::Grid::new(("inspector_units", id)).num_columns(4).spacing([8.0, 2.0]).show(ui, |ui| {
            for u in &b.units {
                ui.label(egui::RichText::new(format!("{}{}", if u.shopfront { "shop · " } else { "" }, u.place())).small());
                ui.label(egui::RichText::new(format!("{:.0} m²", u.area)).small().weak());
                ui.label(egui::RichText::new(format!("{} job{}", u.jobs, crate::ui::fmt::s(u.jobs))).small().weak());
                match u.occupant {
                    nd_sim::Occupant::Vacant => {
                        let days = w.clock.tick.saturating_sub(u.vacant_since) / 1440;
                        ui.label(egui::RichText::new(format!("to let, {days} day{}", crate::ui::fmt::s(days))).small().color(WARN));
                    }
                    nd_sim::Occupant::Main => {
                        ui.label(egui::RichText::new(b.business.as_ref().map_or("its own", |z| z.kind.label())).small().color(GOLD));
                    }
                    nd_sim::Occupant::Tenant(t) => match w.buildings.get(t) {
                        Some(tb) => {
                            let shut = tb.business.as_ref().is_some_and(|z| z.closed);
                            let text = format!("{} ({}){}", tb.name, tb.business.as_ref().map_or("", |z| z.kind.label()), if shut { ", shut" } else { "" });
                            if ui.link(egui::RichText::new(text).small()).clicked() {
                                pick = Some(t);
                            }
                        }
                        None => {
                            ui.label(egui::RichText::new("?").small());
                        }
                    },
                }
                ui.end_row();
            }
        });
        if let Some(t) = pick {
            *selection = Selection::Building(t);
            return;
        }
    }

    // ── Homes: only where people live and there is no business ──
    if b.business.is_none() && b.homes > 0 {
        section(ui, "HOMES");
        ui.horizontal_wrapped(|ui| {
            figure(ui, "homes let", format!("{}/{}", b.households.len(), b.homes), IVORY);
            figure(ui, "residents", format!("{}", b.residents.len()), IVORY);
            figure(ui, "rent/day", format!("{:.0}", b.rent), IVORY);
        });
    } else if b.business.is_none() && b.jobs > 0 && b.construction.is_none() {
        section(ui, "JOBS");
        ui.label(format!("{}/{} filled  ·  wage {:.0}/day", b.workers.len(), b.jobs, b.wage()));
    }

    // ── What the place does, where it applies ──
    service_and_transit(ui, w, id, b);
    if let Some(lot) = w.lots.get(b.lot) {
        if let Some(park) = &lot.park {
            section(ui, "PARK");
            let features: Vec<&str> = park.features.iter().map(|f| f.kind.label()).collect();
            ui.label(format!("{} benches, {} paths, {} entrances{}; part of {}-lot park of {:.0} m²", park.benches.len(), park.paths.len(), park.entrances.len(), if features.is_empty() { String::new() } else { format!(", {}", features.join(", ")) }, nd_sim::names::a(&park.cluster_lots.to_string()), park.cluster_area));
            let sitting = w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Sitting { park: p, .. } if Some(p) == lot.building)).count();
            // [benches] Seats held (sitting or on the way) out of all of them.
            let (seats, held) = w.seats_in(b.lot);
            if sitting > 0 || held > 0 {
                ui.label(format!("{sitting} sitting on the benches  ·  {held} of {seats} seats taken{}", if seats > 0 && held >= seats { "  ·  FULL" } else { "" }));
            }
        }
    }
    if b.parking_capacity > 0 {
        ui.label(format!("parking {}/{} spaces{}", b.parking_used, b.parking_capacity, if b.is_parking() { format!("  ·  {:.1} per visit", b.parking_price) } else { "  ·  for residents and workers".to_string() }));
    }
    crate::ui::places::building_place(ui, w, id);
    // [land] owner, price, plots and the owner's other properties
    crate::ui::estate::building_section(ui, w, id, selection);

    // ── People ──
    let inside: Vec<nd_sim::CitizenId> = w.citizens.iter().filter(|(_, c)| c.inside == Some(id)).map(|(cid, _)| cid).collect();
    let groups: [(&str, &[nd_sim::CitizenId]); 3] = [("LIVE HERE", &b.residents), ("WORK HERE", &b.workers), ("IN THE BUILDING NOW", &inside)];
    for (title, ids) in groups {
        if ids.is_empty() {
            continue;
        }
        section(ui, &format!("{title}  ·  {}", ids.len()));
        crate::ui::portrait::grid(ui, w, ids, 18, selection);
    }

    // ── The site, last and folded: figures the header does not need ──
    egui::CollapsingHeader::new(egui::RichText::new("site").small()).id_salt(("inspector_site", id)).show(ui, |ui| {
        ui.label(egui::RichText::new(format!("land value {:.2}  ·  rent {:.0}/day  ·  wage {:.0}/day  ·  condition {:.0}%", b.land_value, b.rent, b.wage(), b.condition * 100.0)).small());
        ui.label(egui::RichText::new(format!("visitors {}  ·  power {:.1} MW ({:.0}% served)", b.visitors, nd_sim::power::demand_mw(b), b.powered * 100.0)).small());
        if let Some(lot) = w.lots.get(b.lot) {
            ui.label(egui::RichText::new(format!("foot traffic today {}  ·  this hour {}{}", lot.footfall_today, lot.footfall_hour, if lot.car_access() { "" } else { "  ·  on a footpath: cars stop at the street, residents park nearby" })).small());
        }
    });
}

/// Services, plants, rail, metro and depots: what the building does for
/// the city, one line each, only where it applies.
/// [attractions] A landmark's card: open now or when, who came, what the
/// door took, how it lifts the land round it and draws tourists, and
/// (the pier, the observatory) its site.
fn attraction_lines(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId, b: &nd_sim::Building, k: nd_sim::ServiceKind) {
    section(ui, "ATTRACTION");
    let hour = w.clock.hour() as u8;
    let hours = k.hours();
    let open = b.operating() && hours.contains(hour) && b.workers.len() * 4 >= b.jobs as usize;
    let state = if b.construction.is_some() || b.demolition.is_some() {
        "being built".to_string()
    } else if open {
        format!("open now ({})", crate::ui::help::hours_label(hours))
    } else if !b.operating() {
        "shut".to_string()
    } else if b.workers.len() * 4 < b.jobs as usize {
        format!("shut: {} of {} staff", b.workers.len(), b.jobs)
    } else {
        format!("closed now; open {}", crate::ui::help::hours_label(hours))
    };
    ui.label(format!("{state}  ·  {} visitors in now, {} at once at most  ·  staff {}/{}", b.visitors, k.capacity(nd_sim::Tier::Mid), b.workers.len(), b.jobs));
    let book = w.attractions.book(id);
    let fee = if k.admission() > 0.0 { format!("admission {:.0}", k.admission()) } else { "free to enter".to_string() };
    ui.label(format!("today: {} townsfolk, {} tourists, {:.0} taken  ·  yesterday: {} townsfolk, {} tourists, {:.0} taken  ·  {fee}", book.visits_today, book.tourists_today, book.takings_today, book.visits_yesterday, book.tourists_yesterday, book.takings_yesterday));
    let (r, wt) = k.land_value(nd_sim::Tier::Mid);
    let lift = wt * (0.55 + 0.45 * b.staffing());
    let here = w.lots.get(b.lot).map_or(0.0, |l| w.land_value.at(l.center));
    hint(ui, &format!("Lifts land value up to +{lift:.2} (raw) within {r:.0} m at this staffing; land here reads {here:.2}. Draws {:.0} points of tourist pull ({:.0} now).", k.tourist_pull(), k.tourist_pull() * (0.5 + 0.5 * b.staffing())));
    if let Some(d) = w.attractions.piers.get(&id) {
        hint(ui, &format!("The deck runs {:.0} m out over the sea on piles, {:.0} m wide, with a {:.0} m head for the rides.", d.length(), d.width, d.head));
    }
    if k == nd_sim::ServiceKind::Observatory {
        if let Some(rise) = w.lots.get(b.lot).and_then(|l| w.rise_above_town(l.center)) {
            hint(ui, &format!("Stands {rise:+.0} m from the middle of town."));
        }
    }
}

pub(super) fn service_and_transit(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId, b: &nd_sim::Building) {
    if let Some(s) = b.service {
        let units: Vec<&nd_sim::Unit> = w.services.units.values().filter(|u| u.station == id).collect();
        let mut line = format!("{}  ·  {:.0}/day", s.kind.tier_label(s.tier), s.kind.upkeep_per_day(s.tier));
        if s.kind.radius(s.tier) > 0.0 {
            line += &format!("  ·  covers {:.0} m {}", s.kind.radius(s.tier), if s.kind == nd_sim::ServiceKind::TrashDepot { "around" } else { "by road" });
        }
        if !units.is_empty() {
            line += &format!("  ·  {} {}s, {} crewed, {} on calls, {} on patrol", units.len(), units[0].kind.label(), w.crewed_units(id), units.iter().filter(|u| u.busy()).count(), units.iter().filter(|u| u.state == nd_sim::services::UnitState::Patrol).count());
        }
        if s.kind == nd_sim::ServiceKind::Health {
            line += &format!("  ·  beds {}/{}", w.services.admissions.iter().filter(|a| a.hospital == id).count(), s.kind.capacity(s.tier));
        }
        section(ui, "SERVICE");
        ui.label(egui::RichText::new(line).color(crate::ui::theme::SERVICE_BLUE));
        // [games] A stadium's game day.
        if s.kind == nd_sim::ServiceKind::Stadium {
            use nd_sim::world::games::{GATES_OPEN, KICKOFF};
            let hm = |m: u64| format!("{:02}:{:02}", m / 60, m % 60);
            let today = w.services.events_today.contains(&id);
            let text = match w.game_phase(id) {
                Some(p) => format!("Game day: {}  ·  {} away fans' cars parked", p.label(), w.game_cars_parked(id)),
                None if today && w.clock.minute_of_day() < GATES_OPEN => format!("Game day: gates open {}, kick-off {}", hm(GATES_OPEN), hm(KICKOFF)),
                None if today => "Game day: the game is over".to_string(),
                None => "No game today".to_string(),
            };
            ui.label(egui::RichText::new(text).color(if today { egui::Color32::from_rgb(255, 200, 60) } else { egui::Color32::GRAY }));
        }
        if s.kind.is_landmark() {
            attraction_lines(ui, w, id, b, s.kind);
        }
        // [r5-trash] where the trash goes
        if s.kind.is_waste() || s.kind == nd_sim::ServiceKind::TrashDepot {
            crate::ui::trash::inspector_lines(ui, w, id, s.kind);
        }
    }
    if let Some(kind) = b.plant {
        // [plant-groups] A Sunfield or Wind Park: the whole park's figures.
        if let Some(g) = w.plant_group_of(id) {
            let members: Vec<&nd_sim::Building> = g.members.iter().filter_map(|m| w.buildings.get(*m)).collect();
            let cap: f32 = members.iter().map(|m| w.plant_capacity_mw(m)).sum();
            let now: f32 = members.iter().map(|m| w.plant_dispatched_mw(m)).sum();
            let upkeep: f32 = members.iter().map(|m| w.plant_upkeep(m)).sum();
            section(ui, if kind == nd_sim::PowerPlantKind::Wind { "WIND PARK" } else { "SUNFIELD" });
            ui.label(format!("{now:.1} MW now of {cap:.0} MW nameplate, {upkeep:.0}/day upkeep"));
            hint(ui, &format!("{} lots, {:.0} m² together; +{:.0}% for the shared inverters and grid connection", g.members.len(), g.area, (nd_sim::world::plant_groups::group_bonus(g.members.len()) - 1.0) * 100.0));
            for m in &g.members {
                if let Some(mb) = w.buildings.get(*m) {
                    let area = w.lots.get(mb.lot).map_or(0.0, |l| l.area());
                    hint(ui, &format!("{}{}: {:.0} m², {:.1} MW", if *m == id { "> " } else { "  " }, mb.name, area, w.plant_capacity_mw(mb)));
                }
            }
        } else {
            section(ui, "POWER PLANT");
        }
        ui.label(format!("{}{:.1} MW nameplate, {:.0}% available now, {:.0}/day upkeep", if w.plant_group_of(id).is_some() { "this lot: " } else { "" }, w.plant_capacity_mw(b), w.plant_availability(b) * 100.0, w.plant_upkeep(b)));
        // [grid-weather] The site's wind or sun, and what the farm runs at.
        if let Some(l) = w.lots.get(b.lot).filter(|_| matches!(kind, nd_sim::PowerPlantKind::Wind | nd_sim::PowerPlantKind::Solar)) {
            hint(ui, &crate::ui::tools::site_note(w, kind, l.center).replace("Under the pointer: ", "Site: "));
            if kind == nd_sim::PowerPlantKind::Wind {
                let share = w.plant_dispatched_mw(b) / w.plant_capacity_mw(b).max(1e-3);
                let v = w.weather.wind_ms() * w.plant_resource(b).wind;
                let state = if v >= nd_sim::weather::CUT_OUT_MS { "braked: the wind is past cut-out".to_string() } else if v < nd_sim::weather::CUT_IN_MS { "becalmed".to_string() } else { format!("running at {:.0}%", share * 100.0) };
                hint(ui, &format!("wind here now {v:.1} m/s: {state}"));
            }
        }
        if kind.sized_when_built() {
            // [plant-scale] Fixed when it was built (1 on older saves').
            match b.plant_scale {
                Some(s) => hint(ui, &format!("built for its {:.0} m² site: {:.2}× a standard {}", w.lots.get(b.lot).map_or(0.0, |l| l.area()), s, kind.label())),
                None => hint(ui, &format!("a standard {} (built before plants were sized to their site)", kind.label())),
            }
        } else if kind.reference_area().is_some() {
            hint(ui, &format!("sized to its lot: {:.0} m², {:.2}× a standard {}", w.lots.get(b.lot).map_or(0.0, |l| l.area()), w.plant_scale(b), kind.label()));
        }
        // [dams] (H1) Its dam.
        if kind == nd_sim::PowerPlantKind::Hydro {
            match w.dams.wall_of(id) {
                Some(wall) => hint(ui, &format!("dam wall {:.0} m from bank to bank, a footpath along its crest (no roads on a dam)", wall.length())),
                None if w.dams.is_pending(id) => {
                    ui.label(egui::RichText::new("no dam wall yet: it makes no power until one is drawn").color(egui::Color32::from_rgb(255, 170, 80)));
                }
                None => hint(ui, "the old automatic dam across the river beside it (a wall can be drawn to replace it)"),
            }
        }
        if b.demolition.is_some() {
            ui.label(egui::RichText::new("clearing the lot: not on the grid yet").color(egui::Color32::from_rgb(255, 170, 80)));
        }
    } else if let Some(bat) = &b.battery {
        // [power] a grid battery: charge, what it is doing, its limits
        let k = bat.kind;
        section(ui, "BATTERY");
        let doing = if bat.flow_mw > 0.05 { format!("delivering {:.1} MW", bat.flow_mw) } else if bat.flow_mw < -0.05 { format!("charging {:.1} MW", -bat.flow_mw) } else { "holding".to_string() };
        ui.label(format!("{}: {:.0} of {:.0} MWh ({:.0}%), {doing}; up to {:.0} MW, {:.0}/day upkeep", k.label(), bat.charge_mwh, k.capacity_mwh(), bat.level() * 100.0, k.max_mw(), k.upkeep_per_day()));
        ui.add(egui::ProgressBar::new(bat.level()).desired_height(10.0).fill(crate::ui::detail::BATTERY.gamma_multiply(0.8)));
        // [batteries] The rule it runs by this hour, and what it saved.
        let pw = &w.economy.power;
        let bands = pw.price_bands;
        ui.label(egui::RichText::new(format!("{} (power {:.2}/MWh now; cheap at {:.2} or less, dear from {:.2})", pw.battery_mode.label(), pw.import_price, bands.cheap, bands.dear)).small());
        let sign = |v: f32| if v >= 0.0 { format!("saved {v:.0}") } else { format!("cost {:.0}", -v) };
        ui.label(egui::RichText::new(format!("{} today, {} yesterday", sign(bat.savings_today), sign(bat.savings_yesterday))).small().color(if bat.savings_yesterday >= 0.0 { GOOD } else { WARN }));
        if let Some(log) = w.economy.history.plant(id).filter(|l| !l.level.is_empty()) {
            let hours = crate::ui::pane::hours(&w.economy.history);
            let n = log.level.len().min(hours.len());
            if n >= 2 {
                let labels = crate::ui::pane::hour_labels(&hours[hours.len() - n..]);
                let price: Vec<f32> = hours[hours.len() - n..].iter().map(|r| r.import_price * 10.0).collect();
                let series = [
                    crate::ui::pane::Series::new("stored MWh", crate::ui::detail::BATTERY, log.level.iter().skip(log.level.len() - n).copied().collect()),
                    crate::ui::pane::Series::new("flow MW (+ out)", egui::Color32::from_rgb(255, 190, 80), log.hours.iter().skip(log.hours.len() - n).copied().collect()),
                    crate::ui::pane::Series::new("price x10", egui::Color32::from_rgb(200, 120, 230), price),
                ];
                crate::ui::pane::line_chart(ui, &labels, &series, 80.0, &|x| format!("{x:.1}"));
                crate::ui::pane::legend(ui, &series.iter().map(|s| (s.name.as_str(), s.color)).collect::<Vec<_>>());
                hint(ui, "State of charge over the last hours against the grid price: it fills in the cheap hours (and on any surplus) and empties in the dear evening ones.");
            }
        }
    }
    // [dock-access] A dock or station off the roads: say so, loud, and what to do.
    if let (Some(problem), Some(true)) = (w.access_problem(id), w.footpath_only(id)) {
        // (D7) A footpath only: people walk in, goods need a road.
        section(ui, "ROAD ACCESS");
        ui.label(egui::RichText::new("FOOTPATH ONLY: GOODS NEED A ROAD").color(egui::Color32::from_rgb(255, 170, 80)).strong());
        hint(ui, &crate::ui::fmt::capitalise(&problem));
    } else if let Some(problem) = w.access_problem(id) {
        section(ui, "ROAD ACCESS");
        ui.label(egui::RichText::new("NOT CONNECTED TO THE ROAD NETWORK").color(egui::Color32::from_rgb(255, 170, 80)).strong());
        let todo = if w.stub_ends_of(id).is_empty() { "Its street is joined to roads that reach neither the city nor a way out of town: carry them on to the network." } else { "Draw a road to the marked end of its street stub (the amber ring); it snaps on like any road end." };
        hint(ui, todo);
        hint(ui, problem.split("; ").nth(1).unwrap_or(""));
    } else if let Some(note) = w.access_note(id) {
        // [sweep-sim] Walk-only: it works for passengers; say what a road would add.
        section(ui, "ROAD ACCESS");
        ui.label(egui::RichText::new("ON FOOT ONLY").color(egui::Color32::from_rgb(150, 210, 255)).strong());
        hint(ui, &note);
    }
    // [fishing] The port's trawlers, what each is doing, and its grounds.
    // [sweep-visual] And a fishery's trawler off its own jetty.
    if let Some(f) = w.fleet(id).filter(|f| f.jetty || b.port.is_some_and(|k| k.is_fishing())) {
        section(ui, "FISHING FLEET");
        ui.label(format!("{} boats, {} at sea · {:.0} fish landed today, {:.0} yesterday · {} trips today", f.boats.len(), f.at_sea(), f.landed_today, f.landed_yesterday, f.trips_today));
        if let Some(why) = &f.kept_in {
            hint(ui, &format!("kept in today: {why}"));
        }
        if f.grounds.is_empty() {
            hint(ui, "no open water within reach for fishing grounds: the crews fish off the quay");
        }
        for r in w.fishing_boats(id) {
            let mut line = format!("{}: {}", r.name, r.phase.label());
            if let Some(g) = r.ground {
                line += &format!(" (ground {})", g + 1);
            }
            if r.phase.at_sea() {
                line += &format!(" · {:.0} fish aboard after {} min", r.hold, r.fished_min);
                if let Some(h) = r.home_by {
                    line += &format!(" · home by {:02}:{:02}", h / 60, h % 60);
                }
            }
            line += &format!(" · {:.0} landed today", r.catch_today);
            ui.label(egui::RichText::new(line).small());
        }
        if let Some(port) = w.ports.port_of(id) {
            let grounds: Vec<String> = f
                .grounds
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    let foul = w.water_fouling(g.center);
                    let clean = if foul > 0.02 { format!(", water {:.0}% fouled", foul * 100.0) } else { String::new() };
                    format!("{}: {:.1} km out, {:.0}% rich{clean}", i + 1, g.center.distance(port.berth) / 1000.0, g.richness * 100.0)
                })
                .collect();
            if !grounds.is_empty() {
                hint(ui, &format!("grounds (shown on the Water survey layer): {}", grounds.join(" · ")));
            }
        }
    }
    // [ferries] [ferry-routes] Its routes: where to, the boats, the next one.
    crate::ui::ferry::dock_routes(ui, w, id);
    // [waterfront] its edge on the water, a beach, a private jetty
    crate::ui::shore::lines(ui, w, id);
    if let Some(link) = &b.rail {
        let joined = if link.walk_only() { " · on foot only (no road yet)" } else if link.works() { "" } else if !link.rail_connected { " · NOT CONNECTED TO THE RAILWAY" } else { " · NOT CONNECTED TO THE ROADS" };
        section(ui, "RAIL");
        match link.kind {
            nd_sim::RailFacility::Station => ui.label(format!("railway station: platform {:.0} m, {} trains today, {} riders today across the line{joined}", link.platform[0].distance(link.platform[1]), w.rail.stats.passenger_trains_today, w.rail.stats.riders_today)),
            nd_sim::RailFacility::Central => ui.label(format!("Central Station: intercity platform {:.0} m, {} trains today, {} rail riders today; metro below, bus terminal in front{joined}", link.platform[0].distance(link.platform[1]), w.rail.stats.passenger_trains_today, w.rail.stats.riders_today)),
            nd_sim::RailFacility::FreightYard => {
                // [freight] Orders wait out of town for a train; then they ride in on it.
                let (orders, units) = w.rail.orders_for(id);
                let aboard = w.rail.aboard_for(id);
                ui.label(format!("freight yard: {orders} orders ({units:.0} units) waiting out of town for the next train, {aboard:.0} units on their way by train, {:.0} units of exports on the apron{joined}", b.rail_outbound))
            }
        };
    }
    // [subway]
    if b.metro {
        let (today, yesterday) = w.subway.boardings_at(id);
        let lines: Vec<String> = w.subway.lines_at(id).map(|(_, l)| format!("{} (every {} min)", l.name, l.headway)).collect();
        let waiting = w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(nd_sim::transit::TransitLeg::Subway { board, .. }) if board == id)).count();
        section(ui, "METRO");
        ui.label(if lines.is_empty() { "no line calls here yet (Build window, Transit, Metro lines)".to_string() } else { format!("{} · {today} boarded today, {yesterday} yesterday · {waiting} on the platform", lines.join(", ")) });
    }
    // [cable] A cable car station on its lot or its roof.
    if w.cable.stations.values().any(|s| s.building == Some(id)) {
        section(ui, "CABLE CAR");
        crate::cable_ux::building_card(ui, w, id);
    }
    if b.depot {
        let out = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|x| x.depot == id)).count();
        section(ui, "BUS DEPOT");
        ui.label(format!("room for {} buses, {} out on the lines, {:.0}/day", nd_sim::transit::DEPOT_CAPACITY, out, nd_sim::transit::DEPOT_UPKEEP_PER_DAY));
        depot_lines(ui, w, id);
    }
}

/// The cash-flow sparkline: a pair of bars per day, takings up in green and
/// spending down in red, with the net as a gold line; today, still open, is
/// the last pair, drawn fainter. Hover a day for its figures.
pub(super) fn cashflow(ui: &mut egui::Ui, books: &[nd_sim::market::DayBook], today: (f32, f32)) {
    let days: Vec<(f32, f32)> = books.iter().map(|d| (d.revenue, d.spend)).chain(std::iter::once(today)).collect();
    if days.len() < 2 {
        hint(ui, "cash flow: the first day's books close at midnight");
        return;
    }
    let width = ui.available_width().min(320.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 54.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6));
    let top = days.iter().map(|d| d.0.max(d.1)).fold(1.0f32, f32::max);
    let mid = rect.center().y;
    let half = rect.height() * 0.5 - 2.0;
    let n = days.len();
    let step = rect.width() / n as f32;
    let bar = (step * 0.7).clamp(1.0, 9.0);
    p.hline(rect.x_range(), mid, egui::Stroke::new(1.0, egui::Color32::from_gray(70)));
    let mut net: Vec<egui::Pos2> = Vec::with_capacity(n);
    for (i, (rev, spend)) in days.iter().enumerate() {
        let x = rect.left() + step * (i as f32 + 0.5);
        let fade = if i + 1 == n { 0.45 } else { 1.0 };
        let up = half * rev / top;
        let down = half * spend / top;
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x - bar / 2.0, mid - up), egui::pos2(x + bar / 2.0, mid)), 0.0, GOOD.gamma_multiply(fade));
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x - bar / 2.0, mid), egui::pos2(x + bar / 2.0, mid + down)), 0.0, WARN.gamma_multiply(fade));
        net.push(egui::pos2(x, mid - half * (rev - spend) / top));
    }
    p.add(egui::Shape::line(net, egui::Stroke::new(1.5, GOLD)));
    if let Some(pos) = resp.hover_pos() {
        let i = (((pos.x - rect.left()) / step) as usize).min(n - 1);
        let (rev, spend) = days[i];
        let when = if i + 1 == n { "today so far".to_string() } else if i + 2 == n { "yesterday".to_string() } else { format!("{} days ago", n - 1 - i) };
        resp.on_hover_text(format!("{when}: took {rev:.0}, spent {spend:.0}, net {:+.0}", rev - spend));
    }
    let closed = &days[..n - 1];
    let total: f32 = closed.iter().map(|d| d.0 - d.1).sum();
    hint(ui, &format!("cash flow, last {} day{}: net {total:+.0}  (green in, red out, gold net)", closed.len(), crate::ui::fmt::s(closed.len())));
}

/// [dirty-industry] What dirty industry means for this building: a dirty
/// plant says how far it fouls and how many neighbours suffer; a sufferer
/// lists its unhappy neighbours, worst first, with what they cost it
/// ("Ironmaw Smelter 120 m · food output −30%, land value −0.09"); and the
/// ground's contamination when there is any. Returns a neighbour clicked.
pub(super) fn dirt_lines(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) -> Option<nd_sim::BuildingId> {
    let b = &w.buildings[id];
    let mut clicked = None;
    if let Some(src) = w.dirty.iter().find(|s| s.building == id) {
        let hurt = w.buildings.iter().filter(|(o, x)| *o != id && x.construction.is_none() && w.building_exposure(*o) > 0.05 && w.building_lot(*o).is_some_and(|l| l.center.distance(src.pos) < src.reach)).count();
        let spill = b.business.as_ref().and_then(|z| z.kind.spill()).map(|(r, _)| r).or_else(|| b.plant.and_then(|k| k.ground_spill()).map(|(r, _)| r)).unwrap_or(0.0);
        ui.label(egui::RichText::new(format!("DIRTY INDUSTRY: fouls the ground within {spill:.0} m; neighbours within {:.0} m suffer ({hurt} now)", src.reach)).small().color(egui::Color32::from_rgb(210, 170, 90)));
        hint(ui, "farms, fishing, homes, shops and clean tech hate it; manufacturing and logistics shrug");
    }
    let neighbours = w.unhappy_neighbours(id);
    if !neighbours.is_empty() {
        section(ui, "UNHAPPY NEIGHBOURS");
        for n in neighbours.iter().take(4) {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if ui.link(egui::RichText::new(&n.name).small().color(WARN)).on_hover_text(format!("Select the {}", n.what)).clicked() {
                    clicked = Some(n.source);
                }
                ui.label(egui::RichText::new(format!("{} · {:.0} m · hit {:.0}%", n.what, n.distance, n.hit * 100.0)).small());
            });
        }
        if neighbours.len() > 4 {
            hint(ui, &format!("and {} more", neighbours.len() - 4));
        }
        if let Some(cost) = w.adjacency_cost(id) {
            ui.label(egui::RichText::new(format!("Costs it: {cost}")).small().color(WARN));
        }
    }
    if let Some(l) = w.lots.get(b.lot) {
        let c = w.contamination.over(l.center, &l.polygon);
        if c >= 0.02 {
            let col = if c > 0.3 { WARN } else { egui::Color32::from_rgb(200, 200, 120) };
            // What the soil costs this farm (`World::industry_hourly`).
            use nd_sim::BusinessKind as K;
            let share = match b.business.as_ref().map(|z| z.kind) {
                Some(K::GrainFarm) => 0.9,
                Some(K::CowFarm | K::Dairy) => 0.7,
                Some(K::VertFarm | K::Fishery | K::ChickenFarm) => 0.5,
                _ => 0.0,
            };
            let what = if share > 0.0 { format!(" · output −{:.0}%", c * share * 100.0) } else { String::new() };
            ui.label(egui::RichText::new(format!("Ground contamination {:.0}%{what}", c * 100.0)).small().color(col));
            // [dirty-industry] How to get rid of it.
            match w.cleanups.iter().find(|j| j.center.distance(l.center) <= j.radius) {
                Some(j) => hint(ui, &format!("a clean-up crew is digging here: {:.0}% done", j.progress() * 100.0)),
                None => hint(ui, "Bulldoze (B) → Clean up ground digs it out"),
            }
        }
    }
    clicked
}

/// [lines] F1: a depot's lines. Buses go out from the working depot with
/// room nearest a line's first stop, so these are the lines it sends buses
/// to now and the ones it is nearest to; each opens in the Lines window.
fn depot_lines(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) {
    let depots: Vec<(nd_sim::BuildingId, nd_sim::Vec2)> = w.buildings.iter().filter(|(_, b)| b.working_depot()).filter_map(|(d, b)| w.lots.get(b.lot).map(|l| (d, l.car_point))).collect();
    let mut rows: Vec<(nd_sim::RouteId, &nd_sim::transit::BusRoute, usize, bool)> = Vec::new();
    for (rid, r) in &w.transit.routes {
        let from_here = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|b| b.route == rid && b.depot == id)).count();
        let nearest = r.stops.first().and_then(|s| depots.iter().min_by(|a, b| a.1.distance(s.pos).total_cmp(&b.1.distance(s.pos)))).is_some_and(|(d, _)| *d == id);
        if from_here > 0 || nearest {
            rows.push((rid, r, from_here, nearest));
        }
    }
    if rows.is_empty() {
        hint(ui, if w.transit.routes.is_empty() { "No bus lines yet: the Lines window (top bar) or the Buses tool draws one." } else { "No line runs from here: buses leave from the depot nearest each line's first stop." });
        return;
    }
    ui.label(egui::RichText::new("LINES FROM HERE").small().weak());
    for (rid, r, from_here, nearest) in rows {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.5, crate::ui::lines::line_color(r.color));
            if ui.small_button(&r.name).on_hover_text("Open the line in the Lines window").clicked() {
                crate::ui::lines::request_pick(ui.ctx(), rid);
            }
            let what = match (from_here, nearest) {
                (0, _) => format!("its depot ({} buses wanted)", r.buses),
                (k, true) => format!("{k} of its {} buses out from here", r.buses),
                (k, false) => format!("{k} bus{} out from here (an overflow)", if k == 1 { "" } else { "es" }),
            };
            ui.label(egui::RichText::new(format!("{} stops · {what} · {} riders today", r.stops.len(), r.riders_today)).small());
        });
    }
}
