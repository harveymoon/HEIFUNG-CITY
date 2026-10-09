//! How the city looks: the graphics settings and the map layers.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use crate::render::Overlays;

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
        "frame_ms": round(g.frame_ms), "sim_ms": round(g.sim_ms), "auto_steps": g.steps, "reason": g.reason, "effects": g.effects,
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
