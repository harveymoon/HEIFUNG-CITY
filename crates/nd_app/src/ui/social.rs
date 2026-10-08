//! [social] People's lives together in the HUD: a person's friends, rivals,
//! groups and recent conversations in their pane; the People window's
//! Social tab (the day's friendships and feuds, every crew and club with
//! its members and what it made, petitions to the player, the news); the
//! toast for social news; and speech bubbles over people talking when the
//! camera is close.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_sim::social::{Bond, Group, Outcome, PetitionState, Setting, Work};
use nd_sim::{BuildingId, CitizenId, World};

use super::pane;
use super::{hint, section, UiCtx, UiFlags, GOLD, IVORY, WARN};
use crate::camera::{CameraMode, OrbitCamera};
use crate::sim::Sim;
use crate::tools::Selection;

/// Every icon the social layer shows (the font test reads this file).
#[allow(dead_code)]
pub const ICONS: [&str; 16] = ["💬", "😂", "❤", "✔", "😠", "💢", "💥", "♪", "⚽", "📖", "🎨", "💡", "🌱", "🏠", "✊", "★"];

/// The colour of a bond.
pub fn bond_color(b: Bond) -> egui::Color32 {
    match b {
        Bond::Partner => egui::Color32::from_rgb(255, 110, 170),
        Bond::CloseFriend => egui::Color32::from_rgb(120, 230, 150),
        Bond::Friend => egui::Color32::from_rgb(150, 210, 170),
        Bond::Acquaintance => egui::Color32::from_rgb(180, 180, 175),
        Bond::Stranger => egui::Color32::from_rgb(140, 140, 140),
        Bond::Rival => egui::Color32::from_rgb(240, 170, 90),
        Bond::Enemy => WARN,
    }
}

fn outcome_color(o: Outcome) -> egui::Color32 {
    match o {
        Outcome::Flirted => egui::Color32::from_rgb(255, 110, 170),
        Outcome::Laughed | Outcome::Agreed => egui::Color32::from_rgb(120, 220, 255),
        Outcome::Favour => egui::Color32::from_rgb(120, 230, 150),
        Outcome::Argued => egui::Color32::from_rgb(240, 170, 90),
        Outcome::Insulted | Outcome::Fight => WARN,
    }
}

fn name(w: &World, c: CitizenId) -> &str {
    w.citizens.get(c).map_or("(gone)", |x| x.name.as_str())
}

fn place_name(w: &World, b: Option<BuildingId>) -> Option<&str> {
    b.and_then(|b| w.buildings.get(b)).map(|x| x.name.as_str())
}

/// What one of a person's pane rows picked.
pub enum Pick {
    Citizen(CitizenId),
    Building(BuildingId),
}

/// A person's people: friends and foes (each a button), their groups, and
/// the last few conversations.
pub fn citizen_social(ui: &mut egui::Ui, w: &World, id: CitizenId) -> Option<Pick> {
    let circle = w.circle_of(id)?;
    let mut pick = None;
    let mut ties: Vec<&nd_sim::social::Tie> = circle.ties.iter().filter(|t| t.bond() != Bond::Stranger).collect();
    ties.sort_by(|a, b| a.bond().rank().cmp(&b.bond().rank()).then(b.affinity.abs().total_cmp(&a.affinity.abs())));
    let friends = circle.friends().count();
    let foes = circle.foes().count();
    if !ties.is_empty() {
        hint(ui, &format!("people ({friends} friends, {foes} rivals or enemies, {} known)", ties.len()));
        for t in ties.iter().take(8) {
            let bond = t.bond();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let col = bond_color(bond);
                let b = ui.add(egui::Button::new(egui::RichText::new(name(w, t.who)).small().color(col)).small().stroke(egui::Stroke::new(1.0, col.gamma_multiply(0.5))));
                let b = b.on_hover_text(format!("affinity {:+.2} · familiarity {:.2} · trust {:.2} · {} talks{}", t.affinity, t.familiarity, t.trust, t.talks, if t.lent > 0.0 { format!(" · owes ${:.0}", t.lent) } else { String::new() }));
                if b.clicked() {
                    pick = Some(Pick::Citizen(t.who));
                }
                let met = match place_name(w, t.met_at) {
                    Some(p) if matches!(t.met, nd_sim::social::Met::Venue | nd_sim::social::Met::Park | nd_sim::social::Met::Bench | nd_sim::social::Met::Work) => format!("met at {p}"),
                    _ => format!("met {}", t.met.label()),
                };
                ui.label(egui::RichText::new(format!("{} · {met} · day {}", bond.label(), t.since / 1440 + 1)).small().color(col.gamma_multiply(0.85)));
            });
        }
    }
    let groups: Vec<&Group> = circle.groups.iter().filter_map(|g| w.social.group(*g)).collect();
    for g in groups {
        let text = egui::RichText::new(format!("{} {} · {} · {} members{}", g.kind.icon(), g.name, g.kind.label(), g.members.len(), if g.founder == id { " · founder" } else { "" })).small().color(GOLD);
        let r = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
        if let Some(b) = g.base.filter(|b| w.buildings.contains_key(*b)) {
            if r.on_hover_text("Select where it meets").clicked() {
                pick = Some(Pick::Building(b));
            }
        }
    }
    if !circle.talks.is_empty() {
        hint(ui, "recent conversations");
        for n in circle.talks.iter().rev().take(4) {
            let at = place_name(w, n.place).map_or_else(
                || match n.setting {
                    Setting::Street => " in the street".to_string(),
                    Setting::Stop => " at the bus stop".to_string(),
                    Setting::Transit => " on the way".to_string(),
                    _ => String::new(),
                },
                |p| format!(" at {p}"),
            );
            let line = format!("{} talked {} with {}{at}: {}", n.outcome.icon(), n.topic.label(), name(w, n.with), n.outcome.label());
            let r = ui.add(egui::Label::new(egui::RichText::new(line).small().color(outcome_color(n.outcome).gamma_multiply(0.9))).sense(egui::Sense::click()));
            if r.on_hover_text("Select them").clicked() {
                pick = Some(Pick::Citizen(n.with));
            }
        }
    }
    pick
}

/// A group's works as short phrases.
fn work_line(w: &World, x: &Work) -> String {
    let b = |id: BuildingId| w.buildings.get(id).map_or("(gone)".to_string(), |b| b.name.clone());
    match x {
        Work::Gig { venue, day } => format!("day {}: played {}", day + 1, b(*venue)),
        Work::Match { park, day } => format!("day {}: a match in {}", day + 1, b(*park)),
        Work::Reading { place, day } => format!("day {}: a reading at {}", day + 1, b(*place)),
        Work::Mural { lot, day } => format!("day {}: a mural on {}", day + 1, w.lots.get(*lot).and_then(|l| l.building).map_or("a wall".to_string(), b)),
        Work::Garden { day, .. } => format!("day {}: planted a community garden", day + 1),
        Work::Business { building, day } => format!("day {}: opened {}", day + 1, b(*building)),
        Work::Petition { id, day } => format!("day {}: petition #{id}", day + 1),
        Work::Protest { day, .. } => format!("day {}: protested", day + 1),
    }
}

/// The People window's Social tab.
pub fn tab(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let s = &w.social;
    let t = &s.today;
    let y = s.yesterday();
    section(ui, "TODAY");
    let vs = |now: u32, then: Option<u32>| then.map_or(format!("{now}"), |x| format!("{now} (yesterday {x})"));
    ui.label(egui::RichText::new(format!("{} conversations · {} warm · {} sour", t.talks, t.good_talks, t.bad_talks)).small());
    ui.label(egui::RichText::new(format!("friendships made {} · close {} · couples {}", vs(t.friendships, y.map(|d| d.friendships)), t.close_friendships, t.couples)).small().color(bond_color(Bond::Friend)));
    ui.label(egui::RichText::new(format!("feuds {} · fights {}", vs(t.feuds, y.map(|d| d.feuds)), t.fights)).small().color(bond_color(Bond::Rival)));
    ui.label(egui::RichText::new(format!("meet-ups {} · home visits {} · jobs through friends {} · flats {} · loans {} · moved in together {}", t.meetups, t.home_visits, t.jobs_found, t.homes_found, t.loans, t.moved_in)).small());
    let fr: Vec<f32> = s.history.iter().rev().take(30).rev().map(|d| d.friendships as f32).chain(std::iter::once(t.friendships as f32)).collect();
    if fr.len() > 1 {
        pane::sparkline(ui, &fr, bond_color(Bond::CloseFriend), 32.0, "friendships made each day, and today so far");
    }
    let counts = s.bond_counts();
    let n = |b: Bond| counts.get(&b).copied().unwrap_or(0) / 2;
    ui.horizontal_wrapped(|ui| {
        for b in [Bond::Partner, Bond::CloseFriend, Bond::Friend, Bond::Rival, Bond::Enemy] {
            ui.label(egui::RichText::new(format!("{} {}", n(b), b.label())).small().color(bond_color(b)));
        }
        hint(ui, "pairs in the city");
    });

    let mut select: Option<Selection> = None;
    let mut fly: Option<nd_sim::Vec2> = None;
    let mut groups: Vec<&Group> = s.active_groups().collect();
    groups.sort_by(|a, b| (b.reputation + b.members.len() as f32 * 0.05).total_cmp(&(a.reputation + a.members.len() as f32 * 0.05)).then(a.id.cmp(&b.id)));
    section(ui, &format!("GROUPS ({})", groups.len()));
    if groups.is_empty() {
        hint(ui, "No crews or clubs yet: friends with a passion in common start them.");
    }
    egui::ScrollArea::vertical().id_salt("social_groups").max_height(260.0).show(ui, |ui| {
        for g in &groups {
            let head = format!("{} {} · {} · {} members · rep {:+.2}", g.kind.icon(), g.name, g.kind.label(), g.members.len(), g.reputation);
            egui::CollapsingHeader::new(egui::RichText::new(head).small().color(IVORY)).id_salt(("group", g.id)).show(ui, |ui| {
                let threshold = g.kind.threshold();
                ui.add(egui::ProgressBar::new((g.momentum / threshold).clamp(0.0, 1.0)).desired_height(6.0)).on_hover_text(format!("momentum {:.2} of {:.1} to make something", g.momentum, threshold));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(egui::RichText::new(format!("since day {} · met {} times", g.founded + 1, g.meetings)).small().weak());
                    if let Some(b) = g.base.filter(|b| w.buildings.contains_key(*b)) {
                        if ui.small_button(format!("meets at {}", w.buildings[b].name)).clicked() {
                            select = Some(Selection::Building(b));
                        }
                    }
                    if let Some(from) = g.split_from.and_then(|x| s.group(x)) {
                        ui.label(egui::RichText::new(format!("split from {}", from.name)).small().weak());
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for m in &g.members {
                        let star = if *m == g.founder { "★ " } else { "" };
                        if ui.small_button(format!("{star}{}", name(w, *m))).clicked() {
                            select = Some(Selection::Citizen(*m));
                        }
                    }
                });
                for x in g.works.iter().rev().take(5) {
                    ui.label(egui::RichText::new(work_line(w, x)).small().color(GOLD));
                    if let Work::Business { building, .. } = x {
                        if w.buildings.contains_key(*building) && ui.small_button("select the business").clicked() {
                            select = Some(Selection::Building(*building));
                        }
                    }
                }
            });
        }
    });

    let open: Vec<&nd_sim::social::Petition> = s.petitions.iter().filter(|x| matches!(x.state, PetitionState::Open | PetitionState::Protested)).collect();
    if !open.is_empty() {
        section(ui, "PETITIONS TO YOU");
        for x in open.iter().rev().take(5) {
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("go").clicked() {
                    fly = Some(x.at);
                }
                let col = if x.state == PetitionState::Protested { WARN } else { GOLD };
                ui.label(egui::RichText::new(format!("{} · {}", x.state.label(), x.text)).small().color(col));
            });
        }
    }
    let murals = s.murals.len();
    let gardens = s.gardens.len();
    if murals + gardens > 0 {
        hint(ui, &format!("{murals} murals · {gardens} community gardens"));
    }
    section(ui, "NEWS");
    if s.news.is_empty() {
        hint(ui, "Nothing yet.");
    }
    for x in s.news.iter().rev().take(8) {
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("go").clicked() {
                fly = Some(x.pos);
            }
            ui.label(egui::RichText::new(format!("{} · {}", super::emergencies::when(x.tick), x.text)).small());
        });
    }
    if let Some(sel) = select {
        *p.selection = sel;
    }
    if let Some(at) = fly {
        p.fly_to = Some(Vec2::new(at.x, at.y));
    }
}

/// Fresh social news as the toast (big things only: groups making
/// something, petitions, protests, splits).
pub fn news_toast(w: &World, flags: &mut UiFlags) {
    let Some(n) = w.social.news.last() else { return };
    if n.seq <= flags.social_seen {
        return;
    }
    flags.social_seen = n.seq;
    if w.clock.tick.saturating_sub(n.tick) <= 30 {
        flags.message = n.text.clone();
    }
}

/// Bubbles show for this long (real seconds) after the talk was first seen.
const BUBBLE_SECS: f64 = 4.0;
/// The camera closer than this (metres) to show them.
const BUBBLE_RANGE: f32 = 320.0;
const MAX_BUBBLES: usize = 40;

/// Speech bubbles over people talking (or over the door of the place they
/// are talking in), a gig's note over the venue, a protest's fist: when the
/// camera is close.
pub fn bubbles(mut contexts: EguiContexts, sim: Res<Sim>, voices: Res<crate::committee::CommitteeRt>, mode: Res<CameraMode>, flags: Res<UiFlags>, cams: Query<(&Camera, &GlobalTransform, &OrbitCamera)>, mut seen: Local<std::collections::BTreeMap<u64, f64>>) -> Result {
    let Some((camera, gt, orbit)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let now = ctx.input(|i| i.time);
    let w = &sim.world;
    let eye = gt.translation();
    let close = *mode == CameraMode::Walk || orbit.distance < BUBBLE_RANGE;
    // Remember when each talk first showed; forget old ones.
    for t in w.social.recent.iter().rev().take(200) {
        seen.entry(t.serial).or_insert(now);
    }
    seen.retain(|_, at| now - *at < BUBBLE_SECS + 1.0);
    if !close || !super::theme::bubbles_on() {
        return Ok(());
    }
    let tick = w.clock.tick;
    // Over the map only, never over the bars.
    let screen = flags.map_rect(ctx.content_rect()).shrink(4.0);
    let mut items: Vec<(f32, egui::Pos2, String, egui::Color32, f32)> = Vec::new();
    let at3 = |p: nd_sim::Vec2, h: f32| Vec3::new(p.x, w.surface_height(p) + h, p.y);
    for t in w.social.recent.iter().rev() {
        let Some(&first) = seen.get(&t.serial) else { continue };
        let age = now - first;
        if age > BUBBLE_SECS || tick.saturating_sub(t.tick) > 90 {
            continue;
        }
        // Outdoors, over the pair as they are now (if still together).
        let mut p = t.pos;
        if t.setting.is_open_air() {
            if let (Some(a), Some(b)) = (w.citizens.get(t.a), w.citizens.get(t.b)) {
                if a.pos.distance(b.pos) < 8.0 {
                    p = (a.pos + b.pos) * 0.5;
                }
            }
        }
        let h = if t.setting.is_open_air() { 2.5 } else { 4.5 };
        let world = at3(p, h);
        let d = world.distance(eye);
        if d > BUBBLE_RANGE * 1.4 {
            continue;
        }
        let Ok(v) = camera.world_to_viewport(gt, world) else { continue };
        let pos = crate::ui::theme::to_points(ctx, v);
        if !screen.contains(pos) {
            continue;
        }
        let text = if t.outcome == Outcome::Fight { format!("{} FIGHT", t.outcome.icon()) } else if d < 90.0 { format!("{} {}", t.outcome.icon(), voices.voices.line(t.serial).unwrap_or(&t.line)) } else { format!("{} {}", t.outcome.icon(), t.topic.label()) };
        let fade = (1.0 - ((age - (BUBBLE_SECS - 1.0)).max(0.0))) as f32;
        items.push((d, pos, text, outcome_color(t.outcome), fade.clamp(0.0, 1.0)));
    }
    // Gigs playing now, protests on.
    for g in w.social.gigs.iter().filter(|g| g.on_at(tick)) {
        let Some(lot) = w.building_lot(g.venue) else { continue };
        let floors = w.buildings.get(g.venue).map_or(2.0, |b| b.floors as f32);
        let world = at3(lot.center, floors * 3.6 + 6.0);
        let Ok(v) = camera.world_to_viewport(gt, world) else { continue };
        let pos = crate::ui::theme::to_points(ctx, v);
        if !screen.contains(pos) {
            continue;
        }
        let band = w.social.group(g.group).map_or("live music", |x| x.name.as_str());
        items.push((0.0, pos, format!("♪ {band} live"), egui::Color32::from_rgb(255, 90, 220), 1.0));
    }
    let day = w.clock.day();
    for x in w.social.petitions.iter().filter(|x| x.state == PetitionState::Protested && x.settled == Some(day)) {
        let Some(r) = x.rally else { continue };
        let here = w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Visiting(b) if b == r)).count();
        if here < 2 {
            continue;
        }
        let Some(lot) = w.building_lot(r) else { continue };
        let Ok(v) = camera.world_to_viewport(gt, at3(lot.center, 9.0)) else { continue };
        let pos = crate::ui::theme::to_points(ctx, v);
        if !screen.contains(pos) {
            continue;
        }
        let who = w.social.group(x.group).map_or("residents", |g| g.name.as_str());
        items.push((0.0, pos, format!("✊ {who}: {here} protesting"), WARN, 1.0));
    }
    if items.is_empty() {
        return Ok(());
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    items.truncate(MAX_BUBBLES);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("social_bubbles")));
    let font = egui::FontId::proportional(12.0);
    // Nearest first take their place; one that would cover another moves
    // up above it (twice at most), else it is left out.
    let mut placed: Vec<(egui::Rect, egui::Pos2, std::sync::Arc<egui::Galley>, egui::Color32, f32)> = Vec::new();
    for (_, pos, text, col, alpha) in items {
        let galley = painter.layout_no_wrap(text, font.clone(), IVORY.gamma_multiply(alpha));
        let size = galley.size() + egui::vec2(12.0, 6.0);
        let mut rect = egui::Rect::from_center_size(pos - egui::vec2(0.0, size.y * 0.5 + 8.0), size);
        let mut tries = 0;
        while placed.iter().any(|p| p.0.expand(1.0).intersects(rect)) && tries < 3 {
            rect = rect.translate(egui::vec2(0.0, -(size.y + 3.0)));
            tries += 1;
        }
        if tries == 3 || !screen.contains_rect(rect) {
            continue;
        }
        placed.push((rect, pos, galley, col, alpha));
    }
    for (rect, pos, galley, col, alpha) in placed.into_iter().rev() {
        let fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, (225.0 * alpha) as u8);
        let stroke = egui::Stroke::new(1.0, col.gamma_multiply(alpha));
        // The tail points down at the speakers (only from the lowest box).
        if rect.bottom() > pos.y - 14.0 {
            painter.add(egui::Shape::convex_polygon(vec![rect.center_bottom() + egui::vec2(-5.0, -1.0), rect.center_bottom() + egui::vec2(5.0, -1.0), pos - egui::vec2(0.0, 1.0)], fill, stroke));
        }
        painter.rect(rect, 6.0, fill, stroke, egui::StrokeKind::Inside);
        painter.galley(rect.min + egui::vec2(6.0, 3.0), galley, IVORY);
    }
    Ok(())
}
