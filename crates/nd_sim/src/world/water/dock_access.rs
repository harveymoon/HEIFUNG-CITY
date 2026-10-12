//! [dock-access] Whether a building's access road reaches the network.

use super::*;

impl World {
    /// [dock-access] For a dock, station or yard: whether its street
    /// reaches the road network (`network_components`) for drivers. None
    /// for any other building.
    pub fn road_access(&self, b: BuildingId) -> Option<bool> {
        if let Some(p) = self.ports.port_of(b) {
            return Some(p.road_connected);
        }
        self.buildings.get(b).and_then(|x| x.rail.as_ref()).map(|l| l.road_connected)
    }

    /// [sweep-sim] For a dock, station or yard: whether its access is
    /// enough for its work (a road; for passengers a path will do). None
    /// for any other building.
    pub fn access_serves(&self, b: BuildingId) -> Option<bool> {
        if let Some(p) = self.ports.port_of(b) {
            return Some(p.road_connected || (p.kind.is_ferry() && p.walk_connected));
        }
        self.buildings.get(b).and_then(|x| x.rail.as_ref()).map(|l| l.road_connected || (l.kind.is_passenger() && l.walk_connected))
    }

    /// [sweep-sim] A ferry terminal or passenger station people can walk to
    /// but no road reaches yet: it serves passengers; taxis, buses and
    /// deliveries cannot get there. The line the inspector and API show.
    pub fn access_note(&self, b: BuildingId) -> Option<String> {
        if self.road_access(b)? || !self.access_serves(b)? {
            return None;
        }
        Some("reached on foot only: a footpath joins it to the city, so passengers walk in and out; goods need a road, and taxis, buses and deliveries cannot get there until a road reaches the marked end of its street stub".to_string())
    }

    /// [dock-access] (D7) A dock, station or yard joined to the city by a
    /// footpath and no road. None for any other building.
    pub fn footpath_only(&self, b: BuildingId) -> Option<bool> {
        let walk = match self.ports.port_of(b) {
            Some(p) => p.walk_connected,
            None => self.buildings.get(b)?.rail.as_ref()?.walk_connected,
        };
        Some(walk && !self.road_access(b)?)
    }

    /// [dock-access] What the player is told about a dock or station off
    /// the roads, for the inspector, the alerts and the API.
    pub fn access_problem(&self, b: BuildingId) -> Option<String> {
        if self.road_access(b)? || self.access_serves(b)? {
            return None;
        }
        let joined = self.stub_ends_of(b).is_empty();
        let walk = self.ports.port_of(b).map(|p| p.walk_connected).or_else(|| self.buildings.get(b).and_then(|x| x.rail.as_ref()).map(|l| l.walk_connected)).unwrap_or(false);
        // (D7) Joined by a footpath only: say that first and plainly.
        if walk {
            let goods = match self.ports.port_of(b).map(|p| p.kind) {
                Some(PortKind::ShippingPort) => "no freight is landed or shipped until lorries can reach the quay",
                Some(PortKind::FishingPortSmall | PortKind::FishingPortLarge) => "the catch is not landed until lorries can take it away",
                Some(PortKind::MarinaSmall | PortKind::MarinaLarge) => "boat owners drive in with their gear and the chandlery takes deliveries, so it stays shut",
                _ => "freight trains are not loaded until lorries can reach the yard",
            };
            let todo = if joined { "carry a road on from its street to the city" } else { "draw a road to the marked end of its street stub" };
            return Some(format!("joined only by a footpath: people can walk to it, but goods need a road; {goods}: {todo}"));
        }
        let what = if self.ports.port_of(b).is_some_and(|p| p.kind.is_ferry()) {
            "ferries sail, but carry no one until people can get to the pier (a road or a path)"
        } else if self.ports.port_of(b).is_some() {
            "nothing lands or is sold until lorries can reach it"
        } else if self.buildings.get(b).and_then(|x| x.rail.as_ref()).is_some_and(|l| l.kind.is_passenger() && l.rail_connected) {
            "trains call, but nobody gets on or off until a road or a path reaches it"
        } else {
            "it opens once it is on the roads"
        };
        Some(if joined {
            format!("not connected to the road network: its street does not reach the city or a way out of town yet; {what}")
        } else {
            format!("not connected to the road network: draw a road to the marked end of its street stub; {what}")
        })
    }

    /// [dock-access] The free ends of a dock's or station's street stub
    /// that a road can be drawn onto: dead ends of the street it laid (a
    /// dock's access road: its far end only). Empty once joined.
    pub fn stub_ends_of(&self, b: BuildingId) -> Vec<Vec2> {
        let ends: Vec<Vec2> = if let Some(p) = self.ports.port_of(b) {
            match p.stub {
                Some(s) => vec![s[1]],
                None => p.front_line().to_vec(),
            }
        } else if let Some(f) = self.facilities.values().find(|f| f.building == Some(b)) {
            f.road_line().to_vec()
        } else {
            return Vec::new();
        };
        ends.into_iter()
            .filter_map(|e| self.roads.node_near(e, 2.5))
            .filter(|&n| self.roads.node(n).segments.iter().filter(|&&s| self.roads.segment(s).alive).count() == 1)
            .map(|n| self.roads.node(n).pos)
            .collect()
    }

    /// [dock-access] Docks and stations off the roads with the free ends
    /// of their stubs, for the "connect here" markers.
    pub fn unjoined_stubs(&self) -> Vec<(BuildingId, Vec<Vec2>)> {
        let docks = self.ports.ports.values().filter(|p| !p.road_connected).filter_map(|p| p.building);
        let rail = self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| !l.road_connected)).map(|(id, _)| id);
        docks.chain(rail).filter(|b| self.buildings.contains_key(*b)).map(|b| (b, self.stub_ends_of(b))).collect()
    }

    /// [dock-access] The Build tool's verdict for a dock or a rail piece at
    /// `p`: what placing it there would do (`Ok`), or why it cannot go
    /// (`Err`). None for anything else. Neither needs a road: a dock needs
    /// the water's edge, a station or yard nothing but room; what is not
    /// there yet is laid as stubs to join later.
    pub fn piece_verdict(&self, what: crate::services::Placeable, p: Vec2) -> Option<Result<String, String>> {
        use crate::services::Placeable;
        let cost = what.cost();
        let afford = |v: Result<String, String>| if self.economy.treasury < cost { Err(format!("the treasury has {:.0}; it costs {:.0}", self.economy.treasury, cost)) } else { v };
        match what {
            Placeable::Dock(kind) => Some(afford(self.plan_port(kind, p).map(|port| {
                let probe = port.front_mid() - port.seaward * rail::ROAD_GAP;
                if self.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= rail::ROAD_EDGE_FUSE) {
                    "on the water's edge, fronting the street here".to_string()
                } else {
                    let ferry = if kind.is_ferry() { " (its ferries sail meanwhile, carrying no one)" } else { "" };
                    format!("on the water's edge; no road needed: it lays a short access road inland. Draw a road to its marked end to put it on the road network{ferry}")
                }
            }))),
            Placeable::Rail(kind) => {
                let (f, rail_fused, road_fused) = rail::Facility::plan(kind, p, &self.roads);
                Some(afford(match self.rail_piece_problem(&f) {
                    Some(why) => Err(why),
                    None => Ok(match (rail_fused, road_fused) {
                        (true, true) => "on the track, fronting the street".to_string(),
                        (true, false) => "on the track; no road needed: it lays a street stub. Draw a road to its marked end to connect it (trains call meanwhile; nobody gets on or off)".to_string(),
                        (false, true) => "by the street; it lays a rail stub: lay track onto it".to_string(),
                        (false, false) => "in open ground; it lays a rail stub and a street stub to join to the track and the roads".to_string(),
                    }),
                }))
            }
            _ => None,
        }
    }
}
