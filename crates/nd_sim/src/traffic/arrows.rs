//! Lane arrows on junction approaches: the turn masks, their defaults from
//! demand, the player's rules and the slow adaptation to measured turns.

use super::*;

/// Arrow bit for a connector's turn.
pub fn turn_bit(turn: i8) -> u8 {
    match turn {
        t if t < 0 => LEFT,
        0 => STRAIGHT,
        _ => RIGHT,
    }
}

/// "L", "S+R", "L+S+R" ...
pub fn arrows_label(a: u8) -> String {
    let parts: Vec<&str> = [(LEFT, "L"), (STRAIGHT, "S"), (RIGHT, "R")].iter().filter(|(b, _)| a & b != 0).map(|(_, n)| *n).collect();
    if parts.is_empty() { "-".into() } else { parts.join("+") }
}

/// Parse "L", "S+R", "LS", "left+straight" ... into arrow bits.
pub fn parse_arrows(text: &str) -> Option<u8> {
    let mut a = 0;
    for part in text.split(['+', ',', ' ', '|']).filter(|p| !p.is_empty()) {
        let p = part.to_ascii_lowercase();
        match p.as_str() {
            "l" | "left" => a |= LEFT,
            "s" | "straight" | "t" | "through" => a |= STRAIGHT,
            "r" | "right" => a |= RIGHT,
            _ if p.chars().all(|c| "lsr".contains(c)) => {
                for c in p.chars() {
                    a |= match c { 'l' => LEFT, 's' => STRAIGHT, _ => RIGHT };
                }
            }
            _ => return None,
        }
    }
    (a != 0).then_some(a)
}

/// Default arrows for an approach of `n` lanes offering `exits`, given the
/// share of recent traffic turning left (None before any is counted) and
/// the arrows it has now (for hysteresis).
pub fn default_arrows(n: usize, exits: u8, left_share: Option<f32>, now: Option<&[u8]>) -> Vec<u8> {
    if n <= 1 || exits.count_ones() <= 1 {
        return vec![exits; n];
    }
    let shared_now = now.is_some_and(|a| a.len() == 2 && a[0] & STRAIGHT != 0);
    let dual_now = now.is_some_and(|a| a.len() >= 3 && a[1] & LEFT != 0);
    // Before anything is counted: the plain default (a left-only lane on a
    // two-lane approach, one left lane on wider ones).
    let share = left_share.unwrap_or(0.35);
    let quiet_left = if shared_now { share < 0.4 } else { share < 0.3 };
    let busy_left = if dual_now { share > 0.3 } else { share > 0.4 };
    let mut a: Vec<u8> = match n {
        2 if quiet_left => vec![LEFT | STRAIGHT, STRAIGHT | RIGHT],
        2 => vec![LEFT, STRAIGHT | RIGHT],
        3 if busy_left => vec![LEFT, LEFT | STRAIGHT, STRAIGHT | RIGHT],
        _ => {
            let mut v = vec![STRAIGHT; n];
            v[0] = LEFT;
            if busy_left && n >= 4 {
                v[1] = LEFT;
            }
            v[n - 1] = STRAIGHT | RIGHT;
            v
        }
    };
    // Fit to the exits there are: a lane left with nothing takes the side
    // it is on; every exit is reachable from some lane.
    for (i, x) in a.iter_mut().enumerate() {
        *x &= exits;
        if *x == 0 {
            let side = if exits & STRAIGHT != 0 { STRAIGHT } else if i < n / 2 { LEFT } else { RIGHT };
            *x = if exits & side != 0 { side } else { exits };
        }
    }
    if exits & LEFT != 0 && a.iter().all(|x| x & LEFT == 0) {
        a[0] |= LEFT;
    }
    if exits & RIGHT != 0 && a.iter().all(|x| x & RIGHT == 0) {
        a[n - 1] |= RIGHT;
    }
    if exits & STRAIGHT != 0 && a.iter().all(|x| x & STRAIGHT == 0) {
        a[n - 1] |= STRAIGHT;
    }
    a
}

/// Arrows for the lanes (leftmost first) of `segment` arriving at `node`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaneUseRule {
    pub segment: SegmentId,
    pub node: NodeId,
    pub arrows: Vec<u8>,
    /// Where the junction is and the unit direction the street leaves it
    /// in: how the rule finds its approach again after a rebuild has
    /// renumbered segments and nodes (`Traffic::anchor_lane_use`). None in
    /// saves from before anchoring, and until the next rebuild for a rule
    /// just set; those are matched by id on the network they were made on.
    #[serde(default)]
    pub at: Option<Vec2>,
    #[serde(default)]
    pub away: Option<Vec2>,
}

impl Traffic {
    /// Mean and maximum of the recent left turn waits, and how many.
    pub fn left_wait_stats(&self) -> (f32, f32, usize) {
        let n = self.left_waits.len();
        if n == 0 {
            return (0.0, 0.0, 0);
        }
        (self.left_waits.iter().sum::<f32>() / n as f32, self.left_waits.iter().copied().fold(0.0, f32::max), n)
    }

    /// Set (or with `None` clear) the player's arrows for the lanes of
    /// `segment` arriving at `node`, leftmost first.
    pub fn set_lane_use(&mut self, segment: SegmentId, node: NodeId, arrows: Option<Vec<u8>>) -> Result<(), String> {
        let Some(ai) = self.net.approaches.iter().position(|a| a.segment == segment && a.node == node) else {
            return Err("that street does not arrive at a junction there".into());
        };
        self.lane_use.retain(|r| !(r.segment == segment && r.node == node));
        match arrows {
            Some(a) => {
                let ap = &self.net.approaches[ai];
                if a.len() != ap.lanes.len() {
                    return Err(format!("the approach has {} lanes, {} arrows given", ap.lanes.len(), a.len()));
                }
                if a.iter().any(|&x| x & ap.exits == 0) {
                    return Err("every lane needs a movement the junction offers".into());
                }
                if a.iter().fold(0, |acc, &x| acc | x) & ap.exits != ap.exits {
                    return Err("some exit would be reachable from no lane".into());
                }
                let a: Vec<u8> = a.iter().map(|&x| x & ap.exits).collect();
                self.lane_use.push(LaneUseRule { segment, node, arrows: a.clone(), at: None, away: None });
                self.net.approaches[ai].fixed = true;
                self.net.set_arrows(ai, a);
            }
            None => {
                let ap = &self.net.approaches[ai];
                let a = default_arrows(ap.lanes.len(), ap.exits, None, None);
                self.net.approaches[ai].fixed = false;
                self.net.set_arrows(ai, a);
            }
        }
        Ok(())
    }

    /// Revise the default arrows of approaches the player has not set from
    /// their recent turning counts.
    pub(super) fn adapt_arrows(&mut self) {
        for ai in 0..self.net.approaches.len() {
            let ap = &self.net.approaches[ai];
            let total: f32 = ap.counts.iter().sum();
            // Roundabout arrows follow the ring's lanes, not the counts.
            let ring = self.net.rings.nodes.contains_key(&ap.node);
            if !ap.fixed && !ring && total >= 20.0 {
                let a = default_arrows(ap.lanes.len(), ap.exits, Some(ap.counts[0] / total), Some(&ap.arrows));
                self.net.set_arrows(ai, a);
            }
            let ap = &mut self.net.approaches[ai];
            for c in &mut ap.counts {
                *c *= 0.5;
            }
        }
    }
}
