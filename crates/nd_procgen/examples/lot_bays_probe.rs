//! Parking lots in a save: spaces the sim counts against bays the mesh
//! paints (`lot_bays_probe <save.ron>`). Not part of the game.
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let w = nd_sim::World::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    for (_, b) in w.buildings.iter().filter(|(_, b)| b.is_parking()) {
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let bays = nd_procgen::buildings::surface_lot_bays_world(lot).len();
        println!("{:<22} floors {} area {:>6.0} capacity {:>4} used {:>4} bays(0.4 yard,3.0) {:>4}", b.name, b.floors, lot.area(), b.parking_capacity, b.parking_used, bays);
    }
}
