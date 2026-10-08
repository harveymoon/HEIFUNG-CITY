//! [tools] `intersection`: read and set what the Intersection editor edits
//! (signals, phase greens, the protected left arrow, banned lefts, lane
//! permissions, crosswalk upgrades), and open or close the editor.

use nd_sim::junctions::{CrossingUpgrade, LeftArrow, SignalMode};
use nd_sim::traffic::{arrows_label, parse_arrows};
use nd_sim::{IntersectionView, World};
use serde_json::{json, Value};

use super::view::*;
use crate::tools::ToolState;

/// The editor's view as JSON.
pub fn view_json(w: &World, v: &IntersectionView) -> Value {
    let approaches: Vec<Value> = v
        .approaches
        .iter()
        .enumerate()
        .map(|(i, a)| {
            json!({
                "index": i, "segment": a.segment.0, "street": a.name, "dir": v2(a.dir), "from": v2(v.at + a.dir * (v.radius + 25.0)),
                "lanes": a.arrows.iter().map(|&x| arrows_label(x)).collect::<Vec<_>>(), "offered": arrows_label(a.exits),
                "set_by_player": a.fixed, "left_forbidden": a.no_left, "left_possible": a.left_possible, "queued": a.queued,
            })
        })
        .collect();
    let phases: Vec<Value> = v.phases.iter().enumerate().map(|(k, p)| json!({ "index": k, "approaches": p.approaches, "green_s": round(p.green), "running": p.running })).collect();
    let crossings: Vec<Value> = v
        .crossings
        .iter()
        .map(|c| {
            json!({
                "index": c.index, "mid": v2(c.mid), "ends": [v2(c.ends[0]), v2(c.ends[1])], "upgrade": c.upgrade.label(), "signalised": c.signalised,
                "kind": format!("{:?}", c.kind).to_lowercase(), "button_light": c.light.map(|l| l.label()),
                "footbridge": c.footbridge.map(|(top, foot, width)| json!({ "walkway": [v2(top[0]), v2(top[1])], "stair_feet": [v2(foot[0]), v2(foot[1])], "width": round(width) })),
                "crosswalk_kept": c.kept, "crosswalk_removed": c.closed,
            })
        })
        .collect();
    let r = &v.rule;
    json!({
        "at": v2(v.at), "junction": v.node.is_some(), "node": v.node.map(|n| n.0), "radius": round(v.radius), "roundabout": v.roundabout,
        "signalized": v.signalized,
        "rule": {
            "signal": r.signal.label(), "left_arrow": r.left_arrow.label(), "arrow_seconds": round(r.arrow_seconds),
            "greens": r.greens.iter().map(|(d, s)| json!({ "dir": v2(*d), "seconds": round(*s) })).collect::<Vec<_>>(),
            "no_left": r.no_left.iter().map(|d| v2(*d)).collect::<Vec<_>>(),
        },
        "approaches": approaches, "phases": phases, "crossings": crossings,
        "footbridges": w.walk.footbridges.len(), "button_presses_total": w.peds.button_presses_total,
    })
}

fn approach_index(v: &IntersectionView, sel: &Value) -> Result<usize, String> {
    if let Some(i) = sel.as_u64() {
        return (i as usize).lt(&v.approaches.len()).then_some(i as usize).ok_or_else(|| format!("no approach {i}"));
    }
    let p = vec2_of(sel).map_err(|_| "an approach is its index or a point [x, y] on its street".to_string())?;
    v.approaches
        .iter()
        .enumerate()
        .min_by(|a, b| (v.at + a.1.dir * 30.0).distance(p).total_cmp(&(v.at + b.1.dir * 30.0).distance(p)))
        .map(|(i, _)| i)
        .ok_or_else(|| "no street arrives there".into())
}

/// The crosswalk a command names: `crossing` as a walk-graph index or a
/// point near it, or the only one there.
fn pick_crossing(cmd: &Value, view: &IntersectionView, what: &str) -> Result<u32, String> {
    let k = match field(cmd, "crossing") {
        Some(Value::Number(n)) => n.as_u64().ok_or("crossing: an index")? as u32,
        Some(p) => {
            let p = vec2_of(p)?;
            view.crossings.iter().min_by(|a, b| a.mid.distance(p).total_cmp(&b.mid.distance(p))).map(|c| c.index).ok_or("no crosswalk here")?
        }
        None if view.crossings.len() == 1 => view.crossings[0].index,
        None => return Err(format!("{what} needs \"crossing\": its index or [x,y] (this junction has several)")),
    };
    if !view.crossings.iter().any(|c| c.index == k) {
        return Err(format!("crosswalk {k} is not at this junction"));
    }
    Ok(k)
}

/// `{"cmd":"intersection","at":[x,y], ...}`: with only `at` (or nothing:
/// the editor's junction) it reads; any of the setting fields edits, in
/// this order: `signal`, `greens`, `left_arrow`/`arrow_seconds`,
/// `left_turns`/`no_left`, `lanes`, `crossing`. `open`/`close` show or hide
/// the editor modal.
pub fn command(cmd: &Value, w: &mut World, tools: &mut ToolState) -> Result<Value, String> {
    let at = match opt_vec2(cmd, "at")? {
        Some(p) => p,
        None => tools.junction_editor.as_ref().map(|e| e.at).ok_or("missing field \"at\": [x, y] near a junction or crosswalk (or open the editor first)")?,
    };
    let view = w.intersection_view(at).ok_or("no junction or crosswalk there")?;
    let mut done: Vec<String> = Vec::new();
    // The junction's rule.
    let mut rule = view.rule.clone();
    let mut rule_changed = false;
    if let Some(s) = opt_str(cmd, "signal")? {
        rule.signal = SignalMode::parse(s).ok_or("signal: auto|signals|none")?;
        rule_changed = true;
    }
    if let Some(g) = field(cmd, "greens") {
        match g {
            Value::String(s) if s == "default" || s == "reset" => rule.greens.clear(),
            Value::Array(a) => {
                // Seconds per phase, in the order `phases` lists them (null
                // leaves a phase as it is).
                for (k, x) in a.iter().enumerate() {
                    let Some(secs) = x.as_f64() else { continue };
                    let ph = view.phases.get(k).ok_or_else(|| format!("no phase {k}: the junction has {}", view.phases.len()))?;
                    for &ap in &ph.approaches {
                        if let Some(a) = view.approaches.get(ap) {
                            rule.set_green(a.dir, Some(secs as f32));
                        }
                    }
                }
            }
            _ => return Err("greens: [seconds per phase, ...] or \"default\"".into()),
        }
        rule_changed = true;
    }
    if let Some(s) = opt_str(cmd, "left_arrow")? {
        rule.left_arrow = LeftArrow::parse(s).ok_or("left_arrow: auto|always|never")?;
        rule_changed = true;
    }
    if let Some(s) = opt_f32(cmd, "arrow_seconds")? {
        rule.arrow_seconds = s.max(0.0);
        rule_changed = true;
    }
    if let Some(b) = opt_bool(cmd, "left_turns")? {
        for a in view.approaches.iter().filter(|a| a.left_possible) {
            rule.set_left_forbidden(a.dir, !b);
        }
        rule_changed = true;
    }
    if let Some(list) = field(cmd, "no_left") {
        // [approach, ...]: those approaches forbid the left, the rest allow it.
        let arr = list.as_array().ok_or("no_left: [approach index or [x,y], ...]")?;
        let picked: Vec<usize> = arr.iter().map(|x| approach_index(&view, x)).collect::<Result<_, _>>()?;
        for (i, a) in view.approaches.iter().enumerate() {
            rule.set_left_forbidden(a.dir, picked.contains(&i));
        }
        rule_changed = true;
    }
    if rule_changed {
        if view.node.is_none() {
            return Err("a lone crosswalk has no junction settings".into());
        }
        w.set_junction_rule(rule)?;
        done.push("junction settings".into());
    }
    // Lane permissions: {"approach": i or [x,y], "lanes": ["L", "S+R"] or "reset"}.
    if let Some(l) = field(cmd, "lanes") {
        let view = w.intersection_view(at).ok_or("the junction is gone")?;
        let k = view.intersection.ok_or("a lone crosswalk has no lanes to set")?;
        let ai = approach_index(&view, field(cmd, "approach").ok_or("lanes needs \"approach\": its index or [x,y]")?)?;
        let arrows = match l {
            Value::String(s) if s == "reset" || s == "default" => None,
            Value::Array(a) => Some(a.iter().map(|x| x.as_str().and_then(parse_arrows).ok_or_else(|| format!("lane arrows like \"L\", \"S+R\", got {x}"))).collect::<Result<Vec<u8>, String>>()?),
            _ => return Err("lanes: [\"L\", \"S+R\", ...] leftmost first, or \"reset\"".into()),
        };
        w.set_lane_permissions(k, view.approaches[ai].segment, arrows)?;
        done.push(format!("lanes of {}", view.approaches[ai].name));
    }
    // A crosswalk upgrade: {"crossing": index or [x,y], "upgrade": "none|button|footbridge"}.
    if let Some(u) = opt_str(cmd, "upgrade")? {
        let upgrade = CrossingUpgrade::parse(u).ok_or("upgrade: none|button|footbridge")?;
        let view = w.intersection_view(at).ok_or("the junction is gone")?;
        let k = pick_crossing(cmd, &view, "upgrade")?;
        w.set_crossing_upgrade(k, upgrade)?;
        done.push(format!("crosswalk {k}: {}", upgrade.label()));
    }
    // [footbridge] Keep the crosswalk under a footbridge at grade (true), or
    // let the bridge replace it (false, the default).
    if let Some(keep) = opt_bool(cmd, "keep_crosswalk")? {
        let view = w.intersection_view(at).ok_or("the junction is gone")?;
        let k = pick_crossing(cmd, &view, "keep_crosswalk")?;
        w.set_crosswalk_kept(k, keep)?;
        done.push(format!("crosswalk {k}: {}", if keep { "kept under its footbridge" } else { "replaced by its footbridge" }));
    }
    if opt_bool(cmd, "open")? == Some(true) {
        tools.junction_editor = crate::ui::junction::EditorState::open(w, at);
        if let (Some(e), Some(i)) = (tools.junction_editor.as_mut(), opt_u64(cmd, "select_approach")?) {
            e.approach = Some(i as usize);
        }
        if let (Some(e), Some(c)) = (tools.junction_editor.as_mut(), opt_u64(cmd, "select_crossing")?) {
            e.crossing = Some(c as u32);
        }
    }
    if let (Some(e), Some(x)) = (tools.junction_editor.as_mut(), opt_f32(cmd, "extent")?) {
        e.extent = x.clamp(crate::ui::junction::EXTENT_MIN, crate::ui::junction::EXTENT_MAX);
    }
    if opt_bool(cmd, "close")? == Some(true) || opt_bool(cmd, "open")? == Some(false) {
        tools.junction_editor = None;
    }
    let view = w.intersection_view(at).ok_or("the junction is gone")?;
    Ok(json!({ "intersection": view_json(w, &view), "done": done, "editor_open": tools.junction_editor.is_some() }))
}

/// A stretch with no room for a column.
pub fn gap_json(g: &nd_sim::pillars::Gap) -> Value {
    json!({ "from": v2(g.a), "to": v2(g.b), "end_in_the_air": g.end, "over_road": g.over_road, "level": round(g.level), "reason": g.reason() })
}

/// For the Road tool's state: the columns of the raised road drawn.
pub fn pillars_json(n: &crate::tools::PillarNote) -> Value {
    json!({
        "count": n.count, "buyouts": n.buyouts, "empty_lots": n.lots, "refusal": n.refusal,
        "gaps": n.gaps.iter().map(gap_json).collect::<Vec<_>>(),
        "at": n.at.iter().map(|p| v2(*p)).collect::<Vec<_>>(),
    })
}
