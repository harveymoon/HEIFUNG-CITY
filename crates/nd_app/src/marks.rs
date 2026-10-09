//! [camera] Marks in the world that are about people the player cares
//! for: a gold halo on favourites (and the cars they drive or own). The
//! posts under a selected person's pins (whose heads `ui::pins` draws) and
//! a selected delivery robot's route are the selection's
//! (`overlay::selection`), with the colours and heights from here.

use bevy::prelude::*;
use nd_sim::{Activity, BuildingId, Holding, Vec2 as SVec2, World};

use crate::sim::Sim;
use crate::overlay::shapes::{flat_at, vehicle_box};
use crate::tools::{Selection, ToolState};

pub struct MarksPlugin;

impl Plugin for MarksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, draw_marks.in_set(crate::overlay::OverlaySet::Draw));
    }
}

/// The gold of favourites: warmer and brighter than the selection's.
pub fn favourite_gold(pulse: f32) -> Color {
    Color::linear_rgb(4.2 + pulse, 2.7 + pulse * 0.6, 0.25)
}

/// A holding's colour, linear for gizmos and as sRGB bytes for the pins.
pub fn holding_rgb(h: Holding) -> [u8; 3] {
    match h {
        Holding::Home => [90, 230, 160],
        Holding::Work => [255, 160, 60],
        Holding::Owns => [200, 130, 255],
    }
}

/// Where a pin's head stands: well above the building's roof at its lot's
/// centre, so it clears the skyline around it.
pub fn pin_top(w: &World, b: BuildingId) -> Option<Vec3> {
    let bl = w.buildings.get(b)?;
    let c = w.lots.get(bl.lot)?.center;
    let roof = bl.floors as f32 * 3.6 + 6.0;
    Some(Vec3::new(c.x, w.surface_height(c) + roof + 14.0, c.y))
}

fn draw_marks(mut gizmos: Gizmos, sim: Res<Sim>, tools: Res<ToolState>, time: Res<Time>, (mode, follow): (Res<crate::camera::CameraMode>, Res<crate::camera::CameraFollow>)) {
    let w = &sim.world;
    // Through a followed person's eyes their own halo would fill the view.
    let pov = *mode == crate::camera::CameraMode::Walk && follow.target.is_some();
    let f = sim.frac();
    let pulse = (time.elapsed_secs() * 2.2).sin() * 0.5 + 0.5;
    let gold = favourite_gold(pulse);
    let blend = |a: SVec2, b: SVec2| if a.distance(b) < 30.0 { a.lerp(b, f) } else { b };
    // On the level walkers and robots are drawn on (under a viaduct, not on it).
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y + w.surface_height_near(p, w.ground_height(p)), p.y);

    // Favourites: a gold ring and a diamond over the head on foot; the car
    // they drive or ride in outlined in gold; their parked car too.
    for &id in &tools.favorites {
        let Some(c) = w.citizens.get(id) else { continue };
        if pov && follow.target == Some(Selection::Citizen(id)) {
            continue;
        }
        let car = match &c.activity {
            Activity::Driving { car, .. } => w.traffic.cars.get(*car),
            Activity::Passenger { driver, seated: true, .. } => match w.citizens.get(*driver).map(|d| &d.activity) {
                Some(Activity::Driving { car, .. }) => w.traffic.cars.get(*car),
                _ => None,
            },
            _ => None,
        };
        if let Some(car) = car {
            let (p, h) = car.pose_at(f);
            let y = w.car_height_at(car, p);
            vehicle_box(&mut gizmos, p, h, car.length() + 1.0, 3.0, y + 0.2, gold);
            diamond(&mut gizmos, Vec3::new(p.x, y + 4.2, p.y), 0.9, gold);
        } else if c.is_visible() {
            let here = blend(c.prev_pos, c.pos);
            gizmos.circle(flat_at(at(here, 0.12)), 1.3 + 0.25 * pulse, gold);
            diamond(&mut gizmos, at(here, 3.0), 0.55, gold);
        }
        if let Some(spot) = c.car.and_then(|v| w.vehicles.cars.get(v)).and_then(|v| v.spot()) {
            vehicle_box(&mut gizmos, spot.pos, spot.heading, 5.4, 2.8, w.surface_height(spot.pos) + 0.2, gold);
        }
    }
}

/// A small upright diamond (two crossed rhombi) centred at `c`.
fn diamond(gizmos: &mut Gizmos, c: Vec3, r: f32, col: Color) {
    let (up, down) = (c + Vec3::Y * r * 1.4, c - Vec3::Y * r * 1.4);
    for d in [Vec3::X * r, Vec3::Z * r] {
        gizmos.linestrip([up, c + d, down, c - d, up], col);
    }
}
