//! The NPC **Motivation** engine (ADR-0004): one selection rule — *most-immediate
//! actionable option* — applied at three levels (node→**Bid**, Bid→**Goal**,
//! Action→**Plan**), with **Pressure** modulating only goal arbitration.
//!
//! This module is **pure**: it has no ECS and no clock of its own. The caller
//! supplies a [`Perception`] (what the NPC senses within its cluster-local range)
//! and the NPC's persistent [`Drives`]; [`decide`] advances the Drives and returns
//! one [`Decision`] for the tick. Fully deterministic — no wall-clock, no RNG
//! (any tie or wander direction is resolved by the ECS layer with a seeded PRNG).

/// A point in world sub-units (1 unit = 1000 sub-units), matching the sim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct P2 {
    pub x: i64,
    pub y: i64,
}

impl P2 {
    pub fn new(x: i64, y: i64) -> Self {
        P2 { x, y }
    }
}

/// The kinds of NPC in v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcKind {
    Wolf,
    Deer,
}

impl NpcKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NpcKind::Wolf => "wolf",
            NpcKind::Deer => "deer",
        }
    }
}

/// One entity the NPC senses, with the wire/actor id used to target it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sensed {
    pub id: u64,
    pub pos: P2,
}

/// What an NPC senses this tick. Everything here is within the NPC's perception
/// range, which by ADR-0005 is ≤ chunk_size — so it is all inside the NPC's own
/// cluster, and reading it never crosses a cluster boundary.
#[derive(Clone, Debug, Default)]
pub struct Perception {
    pub self_pos: P2,
    /// Acute threat: the NPC took damage this tick (a Player clicked it, a rival bit it).
    pub being_attacked: bool,
    /// Menacing entities in range (predators, attacking Players).
    pub threats: Vec<Sensed>,
    /// Huntable prey in range (wolves only).
    pub prey: Vec<Sensed>,
    /// Edible food in range (carcasses) the NPC could eat from.
    pub food: Vec<Sensed>,
    /// Same-species competitors contesting nearby food (rival wolves).
    pub rivals: Vec<Sensed>,
    /// Same-species peers for social steering (deer herd; wolf pack).
    pub herd: Vec<Sensed>,
    /// Local grass level 0..1 (deer grazing substrate).
    pub grass: f64,
    /// Day/night phase 0 (midday) .. 1 (midnight) — the "nightness" of the world
    /// right now. Agent extension (EXTENSIONS.md): modulates temperament.
    pub phase: f64,
}

impl Perception {
    pub fn at(self_pos: P2) -> Self {
        Perception { self_pos, ..Default::default() }
    }
}

/// The NPC's persistent motivational state, carried between ticks (an ECS
/// component). `hunger` is the need *level*; the `_pressure` fields are the
/// leaky, capped integrals of each Need's activation (ADR-0004).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drives {
    /// Hunger need level, 0 (sated) .. 1 (starving).
    pub hunger: f64,
    /// Accumulated hunger Pressure, 0..cap.
    pub hunger_pressure: f64,
    /// Accumulated safety Pressure, 0..cap.
    pub safety_pressure: f64,
}

impl Drives {
    /// Eating lowers the hunger level (clamped at sated).
    pub fn feed(&mut self, amount: f64) {
        self.hunger = (self.hunger - amount).max(0.0);
    }
}

/// The single Action the engine commits to this tick — the head of the Plan.
/// The ECS layer turns this into movement Intent plus an optional verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// No active need — stand still.
    Idle,
    /// No active need worth a destination — drift (ECS picks a seeded direction).
    Wander,
    /// Move toward a point (seek food/grass we can't yet act on).
    Approach(P2),
    /// Move directly away from this point (flee a threat).
    Flee(P2),
    /// Close on and strike a target (hunt prey, or fight-to-hold against a contester).
    Attack(u64, P2),
    /// Consume from a food source in range.
    Eat(u64, P2),
    /// Graze grass in place (deer).
    Graze,
}

/// Tunable Motivation parameters. Per-kind via [`Params::for_kind`].
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Hunger level gained per second of not eating.
    pub metabolism_per_s: f64,
    /// Pressure time-constants (seconds) and shared cap.
    pub hunger_tau_s: f64,
    pub safety_tau_s: f64,
    pub pressure_cap: f64,
    /// Movement speed in sub-units/sec when pursuing a Decision.
    pub speed: f64,
    /// Static inter-chain priority bias (safety > hunger).
    pub hunger_bias: f64,
    pub safety_bias: f64,
    /// Perception range squared (sub-units²) — bounds threat proximity scaling.
    pub perception_range_sq: i64,
    /// Social-sense range² for herd/pack peers (agent extension). Wider than
    /// perception but ≤ chunk_size², so peers are still co-clustered.
    pub social_range_sq: i64,
    /// Range² within which food can be eaten / a contester triggers fight-to-hold.
    pub eat_range_sq: i64,
    /// Grass level below which a deer cannot graze and must seek.
    pub graze_floor: f64,
    /// Need scores below this count as "no active need".
    pub idle_eps: f64,
    /// Range² beyond which a calm animal steers toward its herd centroid
    /// (0 disables cohesion). Agent extension — see EXTENSIONS.md.
    pub herd_comfort_sq: i64,
}

impl Params {
    pub fn for_kind(kind: NpcKind) -> Self {
        match kind {
            NpcKind::Wolf => Params {
                metabolism_per_s: 1.0 / 60.0,
                hunger_tau_s: 60.0,
                safety_tau_s: 10.0,
                pressure_cap: 1.0,
                speed: 4_200.0,
                hunger_bias: 1.0,
                safety_bias: 1.2,
                perception_range_sq: 1_000 * 1_000,
                social_range_sq: 5_000 * 5_000,
                eat_range_sq: 600 * 600,
                graze_floor: 0.0,
                idle_eps: 0.05,
                herd_comfort_sq: 0, // wolves don't herd (they pack-hunt instead)
            },
            NpcKind::Deer => Params {
                metabolism_per_s: 1.0 / 90.0,
                hunger_tau_s: 90.0,
                safety_tau_s: 8.0,
                pressure_cap: 1.0,
                speed: 3_800.0,
                hunger_bias: 1.0,
                safety_bias: 1.5,
                perception_range_sq: 1_000 * 1_000,
                social_range_sq: 5_000 * 5_000,
                eat_range_sq: 600 * 600,
                graze_floor: 0.05,
                idle_eps: 0.05,
                herd_comfort_sq: 2_000 * 2_000,
            },
        }
    }
}

fn dist_sq(a: P2, b: P2) -> i64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

fn nearest(from: P2, xs: &[Sensed]) -> Option<Sensed> {
    xs.iter().copied().min_by_key(|s| dist_sq(from, s.pos))
}

/// Leaky, capped integrator (ADR-0004): tracks `cap·activation` with time
/// constant `tau`, so sustained activation saturates and quiet decays to zero.
fn integrate(p: f64, activation: f64, dt_s: f64, tau_s: f64, cap: f64) -> f64 {
    let target = cap * activation.clamp(0.0, 1.0);
    let alpha = 1.0 - (-dt_s / tau_s).exp();
    (p + (target - p) * alpha).clamp(0.0, cap)
}

/// Immediate threat activation 0..1: acute if just bitten, else scaled by the
/// nearest threat's proximity within perception range.
fn threat_activation(perc: &Perception, params: &Params) -> f64 {
    if perc.being_attacked {
        return 1.0;
    }
    match nearest(perc.self_pos, &perc.threats) {
        None => 0.0,
        Some(t) => {
            let d = dist_sq(perc.self_pos, t.pos) as f64;
            let r = params.perception_range_sq as f64;
            (1.0 - d / r).clamp(0.0, 1.0)
        }
    }
}

/// Advance `drives` and choose this tick's [`Decision`]. `dt_s` is the tick
/// length in seconds. See ADR-0004 for the arbitration.
pub fn decide(
    kind: NpcKind,
    perc: &Perception,
    drives: &mut Drives,
    params: &Params,
    dt_s: f64,
) -> Decision {
    // 1. Need levels advance: hunger rises with metabolism.
    drives.hunger = (drives.hunger + params.metabolism_per_s * dt_s).clamp(0.0, 1.0);

    // 2. Immediate activations.
    let hunger_act = drives.hunger;
    let threat_act = threat_activation(perc, params);

    // 3. Pressure integrals (the strategist; modulates only step 4).
    drives.hunger_pressure =
        integrate(drives.hunger_pressure, hunger_act, dt_s, params.hunger_tau_s, params.pressure_cap);
    drives.safety_pressure =
        integrate(drives.safety_pressure, threat_act, dt_s, params.safety_tau_s, params.pressure_cap);

    // 4. Goal arbitration: bias × immediacy × (1 + pressure). Diurnal temperament
    //    (agent extension) tilts the bias by the day/night phase: wolves bolder at
    //    night, deer warier.
    const NIGHT_TILT: f64 = 0.6;
    let night = perc.phase.clamp(0.0, 1.0);
    let (hunger_bias, safety_bias) = match kind {
        NpcKind::Wolf => (params.hunger_bias * (1.0 + NIGHT_TILT * night), params.safety_bias),
        NpcKind::Deer => (params.hunger_bias, params.safety_bias * (1.0 + NIGHT_TILT * night)),
    };
    let hunger_score = hunger_act * hunger_bias * (1.0 + drives.hunger_pressure);
    let safety_score = threat_act * safety_bias * (1.0 + drives.safety_pressure);

    let safety_active = safety_score > params.idle_eps && threat_act > 0.0;
    let hunger_active = hunger_score > params.idle_eps;

    let decision = if safety_active && safety_score >= hunger_score {
        plan_safety(perc)
    } else if hunger_active {
        plan_hunger(kind, perc, params)
    } else {
        Decision::Wander
    };

    // Herd cohesion (agent extension, EXTENSIONS.md): a calm animal beyond its
    // comfort radius drifts toward the centroid of its herd. Never overrides
    // fleeing — a threat scatters the herd, then it reforms.
    if params.herd_comfort_sq > 0 && !matches!(decision, Decision::Flee(_)) {
        if let Some(c) = herd_centroid(perc) {
            if dist_sq(perc.self_pos, c) > params.herd_comfort_sq {
                return Decision::Approach(c);
            }
        }
    }
    decision
}

/// The pack's focal point: the centroid of this wolf and its packmates, or just
/// itself when alone. Agent extension (EXTENSIONS.md) — pulls a hunt toward a
/// shared focal prey.
fn pack_focus(perc: &Perception) -> P2 {
    if perc.herd.is_empty() {
        return perc.self_pos;
    }
    let n = (perc.herd.len() + 1) as i64;
    let sx: i64 = perc.self_pos.x + perc.herd.iter().map(|s| s.pos.x).sum::<i64>();
    let sy: i64 = perc.self_pos.y + perc.herd.iter().map(|s| s.pos.y).sum::<i64>();
    P2::new(sx / n, sy / n)
}

/// The integer mean position of an animal's herd peers, if any.
fn herd_centroid(perc: &Perception) -> Option<P2> {
    if perc.herd.is_empty() {
        return None;
    }
    let n = perc.herd.len() as i64;
    let sx: i64 = perc.herd.iter().map(|s| s.pos.x).sum();
    let sy: i64 = perc.herd.iter().map(|s| s.pos.y).sum();
    Some(P2::new(sx / n, sy / n))
}

/// Safety goal: flee the nearest threat.
fn plan_safety(perc: &Perception) -> Decision {
    match nearest(perc.self_pos, &perc.threats) {
        Some(t) => Decision::Flee(t.pos),
        None => Decision::Wander,
    }
}

/// Hunger goal: climb the chain by precondition-gated immediacy, with the plan
/// adapting to a contested food source (fight-to-hold).
fn plan_hunger(kind: NpcKind, perc: &Perception, params: &Params) -> Decision {
    match kind {
        NpcKind::Wolf => {
            // Most-immediate actionable: eat food in range.
            if let Some(f) = nearest(perc.self_pos, &perc.food) {
                if dist_sq(perc.self_pos, f.pos) <= params.eat_range_sq {
                    // Eat-calmly is blocked if a contester is on the carcass → fight-to-hold.
                    if let Some(c) = nearest_contester(perc, params) {
                        return Decision::Attack(c.id, c.pos);
                    }
                    return Decision::Eat(f.id, f.pos);
                }
                // Food sensed but out of reach → approach it.
                return Decision::Approach(f.pos);
            }
            // No food: hunt prey. Pack focus (agent extension) — when packmates
            // are near, target the prey nearest the pack centroid so the pack
            // converges on one animal instead of splitting up.
            if let Some(p) = nearest(pack_focus(perc), &perc.prey) {
                return Decision::Attack(p.id, p.pos);
            }
            Decision::Wander
        }
        NpcKind::Deer => {
            if perc.grass > params.graze_floor {
                Decision::Graze
            } else {
                Decision::Wander
            }
        }
    }
}

/// The nearest threat or rival contesting the NPC's food, within eat range.
fn nearest_contester(perc: &Perception, params: &Params) -> Option<Sensed> {
    perc.threats
        .iter()
        .chain(perc.rivals.iter())
        .copied()
        .filter(|s| dist_sq(perc.self_pos, s.pos) <= params.eat_range_sq)
        .min_by_key(|s| dist_sq(perc.self_pos, s.pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 0.05; // one 20 Hz tick

    fn wolf() -> (Params, Drives) {
        (Params::for_kind(NpcKind::Wolf), Drives::default())
    }

    #[test]
    fn idle_npc_with_no_needs_wanders() {
        let (params, mut d) = wolf();
        let perc = Perception::at(P2::new(0, 0));
        // One fresh tick: hunger barely above zero, no threat → below idle_eps.
        let got = decide(NpcKind::Wolf, &perc, &mut d, &params, DT);
        assert_eq!(got, Decision::Wander);
    }

    #[test]
    fn hunger_rises_with_metabolism() {
        let (params, mut d) = wolf();
        let perc = Perception::at(P2::new(0, 0));
        let before = d.hunger;
        decide(NpcKind::Wolf, &perc, &mut d, &params, DT);
        assert!(d.hunger > before, "hunger should rise each tick");
    }

    #[test]
    fn deer_grazes_when_hungry_on_grass() {
        let params = Params::for_kind(NpcKind::Deer);
        let mut d = Drives { hunger: 0.5, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.grass = 1.0;
        assert_eq!(decide(NpcKind::Deer, &perc, &mut d, &params, DT), Decision::Graze);
    }

    #[test]
    fn deer_seeks_when_hungry_without_grass() {
        let params = Params::for_kind(NpcKind::Deer);
        let mut d = Drives { hunger: 0.5, ..Default::default() };
        let perc = Perception::at(P2::new(0, 0)); // grass 0
        assert_eq!(decide(NpcKind::Deer, &perc, &mut d, &params, DT), Decision::Wander);
    }

    #[test]
    fn deer_flees_threat_even_when_hungry() {
        // Safety bias beats hunger: a hungry deer on grass still flees a near wolf.
        let params = Params::for_kind(NpcKind::Deer);
        let mut d = Drives { hunger: 0.8, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.grass = 1.0;
        perc.threats = vec![Sensed { id: 7, pos: P2::new(300, 0) }];
        assert_eq!(decide(NpcKind::Deer, &perc, &mut d, &params, DT), Decision::Flee(P2::new(300, 0)));
    }

    #[test]
    fn wolf_hunts_perceived_prey() {
        let (params, _) = wolf();
        let mut d = Drives { hunger: 0.7, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.prey = vec![Sensed { id: 3, pos: P2::new(900, 0) }];
        assert_eq!(decide(NpcKind::Wolf, &perc, &mut d, &params, DT), Decision::Attack(3, P2::new(900, 0)));
    }

    #[test]
    fn wolf_eats_food_in_range() {
        let (params, _) = wolf();
        let mut d = Drives { hunger: 0.7, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.food = vec![Sensed { id: 5, pos: P2::new(200, 0) }];
        assert_eq!(decide(NpcKind::Wolf, &perc, &mut d, &params, DT), Decision::Eat(5, P2::new(200, 0)));
    }

    #[test]
    fn wolf_approaches_distant_food() {
        let (params, _) = wolf();
        let mut d = Drives { hunger: 0.7, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.food = vec![Sensed { id: 5, pos: P2::new(900, 0) }]; // beyond eat range
        assert_eq!(decide(NpcKind::Wolf, &perc, &mut d, &params, DT), Decision::Approach(P2::new(900, 0)));
    }

    #[test]
    fn starving_wolf_fights_to_hold_contested_carcass() {
        // High hunger pressure lifts hunger past safety: it attacks the contester.
        let (params, _) = wolf();
        let mut d = Drives { hunger: 1.0, hunger_pressure: 1.0, safety_pressure: 0.0 };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.food = vec![Sensed { id: 5, pos: P2::new(150, 0) }];
        perc.rivals = vec![Sensed { id: 9, pos: P2::new(200, 0) }];
        perc.being_attacked = true;
        assert_eq!(decide(NpcKind::Wolf, &perc, &mut d, &params, DT), Decision::Attack(9, P2::new(200, 0)));
    }

    #[test]
    fn calm_wolf_flees_threat_abandoning_food() {
        // Same situation, low hunger pressure: safety wins, it flees.
        let (params, _) = wolf();
        let mut d = Drives { hunger: 0.5, hunger_pressure: 0.0, safety_pressure: 0.0 };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.food = vec![Sensed { id: 5, pos: P2::new(150, 0) }];
        perc.threats = vec![Sensed { id: 9, pos: P2::new(200, 0) }];
        perc.being_attacked = true;
        assert_eq!(decide(NpcKind::Wolf, &perc, &mut d, &params, DT), Decision::Flee(P2::new(200, 0)));
    }

    #[test]
    fn sustained_hunger_builds_then_caps_pressure() {
        let (params, mut d) = wolf();
        d.hunger = 1.0;
        let perc = Perception::at(P2::new(0, 0));
        for _ in 0..4000 {
            d.hunger = 1.0; // pin starving (~200s ≈ 3.3·τ)
            decide(NpcKind::Wolf, &perc, &mut d, &params, DT);
        }
        assert!(d.hunger_pressure > 0.9, "got {}", d.hunger_pressure);
        assert!(d.hunger_pressure <= params.pressure_cap);
    }

    #[test]
    fn pressure_decays_when_sated() {
        let (params, mut d) = wolf();
        d.hunger_pressure = 1.0;
        let perc = Perception::at(P2::new(0, 0));
        for _ in 0..4000 {
            d.feed(1.0); // stay sated (~200s ≈ 3.3·τ)
            decide(NpcKind::Wolf, &perc, &mut d, &params, DT);
        }
        assert!(d.hunger_pressure < 0.05, "got {}", d.hunger_pressure);
    }

    #[test]
    fn wolf_is_bolder_at_night() {
        // A wolf at the carcass under threat: by day safety wins (flee); by night
        // its boldness lifts hunger over safety (fight-to-hold). Only `phase` differs.
        let (params, _) = wolf();
        let make = |phase: f64| {
            let mut perc = Perception::at(P2::new(0, 0));
            perc.phase = phase;
            perc.food = vec![Sensed { id: 5, pos: P2::new(150, 0) }];
            perc.threats = vec![Sensed { id: 9, pos: P2::new(200, 0) }];
            perc.rivals = vec![Sensed { id: 9, pos: P2::new(200, 0) }];
            perc.being_attacked = true;
            perc
        };
        let mut day_d = Drives { hunger: 0.8, ..Default::default() };
        let mut night_d = Drives { hunger: 0.8, ..Default::default() };
        assert_eq!(
            decide(NpcKind::Wolf, &make(0.0), &mut day_d, &params, DT),
            Decision::Flee(P2::new(200, 0))
        );
        assert_eq!(
            decide(NpcKind::Wolf, &make(1.0), &mut night_d, &params, DT),
            Decision::Attack(9, P2::new(200, 0))
        );
    }

    #[test]
    fn deer_is_warier_at_night() {
        // A deer with a mild, distant threat grazes by day but flees at night.
        let params = Params::for_kind(NpcKind::Deer);
        let make = |phase: f64| {
            let mut perc = Perception::at(P2::new(0, 0));
            perc.phase = phase;
            perc.grass = 1.0;
            perc.threats = vec![Sensed { id: 9, pos: P2::new(840, 0) }]; // mild proximity
            perc
        };
        let mut day_d = Drives { hunger: 0.5, ..Default::default() };
        let mut night_d = Drives { hunger: 0.5, ..Default::default() };
        assert_eq!(decide(NpcKind::Deer, &make(0.0), &mut day_d, &params, DT), Decision::Graze);
        assert_eq!(
            decide(NpcKind::Deer, &make(1.0), &mut night_d, &params, DT),
            Decision::Flee(P2::new(840, 0))
        );
    }

    #[test]
    fn lone_wolf_hunts_the_prey_nearest_itself() {
        let (params, _) = wolf();
        let mut d = Drives { hunger: 0.8, ..Default::default() };
        let mut perc = Perception::at(P2::new(4_000, 0));
        perc.prey = vec![
            Sensed { id: 1, pos: P2::new(1_800, 0) },
            Sensed { id: 2, pos: P2::new(3_000, 0) },
        ];
        // No pack → nearest to self (3000 is closer to 4000).
        assert_eq!(
            decide(NpcKind::Wolf, &perc, &mut d, &params, DT),
            Decision::Attack(2, P2::new(3_000, 0))
        );
    }

    #[test]
    fn pack_wolf_targets_prey_nearest_the_pack_centroid() {
        let (params, _) = wolf();
        let mut d = Drives { hunger: 0.8, ..Default::default() };
        let mut perc = Perception::at(P2::new(4_000, 0));
        perc.prey = vec![
            Sensed { id: 1, pos: P2::new(1_800, 0) },
            Sensed { id: 2, pos: P2::new(3_000, 0) },
        ];
        // A packmate at the origin pulls the focus to (2000,0): prey 1 is nearer.
        perc.herd = vec![Sensed { id: 99, pos: P2::new(0, 0) }];
        assert_eq!(
            decide(NpcKind::Wolf, &perc, &mut d, &params, DT),
            Decision::Attack(1, P2::new(1_800, 0))
        );
    }

    #[test]
    fn calm_deer_steers_toward_its_herd() {
        let params = Params::for_kind(NpcKind::Deer);
        let mut d = Drives::default();
        let mut perc = Perception::at(P2::new(0, 0));
        perc.grass = 1.0; // could graze, but the herd is far → go join it
        perc.herd = vec![
            Sensed { id: 1, pos: P2::new(5_000, 0) },
            Sensed { id: 2, pos: P2::new(5_000, 200) },
        ];
        match decide(NpcKind::Deer, &perc, &mut d, &params, DT) {
            Decision::Approach(p) => assert!(p.x > 0, "steers toward the herd, got {p:?}"),
            other => panic!("expected Approach(herd), got {other:?}"),
        }
    }

    #[test]
    fn deer_within_comfort_radius_grazes_not_chases() {
        let params = Params::for_kind(NpcKind::Deer);
        let mut d = Drives { hunger: 0.5, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.grass = 1.0;
        perc.herd = vec![Sensed { id: 1, pos: P2::new(800, 0) }]; // already close
        assert_eq!(decide(NpcKind::Deer, &perc, &mut d, &params, DT), Decision::Graze);
    }

    #[test]
    fn threatened_deer_ignores_herd_and_flees() {
        let params = Params::for_kind(NpcKind::Deer);
        let mut d = Drives { hunger: 0.3, ..Default::default() };
        let mut perc = Perception::at(P2::new(0, 0));
        perc.herd = vec![Sensed { id: 1, pos: P2::new(5_000, 0) }];
        perc.threats = vec![Sensed { id: 9, pos: P2::new(300, 0) }];
        assert_eq!(decide(NpcKind::Deer, &perc, &mut d, &params, DT), Decision::Flee(P2::new(300, 0)));
    }

    #[test]
    fn decision_is_deterministic() {
        let (params, _) = wolf();
        let mut perc = Perception::at(P2::new(0, 0));
        perc.prey = vec![Sensed { id: 3, pos: P2::new(900, 0) }];
        let mut a = Drives { hunger: 0.7, ..Default::default() };
        let mut b = a;
        let da = decide(NpcKind::Wolf, &perc, &mut a, &params, DT);
        let db = decide(NpcKind::Wolf, &perc, &mut b, &params, DT);
        assert_eq!(da, db);
        assert_eq!(a, b);
    }
}
