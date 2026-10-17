//! How the city looks: the graphics settings and the map layers.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use crate::render::Overlays;
use nd_sim::survey::LandLayer;

/// [look] The graphics setting, the tier in force (the setting's or Auto's
/// pick, which takes effect next frame), the frame time and the effects.
pub(super) fn graphics_json(ctx: &Ctx) -> Value {
    let g = &ctx.stats.graphics;
    let tier = match ctx.lighting.quality {
        crate::render::QualitySetting::Fixed(q) => q,
        crate::render::QualitySetting::Auto => g.tier,
    };
    json!({
        "quality": ctx.lighting.quality.label().to_lowercase(), "tier": tier.label().to_lowercase(),
        "frame_ms": round(g.frame_ms), "sim_ms": round(g.sim_ms), "long_frames": g.long_frames.iter().map(|&(ms, sim_ms, tick)| json!({ "ms": round(ms), "sim_ms": round(sim_ms), "tick": tick })).collect::<Vec<_>>(), "auto_steps": g.steps, "reason": g.reason, "effects": g.effects,
    })
}

/// Every map layer and whether it is on, the one showing (`active`, null
/// for none: they are exclusive) and the "show other projects" switch.
pub(super) fn overlays_json(o: &Overlays) -> Value {
    let mut m = serde_json::Map::new();
    for l in crate::map_layers::MAP_LAYERS {
        m.insert(l.key.into(), json!(o.get(l.key).unwrap_or(false)));
    }
    m.insert("land".into(), json!(o.land.map(|l| l.key())));
    m.insert("active".into(), json!(crate::map_layers::active(o).map(|l| l.key())));
    m.insert("projects".into(), json!(o.projects));
    Value::Object(m)
}

/// `overlay`: the map layers. They are exclusive (`crate::map_layers`):
/// turning one on turns every other off; turning the one showing off
/// leaves none.
pub(super) fn overlay(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    use crate::map_layers::{self as ml, LayerId};
    let o = &mut ctx.overlays;
    let unknown = |k: &str| format!("unknown layer {k:?}; one of none, {}", ml::keys().join(", "));
    let parse = |k: &str| -> Result<Option<LayerId>, String> {
        if k.is_empty() || k.eq_ignore_ascii_case("none") || k.eq_ignore_ascii_case("off") {
            Ok(None)
        } else {
            LayerId::parse(k).map(Some).ok_or_else(|| unknown(k))
        }
    };
    let switch = |o: &mut Overlays, l: LayerId, on: bool| {
        if on {
            ml::set_active(o, Some(l));
        } else if ml::active(o) == Some(l) {
            ml::set_active(o, None);
        }
    };
    // `{active: "traffic" | "none"}`: the layer to show.
    match cmd.get("active") {
        None => {}
        Some(Value::Null) => ml::set_active(o, None),
        Some(_) => {
            let k = opt_str(cmd, "active")?.unwrap_or("none");
            ml::set_active(o, parse(k)?);
        }
    }
    // `{layer, on}` (on omitted toggles).
    if let Some(layer) = opt_str(cmd, "layer")? {
        let on = opt_bool(cmd, "on")?;
        if layer == "projects" {
            o.projects = on.unwrap_or(!o.projects);
        } else {
            match parse(layer)? {
                Some(l) => {
                    let now = ml::active(o) == Some(l);
                    switch(o, l, on.unwrap_or(!now));
                }
                None => ml::set_active(o, None),
            }
        }
    }
    // `{land: "oil"}` shows a land layer, `"none"` (or null) hides
    // whichever land layer shows.
    match cmd.get("land") {
        None => {}
        Some(Value::Null) => {
            if o.land.is_some() {
                o.land = None;
            }
        }
        Some(_) => {
            let k = opt_str(cmd, "land")?.unwrap_or("none");
            match parse(k)? {
                Some(l @ LayerId::Land(_)) => ml::set_active(o, Some(l)),
                Some(_) => return Err(format!("{k:?} is not a land layer; one of none, {}", LandLayer::ALL.iter().map(|l| l.key()).collect::<Vec<_>>().join(", "))),
                None => o.land = None,
            }
        }
    }
    // The older form, a bool per layer name: true shows it (alone).
    for l in ml::MAP_LAYERS {
        if let Some(b) = opt_bool(cmd, l.key)? {
            switch(o, LayerId::parse(l.key).expect("registry key"), b);
        }
    }
    if let Some(b) = opt_bool(cmd, "projects")? {
        o.projects = b;
    }
    Ok(overlays_json(o))
}

/// `lighting {hour?, follow_clock?, weather?}`: the time of day the view
/// shows, and [show-weather] whether the weather is drawn.
pub(super) fn lighting(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let l = &mut ctx.lighting;
    if let Some(h) = opt_f32(cmd, "hour")? {
        l.hour = h.clamp(0.0, 24.0);
        // Asking for an hour means wanting to see it.
        l.follow_clock = false;
    }
    if let Some(b) = opt_bool(cmd, "follow_clock")? {
        l.follow_clock = b;
    }
    if let Some(b) = opt_bool(cmd, "weather")? {
        l.show_weather = b;
    }
    Ok(json!({ "follow_clock": l.follow_clock, "hour": l.hour, "heat_vision": l.heat, "weather": l.show_weather }))
}

pub(super) fn heat_vision(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let l = &mut ctx.lighting;
    l.heat = opt_bool(cmd, "on")?.unwrap_or(!l.heat);
    Ok(json!({ "heat_vision": l.heat }))
}

/// [look] `graphics {quality: auto|low|medium|high}` pins the tier (for
/// screenshots that must compare) or hands it back to Auto.
pub(super) fn graphics(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if let Some(q) = opt_str(cmd, "quality")?.or(opt_str(cmd, "tier")?) {
        ctx.lighting.quality = crate::render::QualitySetting::parse(q).ok_or_else(|| format!("unknown quality {q:?}; auto, low, medium or high"))?;
    }
    Ok(graphics_json(ctx))
}
