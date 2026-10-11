//! The inspector on a vehicle: a moving car, bus or truck and a parked
//! car, with the journey, driver, passengers, tow and why-it-stands cards.

use super::*;

/// A moving vehicle: a bus, a truck or a car, its driver, who is aboard,
/// why it stands and the trip so far.
pub(super) fn car(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, id: nd_sim::CarId) {
    let w = &sim.world;
    let Some(car) = w.traffic.cars.get(id) else {
        *selection = Selection::None;
        return;
    };
    let mut driver_shown = false;
    if let Some(bus) = &car.bus {
        let line = w.transit.routes.get(bus.route).map(|r| r.name.clone()).unwrap_or_else(|| "a line that was removed".into());
        neon(ui, &format!("Bus on {line}"));
        let going = if bus.to_depot || bus.next_stop == usize::MAX { "heading back to the depot".to_string() } else { format!("next stop {}", bus.next_stop + 1) };
        ui.label(format!("{} passenger{}  ·  {going}  ·  {:.0} km/h{}", bus.passengers.len(), crate::ui::fmt::s(bus.passengers.len()), car.v * 3.6, if car.hold > 0.0 { "  ·  at a stop" } else { "" }));
        if ui.small_button("Open the line in the Lines window").clicked() {
            crate::ui::lines::request_pick(ui.ctx(), bus.route);
        }
    } else if let Some(cargo) = &car.cargo {
        neon(ui, "Delivery truck");
        let name = |b| w.buildings.get(b).map(|b| b.name.as_str()).unwrap_or("out of town");
        ui.label(format!("{:.0} {} from {} to {}  ·  worth {:.0}  ·  {:.0} km/h", cargo.amount, cargo.resource.label(), name(cargo.from), name(cargo.to), cargo.price, car.v * 3.6));
    } else {
        // [clarity] A city vehicle says what it is and whose it is.
        let unit = w.services.unit_of_car(id).and_then(|u| w.services.units.get(u));
        match unit {
            Some(u) => neon(ui, &format!("{} {} {}  ·  {}", capitalise(u.kind.label()), u.kind.call_sign(), u.number, w.buildings.get(u.station).map_or("", |b| b.name.as_str()))),
            None => neon(ui, &format!("Car  ·  {}", car.model.body.label().replace('_', " "))),
        }
        ui.label(format!("{:.0} km/h", car.v * 3.6));
        // [events] A street racer: the race and their place.
        crate::ui::nights::racer_line(ui, w, id);
        // [moto] A pack rider.
        crate::ui::nights::moto_line(ui, w, id);
        driver_shown = driver_card(ui, w, id, selection);
        // (Crews and strangers are named by the journey card below.)
        // [boot] Clamp it: it pulls over and waits for the tow truck.
        if car.owned.is_some() && tow_button(ui) {
            let owned = car.owned;
            match sim.world.order_tow_car(id) {
                Ok(_) => {
                    if let Some(v) = owned {
                        *selection = Selection::Parked(v);
                    }
                }
                Err(e) => tow_refused(ui, e),
            }
            return;
        }
    }
    // [airlift] Stuck for good? A heavy-lift helicopter takes it away.
    if airlift_row(ui, sim, id) {
        return;
    }
    let w = &sim.world;
    let Some(car) = w.traffic.cars.get(id) else { return };
    // [clarity] Where from, where to, why, and who is driving.
    if journey_card(ui, w, id, selection) {
        return;
    }
    // [services] A wreck waiting for its tow, or the truck coming.
    if tow_status(ui, w, id, selection) {
        return;
    }
    // Everyone aboard but the driver, as faces that select them.
    let riders: Vec<nd_sim::CitizenId> = occupants(w, id).into_iter().filter(|c| *c != car.citizen || !driver_shown).collect();
    if !riders.is_empty() {
        section(ui, &format!("{}  ·  {}", if car.bus.is_some() { "ON BOARD" } else { "PASSENGERS" }, riders.len()));
        crate::ui::portrait::grid(ui, w, &riders, 24, selection);
    } else if driver_shown {
        hint(ui, "alone in the car");
    }
    why_and_trip(ui, w, id, selection, !driver_shown);
}

/// A parked car: where and how long, its owner, a clamp or the tow button.
pub(super) fn parked(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, vid: nd_sim::VehicleId) {
    let Some(owned) = sim.world.vehicles.cars.get(vid) else {
        *selection = Selection::None;
        return;
    };
    let Some(spot) = owned.spot().copied() else {
        *selection = Selection::None;
        return;
    };
    neon(ui, &format!("Parked {}", owned.model.body.label().replace('_', " ")));
    let place = spot.building.and_then(|b| sim.world.buildings.get(b)).map(|b| format!(" at {}", b.name)).unwrap_or_default();
    let hours = sim.world.clock.tick.saturating_sub(spot.since) / 60;
    ui.label(format!("{}{place}  ·  parked {hours} h", spot.kind.label()));
    let owner = owned.owner;
    if let Some(name) = sim.world.citizens.get(owner).map(|c| c.name.clone()) {
        if ui.button(format!("Owner: {name}")).clicked() {
            *selection = Selection::Citizen(owner);
        }
    }
    // [boot] Clamped and waiting, or the button to have it done.
    if spot.booted {
        let now = sim.world.clock.tick;
        let truck = sim.world.enforcement.job_for(vid).map(|j| match j.state {
            // [clamp] An officer's clamp: the fine or the tow truck.
            nd_sim::enforcement::TowState::Waiting { until } if j.reason == nd_sim::enforcement::TowReason::Unpaid => format!("fine {:.0} unpaid  ·  towed in {}", nd_sim::enforcement::CLAMP_FINE, fmt::hours_min_long(until.saturating_sub(now))),
            _ if j.truck.is_some() => "the tow truck is on its way".to_string(),
            _ if j.company.is_none() => "a tow truck is coming from out of town".to_string(),
            _ => "waiting for a tow truck".to_string(),
        }).unwrap_or_else(|| "waiting for a tow truck".to_string());
        ui.label(egui::RichText::new(format!("Clamped  ·  {truck}")).color(egui::Color32::from_rgb(255, 140, 20)));
    } else if spot.kind.on_street() && tow_button(ui) {
        if let Err(e) = sim.world.order_tow(vid) {
            tow_refused(ui, e);
        }
    }
    show_tow_refusal(ui);
}

/// Everyone in a vehicle: the driver of a private car first, then its
/// riders. Riders are a bus's passenger list and anyone whose activity says
/// they ride in it; a private car's own passenger list, when the sim keeps
/// one, belongs here too, so the inspector and its grid need no change.
pub fn occupants(w: &nd_sim::World, id: nd_sim::CarId) -> Vec<nd_sim::CitizenId> {
    let Some(car) = w.traffic.cars.get(id) else { return Vec::new() };
    let mut out: Vec<nd_sim::CitizenId> = Vec::new();
    if car.bus.is_none() && w.citizens.contains_key(car.citizen) {
        out.push(car.citizen);
    }
    if let Some(bus) = &car.bus {
        out.extend(bus.passengers.iter().copied());
    }
    for (cid, c) in &w.citizens {
        if matches!(c.activity, nd_sim::Activity::Riding { bus } if bus == id) && !out.contains(&cid) {
            out.push(cid);
        }
    }
    out
}

/// [clarity] "coming from [place] → going to [place]" (each a button that
/// selects it), what the vehicle is doing, and who is at the wheel when
/// that is not a driver card of its own: a city vehicle's crew (citizens
/// on the station's payroll, as faces), or an unknown person from out of
/// the city. True when the selection changed.
pub(super) fn journey_card(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) -> bool {
    let Some(j) = w.car_journey(id) else { return false };
    let mut picked: Option<Selection> = None;
    ui.add_space(2.0);
    ui.label(egui::RichText::new(capitalise(&j.purpose)).font(crate::ui::theme::lead()).color(egui::Color32::from_rgb(120, 220, 255)).strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("coming from").small().weak());
        if crate::ui::person::place_button(ui, &j.from) {
            picked = j.from.building.map(Selection::Building).or(j.from.car.map(Selection::Car));
        }
        match &j.to {
            Some(t) => {
                ui.label(egui::RichText::new("— going to").small().weak());
                if crate::ui::person::place_button(ui, t) {
                    picked = t.building.map(Selection::Building).or(t.car.map(Selection::Car));
                }
            }
            None => {
                ui.label(egui::RichText::new(format!("— {}", j.purpose)).small().weak());
            }
        }
    });
    match &j.driver {
        nd_sim::world::Driver::Crew(crew) => {
            section(ui, if crew.len() == 1 { "OFFICER" } else { "CREW" });
            crate::ui::portrait::grid(ui, w, crew, 24, selection);
        }
        nd_sim::world::Driver::Unknown(who) => {
            ui.label(egui::RichText::new(format!("At the wheel: {who}")).small().weak());
        }
        nd_sim::world::Driver::Citizen(_) => {}
    }
    match picked {
        Some(s) => {
            *selection = s;
            true
        }
        None => false,
    }
}

/// Why someone is on the road, in a few words.
pub(super) fn purpose_label(p: &nd_sim::citizens::Purpose) -> String {
    use nd_sim::citizens::Purpose as P;
    match p {
        P::Home => "going home".into(),
        P::Work => "going to work".into(),
        P::Visit(lu) => format!("out to the {}", lu.label().to_lowercase()),
        P::Shop(r) => format!("shopping for {}", r.label()),
        P::Wander => "wandering".into(),
        P::Sit => "off to sit in a park".into(),
        P::ToStop => "heading for a stop".into(),
    }
}

/// The driver of a private car under the car's own details: a face that
/// selects them, who they are, why and where they are driving, the drive
/// so far against what it should take, their temper, and where they sit
/// between slow and reckless. False when nobody known is at the wheel.
pub(super) fn driver_card(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) -> bool {
    let Some(car) = w.traffic.cars.get(id) else { return false };
    let Some(c) = w.citizens.get(car.citizen) else { return false };
    let cid = car.citizen;
    section(ui, "DRIVER");
    let mut pick = false;
    ui.horizontal(|ui| {
        pick |= crate::ui::portrait::button(ui, c, 52.0).clicked();
        ui.vertical(|ui| {
            pick |= ui.add(egui::Label::new(egui::RichText::new(&c.name).color(IVORY).strong()).sense(egui::Sense::click())).on_hover_text("Select the driver").clicked();
            ui.label(egui::RichText::new(format!("{}  ·  {} class  ·  mood {:.0}%", c.age, c.class().label(), c.mood * 100.0)).small());
            let (to, purpose) = match &c.activity {
                nd_sim::Activity::Driving { to, purpose, .. } => (*to, Some(purpose)),
                _ => (None, None),
            };
            let dest = to.and_then(|b| w.buildings.get(b)).map(|b| b.name.as_str());
            let line = match (purpose, dest) {
                (Some(p), Some(d)) => format!("{} · to {d}", purpose_label(p)),
                (Some(p), None) => purpose_label(p),
                (None, Some(d)) => format!("to {d}"),
                (None, None) => c.activity.label().to_string(),
            };
            ui.label(egui::RichText::new(line).small());
        });
    });
    if pick {
        *selection = Selection::Citizen(cid);
    }
    if let Some((elapsed, expected)) = w.car_trip_minutes(id) {
        let over = elapsed / expected.max(1.0);
        let col = if over > 2.0 { WARN } else if over > 1.3 { GOLD } else { egui::Color32::LIGHT_GRAY };
        ui.label(egui::RichText::new(format!("On the road {:.0} min  ·  should take {:.0} min", elapsed, expected)).color(col));
    }
    let d = &car.drive;
    if d.calm.is_some() {
        let word = match d.anger {
            a if a >= 0.75 => "raging",
            a if a >= 0.5 => "fuming",
            a if a >= 0.25 => "losing patience",
            a if a > 0.02 => "getting irritated",
            _ => "calm",
        };
        // An empty bar still paints a dot of fill; calm is just said.
        if d.anger > 0.02 {
            ui.add(egui::ProgressBar::new(d.anger).desired_width(220.0).desired_height(18.0).fill(egui::Color32::from_rgb(200, 60, 40)).text(format!("anger: {word}  ·  {:.0}%", d.anger * 100.0)));
        } else {
            ui.label(egui::RichText::new(format!("{word} at the wheel")).small());
        }
    }
    // The driver as they are now (anger included), on the slider; their
    // calm self is marked too when anger has moved them.
    temperament(ui, d.profile.boldness(), d.calm.map(|p| p.boldness()).filter(|b| (b - d.profile.boldness()).abs() > 0.03));
    ui.label(egui::RichText::new(format!("{}  ·  skill {:.0}%  ·  {:.0}% of the limit  ·  {:.1} s gap{}", d.profile.label(), d.profile.skill * 100.0, d.profile.speed * 100.0, d.profile.headway, if d.profile.drunk > 0.05 { format!("  ·  drunk {:.0}%", d.profile.drunk * 100.0) } else { String::new() })).small().weak());
    true
}

/// The slow–normal–reckless slider (read only): a track from blue through
/// grey to red with a marker at `boldness` (`-1..1`), and a hollow one at
/// the driver's calm self when anger has moved them.
pub(super) fn temperament(ui: &mut egui::Ui, boldness: f32, calm: Option<f32>) {
    let width = ui.available_width().min(260.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let track = egui::Rect::from_min_max(egui::pos2(rect.left() + 6.0, rect.top() + 6.0), egui::pos2(rect.right() - 6.0, rect.top() + 12.0));
    let slow = egui::Color32::from_rgb(90, 150, 255);
    let normal = egui::Color32::from_gray(150);
    let reckless = egui::Color32::from_rgb(255, 80, 60);
    let n = 24;
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let col = if t < 0.5 { crate::ui::theme::lerp(slow, normal, t * 2.0) } else { crate::ui::theme::lerp(normal, reckless, t * 2.0 - 1.0) };
        let x0 = track.left() + track.width() * i as f32 / n as f32;
        let x1 = track.left() + track.width() * (i + 1) as f32 / n as f32;
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, track.top()), egui::pos2(x1, track.bottom())), 0.0, col.gamma_multiply(0.7));
    }
    let x_of = |b: f32| track.left() + track.width() * (b.clamp(-1.0, 1.0) + 1.0) / 2.0;
    if let Some(c) = calm {
        p.circle_stroke(egui::pos2(x_of(c), track.center().y), 5.0, egui::Stroke::new(1.5, IVORY));
    }
    p.circle_filled(egui::pos2(x_of(boldness), track.center().y), 6.0, IVORY);
    let font = crate::ui::theme::small();
    let weak = egui::Color32::from_gray(150);
    p.text(egui::pos2(track.left(), rect.bottom()), egui::Align2::LEFT_BOTTOM, "slow", font.clone(), weak);
    p.text(egui::pos2(track.center().x, rect.bottom()), egui::Align2::CENTER_BOTTOM, "normal", font.clone(), weak);
    p.text(egui::pos2(track.right(), rect.bottom()), egui::Align2::RIGHT_BOTTOM, "reckless", font, weak);
    resp.on_hover_text(format!("Driving style {:+.2} (−1 slow and careful, 0 ordinary, +1 reckless): speed, overtaking, ambers and following distance{}", boldness, if calm.is_some() { "; the ring is how they drive when calm" } else { "" }));
}

/// Why the car is standing (or crawling), the car it waits for, the head of
/// the jam it is in, and the drive so far against what it should take.
/// Every car named is a button that selects it.
/// `trip`: also the drive so far and the driver's temper, for vehicles
/// whose driver has no card of their own (buses, trucks, strangers).
pub(super) fn why_and_trip(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection, trip: bool) {
    use nd_sim::traffic::StopReason;
    let t = &w.traffic;
    let Some(car) = t.cars.get(id) else { return };
    let reason = t.stop_reason(id);
    hint(ui, "why");
    let colour = |r: &StopReason| if r.is_good() { egui::Color32::LIGHT_GRAY } else { WARN };
    if reason == StopReason::Moving {
        ui.label("Driving freely");
    } else {
        ui.horizontal_wrapped(|ui| {
            match reason.by() {
                Some(b) => {
                    ui.label(egui::RichText::new(capitalise(reason.lead_in())).color(colour(&reason)));
                    if ui.small_button(w.car_label(b)).on_hover_text("Select this vehicle").clicked() {
                        *selection = Selection::Car(b);
                    }
                }
                None => {
                    ui.label(egui::RichText::new(capitalise(&reason.text(|_| String::new()))).color(colour(&reason)));
                }
            }
        });
        if car.stalled > 1.0 {
            ui.label(egui::RichText::new(format!("standing {}", fmt::secs_or_min(car.stalled))).small().weak());
        }
    }
    let chain = t.jam_chain(id);
    if let Some(head) = chain.head().filter(|_| chain.cars.len() >= 3 || (chain.cars.len() == 2 && chain.cycle)) {
        let hr = t.stop_reason(head);
        ui.add_space(2.0);
        // The front car is rolling: a slow queue, not a blockage.
        let crawling = t.cars.get(head).is_some_and(|h| h.v >= 0.5);
        if chain.cycle {
            ui.label(egui::RichText::new(format!("Gridlock: {} vehicles waiting on each other in a ring", chain.cars.len())).color(WARN));
        } else if crawling {
            ui.label(egui::RichText::new(format!("Queue of {}, moving off again at the front", chain.cars.len() - 1)).strong());
        } else {
            ui.label(egui::RichText::new(format!("Jam: {} vehicles back from the front", chain.cars.len() - 1)).strong());
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(if crawling { "front:" } else { "caused by" });
            if ui.button(w.car_label(head)).on_hover_text("Select the car at the head of the jam").clicked() {
                *selection = Selection::Car(head);
            }
        });
        let speed = t.cars.get(head).map_or(0.0, |h| h.v * 3.6);
        ui.label(egui::RichText::new(format!("— {}  ·  {speed:.0} km/h", hr.text(|c| w.car_label(c)))).color(colour(&hr)));
    }
    if !trip {
        return;
    }
    // [rage] The drive so far.
    if let Some((elapsed, expected)) = w.car_trip_minutes(id) {
        let over = elapsed / expected.max(1.0);
        let col = if over > 2.0 { WARN } else if over > 1.3 { crate::ui::GOLD } else { egui::Color32::LIGHT_GRAY };
        ui.label(egui::RichText::new(format!("On the road {:.0} min  ·  should take {:.0} min", elapsed, expected)).color(col));
    }
    let d = &car.drive;
    if d.calm.is_some() && d.anger > 0.02 {
        let word = match d.anger {
            a if a >= 0.75 => "raging",
            a if a >= 0.5 => "fuming",
            a if a >= 0.25 => "losing patience",
            _ => "getting irritated",
        };
        ui.add(egui::ProgressBar::new(d.anger).desired_width(220.0).fill(egui::Color32::from_rgb(200, 60, 40)).text(format!("Driver {word}  ·  {:.0}%", d.anger * 100.0)));
    }
    ui.label(egui::RichText::new(format!("Driver: {}", d.profile.label())).small().weak());
}

/// [boot] "Tow this car": a wheel clamp now, the tow truck after; the owner
/// pays at the pound.
/// [airlift] The Airlift button (or the helicopter on its way): the
/// emergency service that lifts a vehicle off the road by helicopter, for
/// one stuck where nothing else can shift it. True once called (the car
/// is spoken for: the caller stops drawing the card).
fn airlift_row(ui: &mut egui::Ui, sim: &mut Sim, id: nd_sim::CarId) -> bool {
    let w = &sim.world;
    if w.airlifts().iter().any(|a| a.car == Some(id)) {
        ui.label(egui::RichText::new("The airlift helicopter is on its way to lift it off the road.").color(egui::Color32::from_rgb(255, 200, 60)));
        return false;
    }
    if w.traffic.cars.get(id).is_some_and(|c| c.bus.is_some() || c.siren) {
        return false;
    }
    let mut called = false;
    ui.horizontal(|ui| {
        let b = ui.button(egui::RichText::new(format!("Airlift ({:.0})", nd_sim::world::airlift::AIRLIFT_COST)).color(egui::Color32::from_rgb(255, 200, 60)))
            .on_hover_text("Emergency service: a twin-rotor heavy-lift helicopter flies in, slings this vehicle and flies it away (a resident's car to the pound). For a vehicle stuck where nothing else can shift it.");
        if b.clicked() {
            match sim.world.airlift(id) {
                Ok(()) => called = true,
                Err(e) => {
                    let until = ui.ctx().input(|i| i.time) + 4.0;
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("airlift_refused"), (e, until)));
                }
            }
        }
    });
    let now = ui.ctx().input(|i| i.time);
    if let Some((why, until)) = ui.ctx().data(|d| d.get_temp::<(String, f64)>(egui::Id::new("airlift_refused"))) {
        if now < until {
            ui.label(egui::RichText::new(format!("Can't airlift: {why}")).color(WARN).small());
        }
    }
    let _ = called;
    false
}

pub(super) fn tow_button(ui: &mut egui::Ui) -> bool {
    ui.button(egui::RichText::new("Tow this car").color(egui::Color32::from_rgb(255, 140, 20)))
        .on_hover_text("Clamp it where it stands (a moving car pulls over first) and call the tow truck; the owner pays to get it back from the pound")
        .clicked()
}

/// Why the last tow order was refused, shown under the button for a while.
pub(super) fn tow_refused(ui: &egui::Ui, why: String) {
    let until = ui.ctx().input(|i| i.time) + 4.0;
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("tow_refused"), (why, until)));
}

pub(super) fn show_tow_refusal(ui: &mut egui::Ui) {
    let now = ui.ctx().input(|i| i.time);
    if let Some((why, until)) = ui.ctx().data(|d| d.get_temp::<(String, f64)>(egui::Id::new("tow_refused"))) {
        if now < until {
            ui.label(egui::RichText::new(format!("Can't tow: {why}")).color(WARN).small());
        }
    }
}

/// [services] A written-off car: how long it has waited for the tow, and
/// once a recovery truck is on its way, "truck inbound" with a button that
/// selects it (the truck's road to the wreck is drawn while the wreck is
/// selected, `tools::draw_selection`). A recovery truck: the wreck it is
/// going to. True when the selection changed.
pub(super) fn tow_status(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) -> bool {
    let tick = w.clock.tick;
    if let Some(wr) = w.driving.wrecks.iter().find(|x| x.car == id) {
        section(ui, "TOW");
        let waited = tick.saturating_sub(wr.since);
        match wr.truck.and_then(|t| w.traffic.cars.get(t).map(|c| (t, c))) {
            Some((t, truck)) => {
                let d = w.traffic.cars.get(id).map_or(0.0, |c| c.pos.distance(truck.pos));
                ui.label(egui::RichText::new(format!("Truck inbound  ·  {d:.0} m away  ·  sent {} ago", fmt::hours_min(tick.saturating_sub(wr.sent)))).color(WARN));
                ui.horizontal_wrapped(|ui| {
                    hint(ui, if wr.company.is_some() { "from a tow company in town" } else { "from out of town" });
                    if ui.small_button("locate").on_hover_text("Select the tow truck").clicked() {
                        *selection = Selection::Car(t);
                    }
                });
                if *selection == Selection::Car(t) {
                    return true;
                }
            }
            None => {
                let called = if wr.due > 0 { "called, finding a truck" } else { "waiting for the police to call it in" };
                ui.label(egui::RichText::new(format!("Waiting for a tow  ·  {}", fmt::hours_min(waited))).color(WARN));
                hint(ui, called);
            }
        }
        if wr.truck.is_some() {
            hint(ui, &format!("waiting {} so far", fmt::hours_min(waited)));
        }
        if wr.flipped > 0 {
            let how = if wr.flipped == 1 { "on its side" } else { "on its roof" };
            hint(ui, &if wr.righted_at > 0 { format!("{how}: the crew is righting it") } else { format!("{how}: the truck will right it before towing") });
        }
        return false;
    }
    let Some(car) = w.traffic.cars.get(id) else { return false };
    if let nd_sim::Errand::Recover(wreck) = car.errand {
        section(ui, "RECOVERY");
        ui.horizontal_wrapped(|ui| {
            ui.label(if car.drive.recovering { "Amber lights, nearly there, going round the queue:" } else { "Amber lights, on the way to" });
            if ui.small_button(w.car_label(wreck)).on_hover_text("Select the wreck").clicked() {
                *selection = Selection::Car(wreck);
            }
        });
        return *selection == Selection::Car(wreck);
    }
    false
}
