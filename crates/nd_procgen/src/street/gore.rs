//! Where a ramp joins a highway: the gore's lines and its hatching.

use super::*;

/// The lines of a ramp's join with a highway carriageway at a node, as
/// straight lines from the node: the carriageway's edge line on the ramp's
/// side and the ramp's edge lines and outer asphalt edge.
pub(super) struct GoreLines {
    pub(super) at: Vec2,
    pub(super) dh: Vec2,
    pub(super) dr: Vec2,
    /// Unit normals of each way toward the other.
    pub(super) nh: Vec2,
    pub(super) nr: Vec2,
    /// The carriageway's edge line, the ramp's edge lines and its asphalt
    /// half width.
    pub(super) hh: f32,
    pub(super) hr: f32,
    pub(super) hw_r: f32,
    /// Along the ramp: where its inner edge line has left the carriageway's
    /// (the gore's point), and where its asphalt is clear of the
    /// carriageway's (its own strip begins).
    pub(super) parted: f32,
    pub(super) clear: f32,
    /// [interchange] Length of the ramp's first piece (its straight lead).
    pub(super) len: f32,
}

impl GoreLines {
    pub(super) fn eh(&self, s: f32) -> Vec2 {
        self.at + self.dh * s + self.nh * self.hh
    }
    /// The ramp's inner edge line.
    pub(super) fn er(&self, s: f32) -> Vec2 {
        self.at + self.dr * s + self.nr * self.hr
    }
    /// The ramp's outer asphalt edge, and its outer edge line.
    pub(super) fn eo(&self, s: f32) -> Vec2 {
        self.at + self.dr * s - self.nr * self.hw_r
    }
    pub(super) fn eo_line(&self, s: f32) -> Vec2 {
        self.at + self.dr * s - self.nr * self.hr
    }
    /// How far a point lies out from the carriageway's edge line.
    pub(super) fn out(&self, s: f32, p: Vec2) -> f32 {
        (p - self.eh(s)).dot(self.nh)
    }
}

pub(super) fn gore_lines(roads: &RoadNetwork, node: NodeId, ramp: SegmentId) -> Option<GoreLines> {
    let at = roads.node(node).pos;
    let away = |s: SegmentId| {
        let seg = roads.segment(s);
        let far = if seg.a == node { seg.b } else { seg.a };
        (roads.node(far).pos - at).normalize_or_zero()
    };
    let dr = away(ramp);
    let alive: Vec<SegmentId> = roads.node(node).segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
    // The carriageway on the ramp's side of the node: the one it runs along.
    let hw_seg = alive.iter().copied().filter(|&s| s != ramp).max_by(|&x, &y| away(x).dot(dr).total_cmp(&away(y).dot(dr)))?;
    let dh = away(hw_seg);
    let hw_r = roads.segment(ramp).half_width();
    let (hh, hr) = (roads.segment(hw_seg).half_width() - SHOULDER * 0.5, hw_r - SHOULDER * 0.5);
    let nh = { let n = Vec2::new(-dh.y, dh.x); if n.dot(dr) >= 0.0 { n } else { -n } };
    let nr = { let n = Vec2::new(-dr.y, dr.x); if n.dot(dh) >= 0.0 { n } else { -n } };
    let len = { let (a, b) = roads.segment_points(ramp); a.distance(b) };
    let mut g = GoreLines { at, dh, dr, nh, nr, hh, hr, hw_r, parted: f32::INFINITY, clear: f32::INFINITY, len };
    let mut s = 0.0f32;
    while s <= len.min(150.0) {
        let gap = g.out(s, g.er(s));
        if gap > 0.15 && g.parted.is_infinite() {
            g.parted = s;
        }
        // Clear when the two asphalt edges have parted (each 0.75 m out
        // past its edge line).
        if gap > SHOULDER + 0.05 {
            g.clear = s;
            break;
        }
        s += 0.5;
    }
    g.parted.is_finite().then_some(g)
}

/// Where a ramp joins a highway (or leaves it): the ramp's lane widening
/// out from the carriageway's edge (asphalt from the carriageway's edge line
/// to the ramp's outer edge, with the ramp's outer edge line) until the two
/// are clear of each other, and chevron hatching in the gore beyond its
/// point. The ramp's own strip and markings begin where it is clear
/// (`GoreLines::clear`): drawn from the node on the carriageway's centre
/// line, its edge lines used to cross the carriageway's lanes in an X.
pub(super) fn ramp_gore(g: &mut Geometry, roads: &RoadNetwork, node: NodeId, ramp: SegmentId, asphalt: [f32; 4], paint: [f32; 4]) {
    let Some(gl) = gore_lines(roads, node, ramp) else { return };
    // The widening: from where the ramp's outer edge leaves the carriageway
    // to where the ramp is clear.
    let mut taper_h: Vec<Vec2> = Vec::new();
    let mut taper_o: Vec<Vec2> = Vec::new();
    let mut outline: Vec<Vec2> = Vec::new();
    let end = if gl.clear.is_finite() { gl.clear } else { gl.parted + 40.0 };
    let mut s = 0.0f32;
    while s <= end + 1e-3 {
        if gl.out(s, gl.eo(s)) > 0.0 {
            taper_h.push(gl.eh(s));
            taper_o.push(gl.eo(s));
        }
        if gl.out(s, gl.eo_line(s)) > 0.0 {
            outline.push(gl.eo_line(s));
        }
        s = if s < end && s + 0.5 > end { end } else { s + 0.5 };
    }
    if taper_h.len() >= 2 {
        // Strips of quads: the outline bends where the ramp leaves the
        // carriageway, and one polygon of it is not convex.
        for k in 0..taper_h.len() - 1 {
            g.part(Asphalt).ground_polygon(&[taper_h[k], taper_h[k + 1], taper_o[k + 1], taper_o[k]], ASPHALT_Y + 0.004, asphalt);
        }
    }
    if outline.len() >= 2 {
        g.part(Paint).ground_strip(&outline, 0.18, MARK_Y, paint);
    }
    // The gore beyond its point: paved between the two edge lines, outlined
    // in solid wide lines and hatched with chevrons.
    let (mut side_h, mut side_r): (Vec<Vec2>, Vec<Vec2>) = (Vec::new(), Vec::new());
    let mut s = gl.parted;
    // [interchange] No further than the ramp's first piece runs straight:
    // past its bend the gore's lines ran on over the grass (a short ramp's
    // lead piece is 20 m, the hatching 40 m), a big chevron triangle beside
    // a ramp already turning away.
    let gore_end = (gl.parted + 40.0).min(gl.len);
    while s <= gore_end {
        let (eh, er) = (gl.eh(s), gl.er(s));
        let gap = gl.out(s, er);
        side_h.push(eh);
        side_r.push(er);
        // Chevrons across the gore every 4 m, at road-marking scale:
        // 0.3 m strokes every 3 m read as a grey smear from the top-down
        // camera. Each stroke is 0.6 m wide, and the V's point is drawn 2 m
        // along so the arms sweep at a clear angle.
        let k = s - gl.parted;
        if k >= 1.0 && (k as i32) % 4 == 1 && (k - k.floor()) < 0.25 && gap > 1.2 {
            let mid = (eh + er) * 0.5 + (gl.dh + gl.dr).normalize_or_zero() * 2.0;
            g.part(Paint).ground_strip(&[eh + (er - eh) * 0.1, mid], 0.6, MARK_Y + 0.004, paint);
            g.part(Paint).ground_strip(&[mid, er - (er - eh) * 0.1], 0.6, MARK_Y + 0.004, paint);
        }
        s += 0.5;
    }
    if side_h.len() >= 2 {
        // Above the widening's asphalt, which it overlaps up to `clear`.
        for k in 0..side_h.len() - 1 {
            g.part(Asphalt).ground_polygon(&[side_h[k], side_h[k + 1], side_r[k + 1], side_r[k]], ASPHALT_Y + 0.007, asphalt);
        }
        g.part(Paint).ground_strip(&side_h, 0.3, MARK_Y + 0.002, paint);
        g.part(Paint).ground_strip(&side_r, 0.3, MARK_Y + 0.002, paint);
    }
}
