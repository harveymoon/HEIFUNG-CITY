//! [r4-tiles] Land for sale by remote.
//!
//! - `tiles {}`: every tile round the starting map: owned, for sale now,
//!   its price and what it holds.
//! - `buy_tile {tile: [i, j]}` (or `pos: [x, y]`, a point in it): buy it
//!   from the treasury; `dry_run: true` only says the price or why not.
//! - `pick_tile {tile: [i, j]}`: show the Land for sale layer with that tile
//!   picked in its window (`tile: null` picks none).

use nd_sim::landscape::{outer_tiles, tile_center, tile_direction, tile_of, tile_rect, Tile, GROWN_HALF, LAND_HALF, MAX_RING, TILE};
use nd_sim::world::land_tiles::LandTile;
use nd_sim::World;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::{field, opt_bool, opt_vec2, round, v2};

fn tile_json(t: &LandTile) -> Value {
    let g = &t.ground;
    let (lo, hi) = tile_rect(t.tile());
    json!({
        "tile": [t.tile().0, t.tile().1],
        "direction": tile_direction(t.tile()),
        "owned": t.owned,
        "buyable": t.buyable,
        "price": round(t.price),
        "center": v2(tile_center(t.tile())),
        "bounds": [v2(lo), v2(hi)],
        "land": round(g.land), "sea": round(g.sea), "river": round(g.river), "lake": round(g.lake),
        "level": round(g.level), "buildable": round(g.buildable),
        "height": { "low": round(g.low), "mean": round(g.mean), "high": round(g.high) },
        "slope": round(g.slope), "fertility": round(g.fertility), "oil": round(g.oil), "rock": round(g.rock),
        "hot_spots": g.hot_spots, "river_crosses": g.river_crosses, "stream_crosses": g.stream_crosses,
        "highway": t.highway, "rail": t.rail, "roads": t.roads,
        "summary": t.summary(),
    })
}

pub(super) fn list(w: &World) -> Value {
    let tiles = w.land_tiles();
    json!({
        "tile_m": TILE,
        "start_half_m": LAND_HALF,
        "max_half_m": GROWN_HALF,
        "max_ring": MAX_RING,
        "land_half_m": w.landscape.land_half(),
        "bought": w.landscape.tiles.iter().map(|t| json!([t.0, t.1])).collect::<Vec<_>>(),
        "for_sale": tiles.iter().filter(|t| t.buyable).count(),
        "treasury": round(w.economy.treasury),
        "note": if w.landscape.is_flat() { "the flat map has no land round it to buy" } else { "" },
        "tiles": tiles.iter().map(tile_json).collect::<Vec<_>>(),
    })
}

fn tile_arg(cmd: &Value) -> Result<Option<Tile>, String> {
    if let Some(v) = field(cmd, "tile") {
        if v.is_null() {
            return Ok(None);
        }
        let a = v.as_array().filter(|a| a.len() == 2).ok_or("tile is [i, j]")?;
        let i = a[0].as_i64().ok_or("tile is [i, j] whole numbers")? as i32;
        let j = a[1].as_i64().ok_or("tile is [i, j] whole numbers")? as i32;
        return Ok(Some((i, j)));
    }
    if let Some(p) = opt_vec2(cmd, "pos")? {
        return Ok(Some(tile_of(p)));
    }
    Err(format!("give tile: [i, j] (one of {:?}) or pos: [x, y]", outer_tiles()))
}

pub(super) fn buy(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let t = tile_arg(cmd)?.ok_or("which tile?")?;
    let w = &mut ctx.sim.world;
    if opt_bool(cmd, "dry_run")?.unwrap_or(false) {
        return Ok(match w.tile_offer(t) {
            Ok(price) => json!({ "ok": true, "tile": [t.0, t.1], "price": round(price), "treasury": round(w.economy.treasury) }),
            Err(why) => json!({ "ok": false, "tile": [t.0, t.1], "reason": why }),
        });
    }
    let paid = w.buy_tile(t)?;
    if ctx.picked_tile.0 == Some(t) {
        ctx.picked_tile.0 = None;
    }
    Ok(json!({ "ok": true, "tile": [t.0, t.1], "direction": tile_direction(t), "paid": round(paid), "treasury": round(w.economy.treasury), "land_half_m": w.landscape.land_half(), "bought": w.landscape.tiles.iter().map(|t| json!([t.0, t.1])).collect::<Vec<_>>() }))
}

pub(super) fn pick(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let t = tile_arg(cmd)?;
    ctx.overlays.land_tiles = true;
    ctx.picked_tile.0 = t;
    Ok(json!({ "ok": true, "picked": t.map(|t| json!([t.0, t.1])) }))
}
