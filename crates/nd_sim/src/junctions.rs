//! [tools] What the Intersection editor sets, as saved: per junction the
//! signal (as the junction decides, forced on, or none), the green of each
//! phase, the protected left arrow and the approaches with left turns
//! forbidden (`JunctionRule`, kept by the traffic model and applied to the
//! lane network on every rebuild); per crosswalk an upgrade, a push-button
//! signal or a footbridge (`CrossingRule`, applied to the walk graph).
//!
//! Rules are anchored by position (and direction), not by node or segment
//! id: ids are renumbered by every rebuild of the roads. A rule whose
//! junction or crosswalk is gone does nothing; it comes back to life if
//! the same junction is drawn again.
//!
//! [r4-edge-bridges] A crossing rule at a junction also belongs to that
//! junction (`CrossingAnchor`): the roads of the plan meeting there and
//! the leg it crosses. Moving the junction, or editing its legs, carries
//! it along; deleting the leg takes it away (`World::fit_crossing_rules`).
//!
//! Lane permissions per lane are the player's lane arrows
//! (`traffic::LaneUseRule`), set from the editor as well.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A junction's signals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalMode {
    /// As the junction's shape decides (signals at busy crossroads).
    #[default]
    Auto,
    /// Signals, wherever there are two phases to run.
    Signals,
    /// No signals: drivers yield (left turns give way to oncoming traffic).
    Unsignalised,
}

impl SignalMode {
    pub fn label(self) -> &'static str {
        match self {
            SignalMode::Auto => "auto",
            SignalMode::Signals => "signals",
            SignalMode::Unsignalised => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "default" => Some(SignalMode::Auto),
            "signals" | "signal" | "on" | "signalised" | "signalized" => Some(SignalMode::Signals),
            "none" | "off" | "unsignalised" | "unsignalized" | "yield" => Some(SignalMode::Unsignalised),
            _ => None,
        }
    }
}

/// The protected left arrow at the start of a green.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeftArrow {
    /// When left turners queue or starve (the signal decides).
    #[default]
    Auto,
    /// Every green of an opposed pair opens with the arrow.
    Always,
    /// Never: left turns always yield.
    Never,
}

impl LeftArrow {
    pub fn label(self) -> &'static str {
        match self {
            LeftArrow::Auto => "auto",
            LeftArrow::Always => "always",
            LeftArrow::Never => "never",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "default" => Some(LeftArrow::Auto),
            "always" | "on" | "protected" | "yes" | "true" => Some(LeftArrow::Always),
            "never" | "off" | "no" | "false" => Some(LeftArrow::Never),
            _ => None,
        }
    }
}

/// Shortest and longest green the editor allows, seconds.
pub const GREEN_MIN: f32 = 5.0;
pub const GREEN_MAX: f32 = 90.0;
/// Range of the protected arrow, seconds.
pub const ARROW_MIN_SECONDS: f32 = 3.0;
pub const ARROW_MAX_SECONDS: f32 = 30.0;

/// The player's settings for one junction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JunctionRule {
    /// Where the junction is.
    pub at: Vec2,
    #[serde(default)]
    pub signal: SignalMode,
    /// Green seconds by approach: (unit direction the street leaves the
    /// junction in, seconds). A phase runs the longest of its approaches';
    /// approaches not listed keep the default.
    #[serde(default)]
    pub greens: Vec<(Vec2, f32)>,
    #[serde(default)]
    pub left_arrow: LeftArrow,
    /// Seconds of protected arrow (0: the default).
    #[serde(default)]
    pub arrow_seconds: f32,
    /// Approaches (by the direction their street leaves the junction in)
    /// from which no one may turn left.
    #[serde(default)]
    pub no_left: Vec<Vec2>,
}

impl JunctionRule {
    pub fn new(at: Vec2) -> Self {
        Self { at, ..Default::default() }
    }

    /// Nothing set: the junction as the game would make it.
    pub fn is_default(&self) -> bool {
        self.signal == SignalMode::Auto && self.greens.is_empty() && self.left_arrow == LeftArrow::Auto && self.arrow_seconds <= 0.0 && self.no_left.is_empty()
    }

    /// The green set for an approach leaving in `dir`, if any.
    pub fn green_for(&self, dir: Vec2) -> Option<f32> {
        self.greens.iter().find(|(d, _)| d.dot(dir) > 0.98).map(|(_, s)| *s)
    }

    pub fn set_green(&mut self, dir: Vec2, seconds: Option<f32>) {
        self.greens.retain(|(d, _)| d.dot(dir) <= 0.98);
        if let Some(s) = seconds {
            self.greens.push((dir, s.clamp(GREEN_MIN, GREEN_MAX)));
        }
    }

    pub fn left_forbidden(&self, dir: Vec2) -> bool {
        self.no_left.iter().any(|d| d.dot(dir) > 0.98)
    }

    pub fn set_left_forbidden(&mut self, dir: Vec2, forbidden: bool) {
        self.no_left.retain(|d| d.dot(dir) <= 0.98);
        if forbidden {
            self.no_left.push(dir);
        }
    }
}

/// What a crosswalk has been upgraded to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossingUpgrade {
    /// A plain crosswalk (or the junction's walk lights).
    #[default]
    None,
    /// A push-button pedestrian signal: traffic is stopped by a red only
    /// after someone presses the button, for long enough to cross.
    Button,
    /// A footbridge: a raised walkway over the street with stairs at both
    /// ends. Nobody on the bridge holds up traffic. [footbridge] It
    /// replaces the crosswalk under it (paint, walk lights and the walk
    /// graph's at-grade edge go) unless the player keeps that crosswalk
    /// (`CrossingRule::keep_crosswalk`), when walkers merely prefer the
    /// bridge.
    Footbridge,
    /// [walk-crossings] No crossing here at all (the player shapes where
    /// people cross): nothing painted, no walk light, and walkers route
    /// round by another crossing. Jaywalkers keep away from it too.
    Removed,
}

impl CrossingUpgrade {
    pub fn label(self) -> &'static str {
        match self {
            CrossingUpgrade::None => "none",
            CrossingUpgrade::Button => "button",
            CrossingUpgrade::Footbridge => "footbridge",
            CrossingUpgrade::Removed => "removed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" | "plain" | "zebra" | "off" => Some(CrossingUpgrade::None),
            "button" | "push_button" | "push-button" | "pelican" | "signal" => Some(CrossingUpgrade::Button),
            "footbridge" | "bridge" | "overpass" | "skywalk" => Some(CrossingUpgrade::Footbridge),
            "removed" | "remove" | "no_crossing" | "no-crossing" | "deny" | "closed" => Some(CrossingUpgrade::Removed),
            _ => None,
        }
    }

    /// One-off cost of building it.
    pub fn cost(self) -> f32 {
        match self {
            CrossingUpgrade::None => 0.0,
            CrossingUpgrade::Button => 1_500.0,
            CrossingUpgrade::Footbridge => 18_000.0,
            CrossingUpgrade::Removed => 0.0,
        }
    }
}

/// [r4-edge-bridges] The junction a crossing rule belongs to, as last
/// fitted: where it was, the plan's roads meeting there, the road of the
/// leg crossed and the direction that leg leaves the junction in. The
/// rule follows the junction these roads make wherever it is moved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CrossingAnchor {
    /// The junction's middle when last fitted.
    pub junction: Vec2,
    /// The plan's roads (gestures) meeting at the junction.
    #[serde(default)]
    pub roads: Vec<crate::plan::GestureId>,
    /// The road the crossed leg is part of.
    #[serde(default)]
    pub leg: Option<crate::plan::GestureId>,
    /// Unit direction the crossed leg leaves the junction in.
    #[serde(default)]
    pub leg_dir: Vec2,
    /// The crossed street's name (for the message when it goes).
    #[serde(default)]
    pub street: String,
}

/// An upgrade on one crosswalk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossingRule {
    /// The crossing's middle, on the street's centre line.
    pub at: Vec2,
    /// Unit direction of the street crossed (either way along it).
    pub dir: Vec2,
    pub upgrade: CrossingUpgrade,
    /// [footbridge] A footbridge's crosswalk kept at grade under it (the
    /// player's choice). Off by default, and in saves from before it, so a
    /// footbridge replaces its crosswalk.
    #[serde(default)]
    pub keep_crosswalk: bool,
    /// [r4-edge-bridges] The junction it belongs to (None: a lone zebra's,
    /// or one not fitted yet; a save from before fits it on load).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<CrossingAnchor>,
}

/// Every crosswalk upgrade in the city.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Crosswalks {
    #[serde(default)]
    pub rules: Vec<CrossingRule>,
}

impl Crosswalks {
    /// The rule for a crossing at `at` across a street running along `dir`.
    pub fn find(&self, at: Vec2, dir: Vec2) -> Option<&CrossingRule> {
        self.rules.iter().find(|r| r.at.distance(at) < 2.0 && r.dir.dot(dir).abs() > 0.9)
    }

    pub fn set(&mut self, at: Vec2, dir: Vec2, upgrade: CrossingUpgrade) {
        self.rules.retain(|r| !(r.at.distance(at) < 2.0 && r.dir.dot(dir).abs() > 0.9));
        if upgrade != CrossingUpgrade::None {
            self.rules.push(CrossingRule { at, dir, upgrade, keep_crosswalk: false, anchor: None });
        }
    }

    /// [footbridge] Keep (or take away) the crosswalk under the footbridge
    /// at `at`; false where there is no footbridge rule there.
    pub fn set_keep_crosswalk(&mut self, at: Vec2, dir: Vec2, keep: bool) -> bool {
        match self.rules.iter_mut().find(|r| r.at.distance(at) < 2.0 && r.dir.dot(dir).abs() > 0.9 && r.upgrade == CrossingUpgrade::Footbridge) {
            Some(r) => {
                r.keep_crosswalk = keep;
                true
            }
            None => false,
        }
    }
}

/// Height of a footbridge's walkway above the street (headroom for a
/// truck, plus its deck), metres.
pub const FOOTBRIDGE_LEVEL: f32 = 6.0;
/// Horizontal run of each flight of stairs (a 1:1.7 stair).
pub const FOOTBRIDGE_RUN: f32 = 10.0;
/// Width of the walkway and the stairs.
pub const FOOTBRIDGE_WIDTH: f32 = 2.2;
