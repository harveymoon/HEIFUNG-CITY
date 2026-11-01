//! What a style generator is handed: the context and the extras.

use super::*;

/// Everything a style generator needs.
pub struct Ctx<'a> {
    pub frame: Frame,
    pub rules: &'a LandUseRules,
    pub style: &'a TierStyle,
    pub palette: &'a Palette,
    pub land_use: LandUse,
    pub tier: Tier,
    pub floors: u8,
    /// 0 = pristine, 1 = ruined.
    pub wear: f32,
    /// Bounding-rectangle footprint size after inset.
    pub fw: f32,
    pub fd: f32,
    /// Exact parcel outline in lot coordinates, counter-clockwise.
    pub lot_poly: Vec<Vec2>,
    /// Buildable footprint: the parcel inset by the land use's setback.
    pub poly: Vec<Vec2>,
    pub seed: u64,
    /// For parking lots: share of bays holding a car.
    pub parking: Option<f32>,
    /// Share of the building's electricity demand met: dark windows when low.
    pub powered: f32,
    /// For parks: the sim's plan of paths, benches and features.
    pub park: Option<ParkLayout>,
    /// For stations and yards: the track and platform beside the lot.
    pub rail: Option<RailLink>,
    /// Wind farm rotors are left out for the renderer to spin.
    pub animated_rotors: bool,
    /// A solar or wind plant whose parcel is one of several touching ones:
    /// drawn as part of the whole field (see [`PlantField`]).
    pub field: Option<&'a PlantField>,
    /// Tower crane heads are left out of the mesh and handed over in
    /// `Geometry::cranes` for the renderer to slew (see [`CraneRig`]).
    pub animated_cranes: bool,
    /// Window lights are switched by the renderer from occupancy: nearly
    /// every window is drawn as a marked light over its glass (see
    /// `parts::window_light`), and `lit_chance` ignores power.
    pub dynamic_lights: bool,
    /// A grid battery: its size, share full and flow (MW, positive
    /// delivering), for the module count and the status lights.
    pub battery: Option<(nd_sim::BatteryKind, f32, f32)>,
    /// [plant-scale] See [`Extras::plant_scale`].
    pub plant_scale: f32,
    /// [waterfront] See [`Extras::shore`].
    pub shore: Option<ShoreLook>,
    /// [attractions] See [`Extras::pier`].
    pub pier: Option<nd_sim::attractions::PierDeck>,
    /// [parks] See [`Extras::park_look`].
    pub park_look: Option<super::park::ParkLook>,
    /// [K34] The real boxes a Stacks home is built from, in building order.
    pub boxes: Vec<nd_sim::containers::BoxFace>,
    /// [K34] A depot's stacks are the renderer's (see `Extras::real_boxes`).
    pub real_boxes: bool,
    /// [r5-layered] See [`Extras::overhead`].
    pub overhead: Vec<[Vec2; 4]>,
    /// [r5-trash] See [`Extras::waste_fill`].
    pub waste_fill: f32,
}

/// Per-building state that changes the look beyond style and condition.
#[derive(Clone, Copy, Debug)]
pub struct Extras<'a> {
    pub parking: Option<f32>,
    pub plant: Option<PowerPlantKind>,
    pub powered: f32,
    pub park: Option<&'a ParkLayout>,
    /// A bus depot.
    pub depot: bool,
    pub rail: Option<&'a RailLink>,
    /// A shipping port or ferry terminal.
    pub port: Option<nd_sim::ports::PortKind>,
    /// Still being built: progress in `0..1`. The site is drawn instead of
    /// the finished style.
    pub construction: Option<f32>,
    /// Coming down: progress in `0..1`. The building (as described by the
    /// other arguments) is drawn cut down to what is left, with the
    /// demolition site around it.
    pub demolition: Option<f32>,
    /// Styles of the buildings already standing nearby (see
    /// [`StyleBook`]); the style pick clusters or contrasts with them.
    pub neighbours: &'a [Neighbour],
    /// A city service or attraction: its own style, at the placed tier.
    pub service: Option<nd_sim::services::ServiceKind>,
    /// Leave wind farm rotors out: the renderer draws and spins them.
    pub animated_rotors: bool,
    /// A solar or wind plant drawn as part of a field of touching parcels.
    pub field: Option<&'a PlantField>,
    /// Hand tower crane heads over in `Geometry::cranes` instead of
    /// drawing them, for the renderer to slew (see [`Ctx::animated_cranes`]).
    pub animated_cranes: bool,
    /// Draw window lights for the renderer to switch by occupancy (see
    /// [`Ctx::dynamic_lights`]).
    pub dynamic_lights: bool,
    /// The sim gave this tower a rooftop pad (`nd_sim::air`): a pad-capable
    /// style is picked and a helideck drawn on its roof.
    pub rooftop_pad: bool,
    /// The business on the lot: industry sub-zones have their own looks.
    pub business: Option<nd_sim::BusinessKind>,
    /// [subway] A metro station: drawn as its entrance plaza.
    pub metro: bool,
    /// [land] The form bought or merged plots were built in: McMansions,
    /// mansions, mega hotels and museums have their own style.
    pub form: Option<nd_sim::estate::Form>,
    /// A truck and bus lot: share of its bays in use.
    pub fleet: f32,
    /// [districts] The district architecture the building was put up in:
    /// its own style menu and palette.
    pub look: Option<nd_sim::districts::Look>,
    /// [power] A grid battery: size, share full and flow (see
    /// [`Ctx::battery`]).
    pub battery: Option<(nd_sim::BatteryKind, f32, f32)>,
    /// [plant-scale] A plant's size as a share of its kind's nameplate
    /// (`World::plant_scale`): a coal or nuclear station is composed of
    /// more units, not stretched, when it is bigger.
    pub plant_scale: f32,
    /// [supertall] A league tower: its height and form.
    pub tower: Option<nd_sim::towers::Tower>,
    /// [megablocks] A mega-corp's arcology: its plan (towers, storeys).
    pub megablock: Option<nd_sim::megablock::MegaBlock>,
    /// [megacorp] The owner's brand and where it is shown (`World::brand_mark`).
    pub brand: Option<nd_sim::corp::BrandMark>,
    /// [megacorp] A campus annex: its own look (`Style::for_annex`).
    pub annex: Option<nd_sim::corp::Annex>,
    /// [waterfront] Where the water is for a lot on it, and a beach's crowd.
    pub shore: Option<ShoreLook>,
    /// [attractions] A pleasure pier's deck out over the water (world
    /// coordinates, `World::attractions`): drawn on piles beyond the lot.
    pub pier: Option<&'a nd_sim::attractions::PierDeck>,
    /// [parks] (D5) A park's litter and dealers (`World::parks`).
    pub park_look: Option<super::park::ParkLook>,
    /// [tenants] The units it is let in (`nd_sim::buildings::Unit`): a sign over each
    /// shopfront.
    pub units: &'a [nd_sim::buildings::Unit],
    /// [K34] A Stacks home a stack developer built from real boxes: the
    /// records, the building and the day (for their wear). Forces the
    /// Stacks style and draws those boxes.
    pub boxes_of: Option<(&'a nd_sim::containers::Containers, nd_sim::BuildingId, i64)>,
    /// [K34] A depot whose boxes are real (`nd_sim::containers`): its
    /// stacks are left out and their slots handed to the renderer
    /// (`Geometry::box_slots`), which stands the boxes stored there.
    pub real_boxes: bool,
    /// [r5-layered] The footprints of the raised decks over the lot (world
    /// coordinates, `undercroft::deck_quads`): a park plants no tree and
    /// puts up no wheel or bandstand under one.
    pub overhead: &'a [[Vec2; 4]],
    /// [r5-trash] A landfill's share full (`World::landfill_fill`): its
    /// mound grows with it.
    pub waste_fill: f32,
}

impl Default for Extras<'_> {
    fn default() -> Self {
        Self { plant_scale: 1.0, parking: None, plant: None, powered: 1.0, park: None, depot: false, rail: None, port: None, construction: None, demolition: None, neighbours: &[], service: None, animated_rotors: false, field: None, animated_cranes: false, dynamic_lights: false, business: None, fleet: 0.5, rooftop_pad: false, look: None, metro: false, battery: None, form: None, tower: None, megablock: None, brand: None, annex: None, shore: None, pier: None, park_look: None, units: &[], boxes_of: None, real_boxes: false, overhead: &[], waste_fill: 0.0 }
    }
}

/// The rules, tier look and palette a building is drawn with: the land
/// use's and the tier's own, or a district look's tint of them.
#[derive(Clone, Copy)]
pub struct Finish<'a> {
    pub rules: &'a LandUseRules,
    pub style: &'a TierStyle,
    pub palette: &'a Palette,
}

impl<'a> Ctx<'a> {
    /// The context `style` is drawn in on `lot`: the lot's frame and
    /// outline, the footprint inset by the land use's setback, and the
    /// extras that change the look. Rail styles without a railway get a
    /// stand-in track behind the lot, parks without a plan the sim's
    /// single-lot plan.
    #[allow(clippy::too_many_arguments)]
    pub fn new(lot: &Lot, style: Style, land_use: LandUse, floors: u8, tier: Tier, condition: f32, seed: u64, finish: Finish<'a>, extras: &Extras<'a>) -> Self {
        let (frame, lot_poly) = Frame::lot_local(lot);
        let r = finish.rules;
        let poly = parts::offset_poly(&lot_poly, r.inset);
        Ctx {
            fw: (frame.width - 2.0 * r.inset).max(4.0),
            fd: (frame.depth - 2.0 * r.inset).max(4.0),
            frame,
            rules: r,
            style: finish.style,
            palette: finish.palette,
            land_use,
            tier,
            floors: floors.max(1),
            wear: (1.0 - condition).clamp(0.0, 1.0),
            lot_poly,
            poly,
            seed,
            // A truck lot shows its fleet the way a car park shows its cars.
            parking: extras.parking.or((style == Style::TruckLot).then_some(extras.fleet)),
            powered: extras.powered,
            rail: extras.rail.cloned().or_else(|| style.rail_kind().map(|k| super::back_edge_link(lot, k))),
            park: if land_use == LandUse::Park || matches!(style, Style::ScrubLot | Style::Garden | Style::Plaza) {
                Some(extras.park.cloned().unwrap_or_else(|| nd_sim::park::plan_single(lot, tier)))
            } else if let Some(k) = extras.service.filter(|k| k.has_grounds()) {
                // [attractions] A landmark's own walkways (the sim's plan,
                // or the same plan made here for a preview).
                extras.park.cloned().or_else(|| nd_sim::attractions::plan_grounds(k, lot, extras.pier))
            } else {
                None
            },
            animated_rotors: extras.animated_rotors,
            field: extras.field,
            animated_cranes: extras.animated_cranes,
            dynamic_lights: extras.dynamic_lights,
            battery: extras.battery,
            plant_scale: extras.plant_scale,
            shore: extras.shore,
            pier: extras.pier.cloned(),
            park_look: extras.park_look,
            boxes: extras.boxes_of.map(|(c, b, day)| c.home_faces(b, day)).unwrap_or_default(),
            real_boxes: extras.real_boxes,
            overhead: extras.overhead.to_vec(),
            waste_fill: extras.waste_fill,
        }
    }
}

impl Ctx<'_> {
    pub fn fh(&self) -> f32 {
        self.rules.floor_height
    }

    pub fn height(&self) -> f32 {
        self.floors.max(1) as f32 * self.fh()
    }

    /// Chance a window is lit, clamped so rule files cannot push it past 1.
    pub fn lit_chance(&self) -> f32 {
        if self.dynamic_lights {
            // Most windows can light; which ones do is the renderer's call.
            return (0.92 * (1.0 - 0.3 * self.wear)).clamp(0.0, 1.0);
        }
        (self.style.lit_chance * (1.0 - 0.8 * self.wear) * self.powered.clamp(0.08, 1.0)).clamp(0.0, 1.0)
    }

    /// Whether a lot-local point lies on the parcel. World points go through
    /// `Frame::to_local` first (or `park::Park::inside_world`).
    pub fn inside_local(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.lot_poly)
    }

    /// Whether the straight run from `a` to `b` stays over the lot: a wire
    /// or a string of lights across a concave lot's notch would not.
    pub fn spans_lot(&self, a: Vec2, b: Vec2) -> bool {
        (0..=8).all(|k| self.inside_local(a.lerp(b, k as f32 / 8.0)))
    }

    /// Distance from the middle of a face, outward, to the lot boundary: the
    /// room a canopy, porch or forecourt has before it spills onto the street.
    pub fn reach_out(&self, face: &Face) -> f32 {
        let o = face.mid();
        let d = face.out;
        let n = self.lot_poly.len();
        let mut best = f32::MAX;
        for i in 0..n {
            let a = self.lot_poly[i];
            let b = self.lot_poly[(i + 1) % n];
            let e = b - a;
            let denom = d.perp_dot(e);
            if denom.abs() < 1e-6 {
                continue;
            }
            let t = (a - o).perp_dot(e) / denom;
            let u = (a - o).perp_dot(d) / denom;
            if t >= 0.0 && (0.0..=1.0).contains(&u) {
                best = best.min(t);
            }
        }
        if best == f32::MAX { 0.0 } else { best }
    }

    /// Faces of a lot-local polygon.
    pub fn faces(&self, poly: &[Vec2]) -> Vec<Face> {
        let n = poly.len();
        (0..n).map(|i| Face::from_edge(poly[i], poly[(i + 1) % n])).collect()
    }

    /// Index of the face of a polygon that looks at the road: the longest
    /// edge facing -v. Fences and hoardings leave that edge for the gate.
    pub fn front_face_index(&self, poly: &[Vec2]) -> Option<usize> {
        let faces = self.faces(poly);
        (0..faces.len()).max_by(|&i, &j| {
            let si = faces[i].out.dot(Vec2::new(0.0, -1.0)) * faces[i].len;
            let sj = faces[j].out.dot(Vec2::new(0.0, -1.0)) * faces[j].len;
            si.total_cmp(&sj)
        })
    }

    /// The face of a polygon that looks at the road.
    pub fn front_face(&self, poly: &[Vec2]) -> Face {
        let faces = self.faces(poly);
        self.front_face_index(poly).map(|i| faces[i]).unwrap_or(Face::from_edge(Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0)))
    }

    /// Random point inside the parcel at least `margin` from its edges.
    pub fn random_inside(&self, rng: &mut impl Rng, margin: f32) -> Option<Vec2> {
        parts::random_in_poly(rng, &self.lot_poly, margin)
    }
}
