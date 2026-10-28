//! [viewer] `viewer`: drive the building viewer (open it, pick a building
//! or a catalogue style, filter, light it, film the turntable), and
//! `building_notes`: the notes left in it, as work items.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::files::{absolute, allowed_path, unique_stamp};
use super::view::*;
use crate::viewer::notes::{BuildingNotes, Target};
use crate::viewer::{catalogue, building_target, Filters, Source, Viewer};

/// The catalogue and the city rows, built if the viewer was never opened.
fn ready(ctx: &mut Ctx) {
    if ctx.viewer.catalogue.is_empty() {
        ctx.viewer.catalogue = catalogue::entries(&ctx.assets.rules);
    }
    if ctx.sim.full_rebuild {
        ctx.viewer.dirty_rows();
    }
    let Ctx { viewer, sim, assets, .. } = ctx;
    viewer.refresh_rows(sim, assets);
}

fn opt_name<'a>(cmd: &'a Value, name: &str) -> Result<Option<&'a str>, String> {
    Ok(opt_str(cmd, name)?.map(str::trim).filter(|s| !s.is_empty()))
}

/// Set the filters from a `filter` object; `null` or "" clears one.
fn apply_filter(f: &mut Filters, v: &Value) -> Result<(), String> {
    let Some(o) = v.as_object() else { return Err("filter must be an object".into()) };
    let text = |k: &str| -> Option<Option<String>> { o.get(k).map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty() && *s != "any").map(str::to_string)) };
    for (k, _) in o {
        if !["search", "group", "form", "land_use", "district", "story", "business", "tier", "noted"].contains(&k.as_str()) {
            return Err(format!("unknown filter {k:?}: search, group, form, land_use, district, story, business, tier, noted"));
        }
    }
    if let Some(s) = text("search") {
        f.search = s.unwrap_or_default();
    }
    if let Some(g) = text("group") {
        if let Some(g) = &g {
            if !catalogue::GROUPS.iter().any(|x| x.eq_ignore_ascii_case(g)) {
                return Err(format!("unknown group {g:?}: one of {}", catalogue::GROUPS[..catalogue::GROUPS.len() - 1].join(", ")));
            }
        }
        f.group = g.map(|g| catalogue::GROUPS.iter().find(|x| x.eq_ignore_ascii_case(&g)).unwrap().to_string());
    }
    if let Some(x) = text("form") {
        f.form = x;
    }
    if let Some(x) = text("district") {
        f.district = x;
    }
    if let Some(x) = text("story") {
        f.story = x;
    }
    if let Some(x) = text("business") {
        f.business = x;
    }
    if let Some(x) = text("land_use") {
        f.land_use = x.map(|s| land_use_of(&s)).transpose()?;
    }
    if let Some(x) = text("tier") {
        f.tier = x.map(|s| nd_sim::Tier::ALL.into_iter().find(|t| t.label().eq_ignore_ascii_case(&s) || t.key().eq_ignore_ascii_case(&s)).ok_or_else(|| format!("unknown tier {s:?}: low, mid, high"))).transpose()?;
    }
    if let Some(n) = o.get("noted") {
        f.noted = n.as_bool().ok_or("noted must be a bool")?;
    }
    Ok(())
}

/// The list shown, as rows.
fn list_rows(v: &Viewer, notes: &BuildingNotes, game: &str, city: &str, offset: usize, limit: usize) -> (usize, Vec<Value>) {
    match v.source {
        Source::City => {
            let l = v.city_list(notes, game);
            (l.len(), l.iter().skip(offset).take(limit).map(|&i| {
                let r = &v.rows[i];
                json!({ "id": r.key, "name": r.name, "style": r.style_label, "group": r.group, "floors": r.floors, "district": r.district, "open_notes": notes.open_count(&building_target(game, city, r)) })
            }).collect())
        }
        Source::Catalogue => {
            let l = v.catalogue_list(notes);
            (l.len(), l.iter().skip(offset).take(limit).map(|&i| {
                let e = &v.catalogue[i];
                json!({ "style": e.key, "label": e.label, "group": e.group, "land_use": e.land_use.label(), "variants": e.variants.len(), "open_notes": notes.open_count(&Target::Style { style: e.key.clone() }) })
            }).collect())
        }
        Source::Notes => {
            let l = v.notes_list(notes);
            (l.len(), l.iter().skip(offset).take(limit).filter_map(|id| notes.get(*id)).map(|n| n.json()).collect())
        }
    }
}

/// The viewer's state as the reply shows it.
fn status(ctx: &Ctx) -> Value {
    let v = &*ctx.viewer;
    let w = &ctx.sim.world;
    let game = &w.identity.game_id;
    let (at, n) = {
        let (l, at): (usize, Option<usize>) = match v.source {
            Source::City => {
                let l = v.city_list(&ctx.notes, game);
                (l.len(), v.pick.and_then(|p| l.iter().position(|&i| v.rows[i].id == p)))
            }
            Source::Catalogue => {
                let l = v.catalogue_list(&ctx.notes);
                (l.len(), l.iter().position(|&i| i == v.style))
            }
            Source::Notes => {
                let l = v.notes_list(&ctx.notes);
                (l.len(), v.note.and_then(|x| l.iter().position(|&y| y == x)))
            }
        };
        (at, l)
    };
    let target = v.target(&ctx.sim, &ctx.notes);
    let tt = &*ctx.turntable;
    let f = &v.filters;
    json!({
        "open": v.open, "source": v.source.key(),
        "position": at.map(|a| a + 1), "shown": n, "city_buildings": v.rows.len(), "catalogue_styles": v.catalogue.len(),
        "filter": {
            "search": f.search, "group": f.group, "form": f.form, "land_use": f.land_use.map(|l| l.label()), "district": f.district,
            "story": f.story, "business": f.business, "tier": f.tier.map(|t| t.label()), "noted": f.noted,
        },
        "subject": v.subject_json(&ctx.sim, &ctx.notes),
        "note": v.note.and_then(|id| ctx.notes.get(id)).map(|n| n.json()),
        "notes": target.as_ref().map(|t| ctx.notes.on(t).map(|n| n.json()).collect::<Vec<_>>()),
        "view": {
            "night": tt.night, "lights": tt.lights, "auto_rotate": tt.auto_rotate,
            "yaw_deg": tt.yaw.to_degrees().round(), "pitch_deg": tt.pitch.to_degrees().round(), "zoom": (tt.zoom * 100.0).round() / 100.0,
            "size_m": [tt.size.x.round(), tt.size.y.round(), tt.size.z.round()], "ready": tt.settled() > 0,
        },
        "inspiration": v.inspiration.as_ref().map(|p| p.to_string_lossy().replace('\\', "/")),
    })
}

/// `viewer {action?, source?, building?, style?, variant?, note?, step?, filter?, ...}`.
pub(super) fn viewer(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    ready(ctx);
    let game = ctx.sim.world.identity.game_id.clone();
    let city = ctx.sim.world.identity.display_name().to_string();
    let action = opt_name(cmd, "action")?.unwrap_or("status");
    match action {
        "status" | "select" | "list" | "screenshot" => {}
        "open" => ctx.viewer.open = true,
        "close" => ctx.viewer.open = false,
        "toggle" => ctx.viewer.open = !ctx.viewer.open,
        "next" | "prev" => {}
        a => return Err(format!("unknown action {a:?}: status, open, close, toggle, select, next, prev, list, screenshot")),
    }
    if let Some(s) = opt_name(cmd, "source")? {
        ctx.viewer.source = Source::parse(s).ok_or_else(|| format!("unknown source {s:?}: city, catalogue, notes"))?;
    }
    if opt_bool(cmd, "clear_filters")? == Some(true) {
        ctx.viewer.filters = Filters::default();
    }
    if let Some(f) = field(cmd, "filter") {
        apply_filter(&mut ctx.viewer.filters, f)?;
    }
    if let Some(b) = field(cmd, "building") {
        let id: nd_sim::BuildingId = key_from(b)?;
        let w = &ctx.sim.world;
        let b = w.buildings.get(id).ok_or("no such building")?;
        // A tenant is shown as the building it rents in.
        let id = b.unit_of.filter(|h| w.buildings.contains_key(*h)).unwrap_or(id);
        if ctx.viewer.row(id).is_none() {
            ctx.viewer.dirty_rows();
            ready(ctx);
        }
        ctx.viewer.source = Source::City;
        ctx.viewer.pick = Some(id);
        ctx.viewer.scroll_to_pick = true;
    }
    if let Some(s) = opt_name(cmd, "style")? {
        let style = catalogue::style_of(s).ok_or_else(|| format!("unknown style {s:?} (a key like PencilTower or a label like \"pencil tower\")"))?;
        ctx.viewer.source = Source::Catalogue;
        ctx.viewer.style = style.index();
        ctx.viewer.variant = 0;
        ctx.viewer.scroll_to_pick = true;
    }
    if let Some(n) = opt_u64(cmd, "variant")? {
        let len = ctx.viewer.catalogue.get(ctx.viewer.style).map_or(1, |e| e.variants.len());
        if n as usize >= len {
            return Err(format!("variant {n} out of range: this style has {len} (0..{})", len - 1));
        }
        ctx.viewer.variant = n as usize;
    }
    if let Some(n) = opt_u64(cmd, "note")? {
        let id = n as u32;
        ctx.notes.get(id).ok_or_else(|| format!("no note {id}"))?;
        ctx.viewer.source = Source::Notes;
        ctx.viewer.note = Some(id);
        ctx.viewer.scroll_to_pick = true;
    }
    let step = match action {
        "next" => 1,
        "prev" => -1,
        _ => 0,
    } + field(cmd, "step").map(|s| s.as_i64().ok_or("step must be an integer")).transpose()?.unwrap_or(0) as i32;
    if step != 0 {
        let Ctx { viewer, notes, .. } = ctx;
        viewer.step(step, notes, &game);
    }
    // The look.
    if let Some(l) = opt_name(cmd, "light")? {
        ctx.turntable.night = match l {
            "day" => false,
            "night" => true,
            _ => return Err(format!("light is day or night, not {l:?}")),
        };
    }
    if let Some(b) = opt_bool(cmd, "night")? {
        ctx.turntable.night = b;
    }
    if let Some(b) = opt_bool(cmd, "lights")? {
        ctx.turntable.lights = b;
    }
    if let Some(b) = opt_bool(cmd, "auto_rotate")? {
        ctx.turntable.auto_rotate = b;
    }
    if opt_bool(cmd, "reset_view")? == Some(true) {
        ctx.turntable.reset_view();
    }
    if let Some(y) = opt_f32(cmd, "yaw")? {
        ctx.turntable.yaw = y.to_radians();
        ctx.turntable.auto_rotate = false;
    }
    if let Some(p) = opt_f32(cmd, "pitch")? {
        ctx.turntable.pitch = p.to_radians().clamp(-0.05, 1.45);
    }
    if let Some(z) = opt_f32(cmd, "zoom")? {
        ctx.turntable.zoom = z.clamp(0.25, 4.0);
    }
    if let Some(p) = opt_name(cmd, "inspiration")? {
        ctx.viewer.inspiration = if p == "none" {
            None
        } else {
            let all = crate::viewer::window_inspirations();
            let low = p.to_ascii_lowercase().replace('\\', "/");
            Some(all.into_iter().find(|x| x.to_string_lossy().to_ascii_lowercase().replace('\\', "/").ends_with(&low)).ok_or_else(|| format!("no picture {p:?} in the Inspirations folder"))?)
        };
    }
    let mut reply = status(ctx);
    if action == "screenshot" || field(cmd, "path").is_some() {
        let default = format!("screenshots/viewer-{}.png", unique_stamp());
        let path = allowed_path(opt_str(cmd, "path")?.unwrap_or(&default), "png", true)?;
        // The turntable films only while the viewer is open.
        ctx.viewer.open = true;
        ctx.turntable.shots.push(path.clone());
        reply["screenshot"] = json!({ "path": path.to_string_lossy().replace('\\', "/"), "note": "the turntable's image alone, written once the building has stood a few frames (about a second after a change); poll for the file. The window with the viewer: screenshot" });
    }
    if action == "list" {
        let offset = opt_u64(cmd, "offset")?.unwrap_or(0) as usize;
        let limit = opt_u64(cmd, "limit")?.unwrap_or(50).min(1000) as usize;
        let (total, rows) = list_rows(&ctx.viewer, &ctx.notes, &game, &city, offset, limit);
        reply["total"] = json!(total);
        reply["rows"] = json!(rows);
    }
    reply["open"] = json!(ctx.viewer.open);
    Ok(reply)
}

/// A note target from `building` (id) or `style` (key or label).
fn target_of(cmd: &Value, ctx: &mut Ctx) -> Result<Option<Target>, String> {
    if let Some(b) = field(cmd, "building") {
        let id: nd_sim::BuildingId = key_from(b)?;
        let w = &ctx.sim.world;
        let host = w.buildings.get(id).ok_or("no such building")?.unit_of.filter(|h| w.buildings.contains_key(*h)).unwrap_or(id);
        ready(ctx);
        let row = ctx.viewer.row(host).cloned().or_else(|| crate::viewer::city_row(&ctx.sim, &ctx.assets, host)).ok_or("no such building")?;
        let w = &ctx.sim.world;
        return Ok(Some(building_target(&w.identity.game_id, w.identity.display_name(), &row)));
    }
    if let Some(s) = opt_name(cmd, "style")? {
        let style = catalogue::style_of(s).ok_or_else(|| format!("unknown style {s:?}"))?;
        return Ok(Some(Target::Style { style: catalogue::style_key(style) }));
    }
    Ok(None)
}

/// `building_notes {action?, id?, building?, style?, text?, by?, status?}`.
pub(super) fn building_notes(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let action = opt_name(cmd, "action")?.unwrap_or("list");
    let by = opt_name(cmd, "by")?.unwrap_or("claude").to_string();
    let id = opt_u64(cmd, "id")?.map(|n| n as u32);
    let need_id = || id.ok_or_else(|| format!("{action} needs id"));
    let file = ctx.notes.path().map(|p| absolute(&p.to_string_lossy()));
    let one = |ctx: &Ctx, id: u32| -> Result<Value, String> { Ok(json!({ "note": ctx.notes.get(id).ok_or_else(|| format!("no note {id}"))?.json(), "file": file })) };
    match action {
        "list" => {
            let status = opt_name(cmd, "status")?.unwrap_or("open");
            if !["open", "resolved", "all"].contains(&status) {
                return Err(format!("status is open, resolved or all, not {status:?}"));
            }
            let kind = opt_name(cmd, "kind")?;
            let target = target_of(cmd, ctx)?;
            let game = ctx.sim.world.identity.game_id.clone();
            let this_city = opt_bool(cmd, "this_city")?.unwrap_or(false);
            let rows: Vec<Value> = ctx
                .notes
                .book
                .notes
                .iter()
                .filter(|n| match status {
                    "open" => n.is_open(),
                    "resolved" => !n.is_open(),
                    _ => true,
                })
                .filter(|n| kind.is_none_or(|k| n.target.kind() == k))
                .filter(|n| target.as_ref().is_none_or(|t| n.target.same(t)))
                .filter(|n| !this_city || matches!(&n.target, Target::Building { game: g, .. } if *g == game))
                .map(|n| {
                    let mut j = n.json();
                    // Whether the building still stands in the running city.
                    if let Target::Building { game: g, building, .. } = &n.target {
                        j["in_this_city"] = json!(*g == game && crate::viewer::building_of(&ctx.sim.world, building).is_some());
                    }
                    j
                })
                .collect();
            let open = ctx.notes.book.notes.iter().filter(|n| n.is_open()).count();
            Ok(json!({ "notes": rows, "open": open, "total": ctx.notes.book.notes.len(), "file": file }))
        }
        "get" => one(ctx, need_id()?),
        "set" | "add" => {
            let text = opt_str(cmd, "text")?.ok_or("set needs text")?;
            let id = match id {
                Some(id) => {
                    ctx.notes.edit(id, text)?;
                    id
                }
                None => {
                    let target = target_of(cmd, ctx)?.ok_or("a new note needs building (id) or style (key or label); or id to change one")?;
                    ctx.notes.add(target, text, &by)?
                }
            };
            one(ctx, id)
        }
        "reply" => {
            let id = need_id()?;
            ctx.notes.reply(id, opt_str(cmd, "text")?.ok_or("reply needs text")?, &by)?;
            one(ctx, id)
        }
        "resolve" | "reopen" => {
            let id = need_id()?;
            if let Some(t) = opt_str(cmd, "text")?.filter(|t| !t.trim().is_empty()) {
                ctx.notes.reply(id, t, &by)?;
            }
            ctx.notes.resolve(id, action == "resolve", &by)?;
            one(ctx, id)
        }
        "delete" => {
            let id = need_id()?;
            ctx.notes.delete(id)?;
            Ok(json!({ "deleted": id, "file": file }))
        }
        a => Err(format!("unknown action {a:?}: list, get, set, reply, resolve, reopen, delete")),
    }
}
