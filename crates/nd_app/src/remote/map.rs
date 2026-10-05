//! `map`: the city in a coarse grid, so a text-only reader can see where
//! people live, where the jobs and the problems are.
//!
//! The occupied bounding box (every lot, plus a margin) is cut into square
//! cells of `cell_m` metres, grown if needed so there are at most
//! `MAX_CELLS`. `layer: "summary"` returns a table of the non-empty cells
//! with every number; a single layer returns its grid of values and an
//! ASCII heat map, one character per cell, rows from the smallest y (top)
//! to the largest. Each building counts in the cell of its lot centre,
//! each citizen at home in the cell of their home (people without one where
//! they stand), each vehicle where it is now.

use nd_sim::{LandUse, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::report::{lu_key, HUNGRY_SATIETY, UNPOWERED_BELOW};
use super::view::round;

pub const DEFAULT_CELL_M: f32 = 100.0;
/// Upper bound on cells, so a reply stays a few thousand tokens.
pub const MAX_CELLS: usize = 400;
const MIN_CELL_M: f32 = 25.0;
/// Heat map characters, empty to hottest.
const RAMP: &[u8] = b" .:-=+*#%@";
pub const LAYERS: &[&str] = &["summary", "population", "land_value", "jobs", "hunger", "vacancy", "power", "traffic"];

#[derive(Default, Clone)]
struct Cell {
    lots: u32,
    residents: u32,
    homes: u32,
    jobs: u32,
    jobs_filled: u32,
    businesses: u32,
    lv_sum: f32,
    hungry: u32,
    vacant_lots: u32,
    unpowered: u32,
    cars: u32,
    lu_area: [f32; 6],
}

impl Cell {
    fn land_value(&self) -> f32 {
        if self.lots > 0 { self.lv_sum / self.lots as f32 } else { 0.0 }
    }

    fn empty(&self) -> bool {
        self.lots == 0 && self.residents == 0 && self.cars == 0 && self.hungry == 0
    }

    fn layer(&self, layer: &str) -> f32 {
        match layer {
            "population" => self.residents as f32,
            "land_value" => self.land_value(),
            "jobs" => self.jobs as f32,
            "hunger" => self.hungry as f32,
            "vacancy" => self.vacant_lots as f32,
            "power" => self.unpowered as f32,
            "traffic" => self.cars as f32,
            _ => 0.0,
        }
    }
}

struct Grid {
    origin: SVec2,
    cell: f32,
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
}

impl Grid {
    fn index(&self, p: SVec2) -> Option<usize> {
        let c = ((p.x - self.origin.x) / self.cell).floor();
        let r = ((p.y - self.origin.y) / self.cell).floor();
        if c < 0.0 || r < 0.0 || c as usize >= self.cols || r as usize >= self.rows {
            return None;
        }
        Some(r as usize * self.cols + c as usize)
    }

    fn at(&mut self, p: SVec2) -> Option<&mut Cell> {
        self.index(p).map(|i| &mut self.cells[i])
    }

    fn center(&self, col: usize, row: usize) -> [f64; 2] {
        [round(self.origin.x + (col as f32 + 0.5) * self.cell), round(self.origin.y + (row as f32 + 0.5) * self.cell)]
    }
}

fn build(w: &World, cell_m: f32) -> Result<Grid, String> {
    let mut lo = SVec2::splat(f32::INFINITY);
    let mut hi = SVec2::splat(f32::NEG_INFINITY);
    for l in w.lots.values() {
        for &p in &l.polygon {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    if !lo.x.is_finite() {
        return Err("the city has no lots yet: nothing to map (try seed_demo)".into());
    }
    let size = hi - lo;
    // Grow the cell until the grid fits the budget.
    let mut cell = cell_m.max(MIN_CELL_M);
    let fits = |c: f32| ((size.x / c).ceil().max(1.0) * (size.y / c).ceil().max(1.0)) as usize <= MAX_CELLS;
    while !fits(cell) {
        cell *= 1.25;
    }
    let cell = cell.round();
    let cols = (size.x / cell).ceil().max(1.0) as usize;
    let rows = (size.y / cell).ceil().max(1.0) as usize;
    // Centre the grid on the occupied box.
    let origin = (lo + hi) * 0.5 - SVec2::new(cols as f32, rows as f32) * cell * 0.5;
    let mut g = Grid { origin, cell, cols, rows, cells: vec![Cell::default(); cols * rows] };
    for l in w.lots.values() {
        let lv = w.land_value.at(l.center);
        let vacant = l.building.is_none() && l.usable();
        if let Some(c) = g.at(l.center) {
            c.lots += 1;
            c.lv_sum += lv;
            c.lu_area[l.land_use.index()] += l.area();
            c.vacant_lots += vacant as u32;
        }
    }
    for b in w.buildings.values() {
        let Some(p) = w.lots.get(b.lot).map(|l| l.center) else { continue };
        if let Some(c) = g.at(p) {
            c.homes += b.homes as u32;
            if b.construction.is_none() {
                c.jobs += b.jobs as u32;
                c.jobs_filled += b.workers.len().min(b.jobs as usize) as u32;
            }
            c.businesses += b.business.as_ref().is_some_and(|z| !z.closed) as u32;
            c.unpowered += (b.powered < UNPOWERED_BELOW && nd_sim::power::demand_mw(b) > 0.0) as u32;
        }
    }
    for cz in w.citizens.values() {
        let p = cz.home.and_then(|h| w.buildings.get(h)).and_then(|b| w.lots.get(b.lot)).map_or(cz.pos, |l| l.center);
        if let Some(c) = g.at(p) {
            c.residents += 1;
            c.hungry += (cz.satiety < HUNGRY_SATIETY) as u32;
        }
    }
    for car in w.traffic.cars.values() {
        if let Some(c) = g.at(car.pos) {
            c.cars += 1;
        }
    }
    Ok(g)
}

fn dominant(c: &Cell) -> Value {
    let (i, a) = c.lu_area.iter().enumerate().fold((0, 0.0f32), |best, (i, &a)| if a > best.1 { (i, a) } else { best });
    if a > 0.0 { json!(lu_key(LandUse::ALL[i])) } else { Value::Null }
}

pub fn map(w: &World, cell_m: Option<f32>, layer: &str) -> Result<Value, String> {
    if !LAYERS.contains(&layer) {
        return Err(format!("unknown map layer {layer:?}; one of {}", LAYERS.join(", ")));
    }
    let g = build(w, cell_m.unwrap_or(DEFAULT_CELL_M))?;
    let x1 = g.origin.x + g.cols as f32 * g.cell;
    let y1 = g.origin.y + g.rows as f32 * g.cell;
    let mut out = json!({
        "layer": layer, "cell_m": g.cell, "cols": g.cols, "rows": g.rows,
        "x_range": [round(g.origin.x), round(x1)], "y_range": [round(g.origin.y), round(y1)],
        "cell_coords": format!(
            "cell (col, row) covers x {:.0} + col*{c:.0} .. +{c:.0}, y {:.0} + row*{c:.0} .. +{c:.0}; its centre is [x0 + (col+0.5)*{c:.0}, y0 + (row+0.5)*{c:.0}]. Row 0 is the smallest y (top of the ASCII map, 'north', the renderer's -Z); col 0 the smallest x (left).",
            g.origin.x, g.origin.y, c = g.cell
        ),
    });
    if layer == "summary" {
        let columns = ["col", "row", "center", "residents", "homes", "jobs", "jobs_filled", "businesses", "land_value", "hungry", "vacant_lots", "unpowered", "cars", "land_use"];
        let mut rows = Vec::new();
        for r in 0..g.rows {
            for col in 0..g.cols {
                let c = &g.cells[r * g.cols + col];
                if c.empty() {
                    continue;
                }
                rows.push(json!([col, r, g.center(col, r), c.residents, c.homes, c.jobs, c.jobs_filled, c.businesses, round(c.land_value()), c.hungry, c.vacant_lots, c.unpowered, c.cars, dominant(c)]));
            }
        }
        out["columns"] = json!(columns);
        out["cells_shown"] = json!(rows.len());
        out["cells_note"] = json!("cells with no lots, residents or vehicles are left out; residents are counted at their home, cars where they are now");
        out["cells"] = json!(rows);
        return Ok(out);
    }
    let values: Vec<f32> = g.cells.iter().map(|c| c.layer(layer)).collect();
    let max = values.iter().copied().fold(0.0f32, f32::max);
    let steps = (RAMP.len() - 1) as f32;
    let mut ascii = Vec::with_capacity(g.rows);
    let mut grid = Vec::with_capacity(g.rows);
    for r in 0..g.rows {
        let mut line = String::with_capacity(g.cols);
        let mut nums = Vec::with_capacity(g.cols);
        for col in 0..g.cols {
            let i = r * g.cols + col;
            let v = values[i];
            nums.push(round(v));
            // Anything above zero gets at least the faintest mark, so a
            // single hungry citizen is not lost in the scaling.
            let k = if v <= 0.0 || max <= 0.0 { 0 } else { (1.0 + (v / max) * (steps - 1.0)).round().clamp(1.0, steps) as usize };
            line.push(RAMP[k] as char);
        }
        ascii.push(line);
        grid.push(nums);
    }
    let legend: Vec<Value> = (1..RAMP.len())
        .map(|k| {
            let lo = (k as f32 - 1.5) / (steps - 1.0) * max;
            let hi = (k as f32 - 0.5) / (steps - 1.0) * max;
            json!({ "char": (RAMP[k] as char).to_string(), "from": round(lo.max(0.0)), "to": round(hi.min(max)) })
        })
        .collect();
    // A framed copy with the y of each row on the right, for reading as is.
    let mut text = format!("{layer} (max {:.2} per {:.0} m cell; ' ' = 0)\n+{}+\n", max, g.cell, "-".repeat(g.cols));
    for (r, line) in ascii.iter().enumerate() {
        text.push_str(&format!("|{line}| y={:.0}\n", g.origin.y + (r as f32 + 0.5) * g.cell));
    }
    text.push_str(&format!("+{}+\n x {:.0} .. {:.0} left to right\n", "-".repeat(g.cols), g.origin.x, x1));
    out["unit"] = json!(match layer {
        "population" => "residents (by home)",
        "land_value" => "average land value of the cell's lots, 0..1",
        "jobs" => "jobs in finished buildings",
        "hunger" => "hungry residents (satiety < 0.3)",
        "vacancy" => "vacant buildable lots",
        "power" => "buildings below 60% power",
        _ => "vehicles on the road now",
    });
    out["max"] = json!(round(max));
    out["total"] = json!(round(values.iter().sum()));
    out["legend"] = json!(legend);
    out["ascii"] = json!(ascii);
    out["ascii_text"] = json!(text);
    out["grid"] = json!(grid);
    Ok(out)
}
