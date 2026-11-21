use crate::map::ArenaMap;
use crate::protocol::{
    dist2d, dist3d, wrap_angle, Config, Observation, Offer, PickupObs, SelfObs, Vec3, WeaponKind,
};
use crate::world::WorldState;

#[derive(Debug, Clone)]
pub struct CombatTarget {
    pub is_mech: bool,
    pub is_crate: bool,
    pub pos: Vec3,
    pub vel: Vec3,
    pub hp: f64,
    pub radius: f64,
    pub half_height: f64,
    pub has_mortar: bool,
    pub has_laser: bool,
    pub has_lightning: bool,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct CombatDecision {
    pub aim: Option<[f64; 2]>,
    pub fire: [bool; 2],
    pub reload: bool,
    pub charging_laser: bool,
    pub laser_charge_s: f64,
    pub laser_cooldown_s: f64,
    pub target_pos: Option<Vec3>,
    pub desired_range: f64,
    pub has_los_to_target: bool,
}

impl Default for CombatDecision {
    fn default() -> Self {
        Self {
            aim: None,
            fire: [false, false],
            reload: false,
            charging_laser: false,
            laser_charge_s: 0.0,
            laser_cooldown_s: 0.0,
            target_pos: None,
            desired_range: 65.0,
            has_los_to_target: false,
        }
    }
}

/// Exact linear intercept solver: returns predicted hit point and time of flight.
pub fn solve_linear_intercept(
    origin: Vec3,
    proj_speed: f64,
    target_pos: Vec3,
    target_vel: Vec3,
) -> Option<(Vec3, f64)> {
    let rx = target_pos[0] - origin[0];
    let ry = target_pos[1] - origin[1];
    let rz = target_pos[2] - origin[2];
    let vx = target_vel[0];
    let vy = target_vel[1];
    let vz = target_vel[2];

    let a = vx * vx + vy * vy + vz * vz - proj_speed * proj_speed;
    let b = 2.0 * (rx * vx + ry * vy + rz * vz);
    let c = rx * rx + ry * ry + rz * rz;

    let t = if a.abs() < 1e-6 {
        if b.abs() < 1e-6 {
            return None;
        }
        let t0 = -c / b;
        if t0 > 0.0 {
            t0
        } else {
            return None;
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            (c.sqrt() / proj_speed.max(1.0)).min(2.0)
        } else {
            let sq = disc.sqrt();
            let t1 = (-b - sq) / (2.0 * a);
            let t2 = (-b + sq) / (2.0 * a);
            match (t1 > 0.0, t2 > 0.0) {
                (true, true) => t1.min(t2),
                (true, false) => t1,
                (false, true) => t2,
                (false, false) => return None,
            }
        }
    };

    let t = t.clamp(0.0, 2.5);
    Some((
        [
            target_pos[0] + vx * t,
            target_pos[1] + vy * t,
            target_pos[2] + vz * t * 0.45,
        ],
        t,
    ))
}

/// Solve mortar pitch angles (low_arc, high_arc) in radians for horizontal distance `d` and vertical delta `dz`.
pub fn solve_mortar_pitches(d: f64, dz: f64, speed: f64, gravity: f64) -> Option<(f64, f64)> {
    if d < 1.0 || speed <= 1.0 || gravity <= 0.1 {
        return None;
    }
    let v2 = speed * speed;
    let v4 = v2 * v2;
    let disc = v4 - gravity * (gravity * d * d + 2.0 * dz * v2);
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    let low = ((v2 - sq) / (gravity * d)).atan();
    let high = ((v2 + sq) / (gravity * d)).atan();
    Some((low, high))
}

pub fn mortar_arc_clear(
    map: &ArenaMap,
    origin: Vec3,
    yaw: f64,
    pitch: f64,
    speed: f64,
    gravity: f64,
    flight_t: f64,
) -> bool {
    if !map.loaded || flight_t <= 0.05 {
        return true;
    }
    let vx = speed * pitch.cos() * yaw.cos();
    let vy = speed * pitch.cos() * yaw.sin();
    let vz = speed * pitch.sin();
    let samples = 10usize;
    let mut prev = [
        origin[0] + vx * (flight_t * 0.08),
        origin[1] + vy * (flight_t * 0.08),
        origin[2] + vz * (flight_t * 0.08) - 0.5 * gravity * (flight_t * 0.08).powi(2),
    ];
    for i in 2..samples {
        let t = flight_t * (i as f64 / samples as f64) * 0.92;
        let cur = [
            origin[0] + vx * t,
            origin[1] + vy * t,
            origin[2] + vz * t - 0.5 * gravity * t * t,
        ];
        if !map.has_los(prev, cur) {
            return false;
        }
        prev = cur;
    }
    true
}

pub fn muzzle_pos(me: &SelfObs) -> Vec3 {
    let mh = me.stat("muzzle_height", 7.0);
    [me.pos[0], me.pos[1], me.pos[2] + mh]
}

pub fn mortar_origin_pos(me: &SelfObs) -> Vec3 {
    let h = me.stat("height", 9.0);
    [me.pos[0], me.pos[1], me.pos[2] + h + 0.5]
}

pub fn aim_angles(from: Vec3, to: Vec3) -> [f64; 2] {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    let horiz = (dx * dx + dy * dy).sqrt().max(1e-6);
    [dy.atan2(dx), dz.atan2(horiz)]
}

/// Predict actual torso aim after 1 sim tick (0.05 s) of slew toward `desired`.
#[inline]
fn slew_one_tick(cur: [f64; 2], desired: [f64; 2], turn_rate: f64, pitch_rate: f64) -> [f64; 2] {
    let max_dy = turn_rate * 0.05;
    let max_dp = pitch_rate * 0.05;
    let dy = wrap_angle(desired[0] - cur[0]).clamp(-max_dy, max_dy);
    let dp = (desired[1] - cur[1]).clamp(-max_dp, max_dp);
    [wrap_angle(cur[0] + dy), cur[1] + dp]
}

/// A trigger remains held for two simulation ticks. A weapon still cooling down
/// on the first tick fires along the second tick's torso direction instead.
fn shot_aim(
    me: &SelfObs,
    command: [f64; 2],
    cooldown: f64,
) -> [f64; 2] {
    let turn = me.stat("turn_rate", 4.0);
    let pitch = me.stat("pitch_rate", 3.0);
    let first = slew_one_tick(me.aim, command, turn, pitch);
    if cooldown > 0.05 {
        slew_one_tick(first, command, turn, pitch)
    } else {
        first
    }
}

#[derive(Clone, Copy)]
struct ShotAlignment {
    desired: [f64; 2],
    yaw_tolerance: f64,
    pitch_tolerance: f64,
}

impl ShotAlignment {
    fn accepts(self, actual: [f64; 2]) -> bool {
        wrap_angle(self.desired[0] - actual[0]).abs() <= self.yaw_tolerance
            && (self.desired[1] - actual[1]).abs() <= self.pitch_tolerance
    }
}

/// Borrow gun cooldown only for a mortar shot that fits the current action and
/// leaves enough time to return to gun lead before its next ready tick.
fn mortar_fits_gun_cooldown_window(
    me: &SelfObs,
    gun: ShotAlignment,
    mortar: ShotAlignment,
    gun_cooldown: f64,
    mortar_cooldown: f64,
) -> bool {
    if !mortar.accepts(shot_aim(me, mortar.desired, mortar_cooldown)) {
        return false;
    }
    // The command remains held after a first-tick mortar shot, so budget the
    // return from the torso position after BOTH ticks, not from the firing aim.
    let end_aim = shot_aim(me, mortar.desired, 0.10);
    let return_ticks = ((gun_cooldown - 0.10) / 0.05).ceil().max(1.0);
    let return_s = return_ticks * 0.05;
    wrap_angle(gun.desired[0] - end_aim[0]).abs()
        <= me.stat("turn_rate", 4.0) * return_s + gun.yaw_tolerance
        && (gun.desired[1] - end_aim[1]).abs()
            <= me.stat("pitch_rate", 3.0) * return_s + gun.pitch_tolerance
}

/// With no selected target, do not accidentally discharge toward a patrol yaw.
/// Release at the current direction, preserving the previous discharge timing
/// without retaining half-speed movement or steering a release toward patrol.
fn release_untargeted_laser(out: &mut CombatDecision, me: &SelfObs) {
    for (slot, w) in me.weapons.iter().enumerate() {
        let Some(w) = w else { continue };
        let charge = w.charge.unwrap_or(0.0);
        if w.kind == WeaponKind::Laser && charge > 0.0 {
            out.aim = Some(me.aim);
            out.fire[slot] = false;
            out.charging_laser = false;
        }
    }
}

pub fn select_target(
    obs: &Observation,
    cfg: &Config,
    map: &ArenaMap,
    world: &WorldState,
    allow_farm: bool,
    hold_fire_for_regen: bool,
) -> Option<CombatTarget> {
    let me = &obs.self_obs;
    let muzzle = muzzle_pos(me);
    let size_per_cube = cfg.cubes.size_per_cube;
    let has_laser = me.has_weapon(WeaponKind::Laser);
    let has_lightning = me.has_weapon(WeaponKind::Lightning);
    let has_mortar = me.has_weapon(WeaponKind::Mortar);

    let mut best_enemy: Option<CombatTarget> = None;

    for e in world.enemies.values() {
        let age = obs.t - e.last_seen_t;
        if !e.visible_now && (!has_mortar || age > 3.5) {
            continue;
        }
        let epos = if e.visible_now {
            e.pos
        } else {
            e.estimated_pos(obs.t)
        };
        let scale = e.size_scale(size_per_cube);
        let center = [epos[0], epos[1], epos[2] + 4.5 * scale];
        let d = dist3d(muzzle, center);
        if d > 275.0 {
            continue;
        }

        let los = map.has_los(muzzle, center);
        if !los && !has_mortar {
            // Keep visible enemies (including 5s memory behind cover!) trackable for cover-peeking up to 190m
            if !e.visible_now || d > 190.0 {
                continue;
            }
        }

        if hold_fire_for_regen && d > 55.0 && !e.aiming_at_me && obs.t - e.last_damaged_me_t > 2.5 {
            continue;
        }

        let mut score = 220.0 - d * 0.65;
        if los {
            score += 95.0;
            if d < 55.0 {
                score += 45.0; // Prioritize close flankers so front armor & guns face them immediately!
            }
            if obs.t - e.last_damaged_me_t < 2.0 {
                score += 35.0;
            }
        }
        let hp_frac = (e.hp / e.max_hp.max(1.0)).clamp(0.0, 1.0);
        score += (1.0 - hp_frac) * 120.0;
        if e.hp < 340.0 {
            score += 75.0;
        }
        if e.aiming_at_me {
            score += 45.0;
        }
        if obs.t - e.last_damaged_me_t < 3.5 {
            score += 55.0;
        }
        if e.charging {
            score += 60.0;
        }
        if has_laser && e.has_weapon(WeaponKind::Mortar) {
            score += 65.0; // +60% cookoff!
        }
        if has_lightning && e.has_weapon(WeaponKind::Laser) && d < 65.0 {
            score += 65.0; // short their laser!
        }
        let from_enemy_yaw = (me.pos[1] - epos[1]).atan2(me.pos[0] - epos[0]);
        if wrap_angle(from_enemy_yaw - e.aim[0]).abs() > std::f64::consts::FRAC_PI_2 {
            score += 25.0;
        }
        if !e.visible_now {
            score -= 80.0 + age * 20.0;
        }

        let cand = CombatTarget {
            is_mech: true,
            is_crate: false,
            pos: center,
            vel: e.vel,
            hp: e.hp,
            radius: 2.5 * scale,
            half_height: 4.5 * scale,
            has_mortar: e.has_weapon(WeaponKind::Mortar),
            has_laser: e.has_weapon(WeaponKind::Laser),
            has_lightning: e.has_weapon(WeaponKind::Lightning),
            score,
        };
        if best_enemy.as_ref().is_none_or(|b| cand.score > b.score) {
            best_enemy = Some(cand);
        }
    }

    if best_enemy.is_some() {
        return best_enemy;
    }

    // Even while healing, defend ourselves against an NPC drone actively shooting us at close range!
    let mut best_farm: Option<CombatTarget> = None;
    for npc in &world.npcs {
        let center = [npc.pos[0], npc.pos[1], npc.pos[2] + 1.75];
        let d = dist3d(muzzle, center);
        let max_d = if allow_farm && !hold_fire_for_regen {
            115.0
        } else {
            42.0
        };
        if d > max_d || !map.has_los(muzzle, center) {
            continue;
        }
        let hp = npc.hp.unwrap_or(350.0);
        let score = 155.0 - d + (350.0 - hp) * 0.25;
        let cand = CombatTarget {
            is_mech: false,
            is_crate: false,
            pos: center,
            vel: npc.vel,
            hp,
            radius: 1.8,
            half_height: 1.75,
            has_mortar: false,
            has_laser: false,
            has_lightning: false,
            score,
        };
        if best_farm.as_ref().is_none_or(|b| cand.score > b.score) {
            best_farm = Some(cand);
        }
    }

    if hold_fire_for_regen || !allow_farm {
        return best_farm;
    }

    for cr in &obs.crates {
        let center = [cr.pos[0], cr.pos[1], cr.pos[2] + 1.2];
        let d = dist3d(muzzle, center);
        if d > 95.0 || !map.has_los(muzzle, center) {
            continue;
        }
        let hp = cr.hp.unwrap_or(300.0);
        let score = 130.0 - d + (300.0 - hp) * 0.25 + if me.empty_slot().is_some() { 35.0 } else { 0.0 };
        let cand = CombatTarget {
            is_mech: false,
            is_crate: true,
            pos: center,
            vel: [0.0, 0.0, 0.0],
            hp,
            radius: 1.4,
            half_height: 1.2,
            has_mortar: false,
            has_laser: false,
            has_lightning: false,
            score,
        };
        if best_farm.as_ref().is_none_or(|b| cand.score > b.score) {
            best_farm = Some(cand);
        }
    }

    best_farm
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_combat(
    obs: &Observation,
    cfg: &Config,
    map: &ArenaMap,
    world: &WorldState,
    target: Option<&CombatTarget>,
    urgent_move: bool,
    hold_fire_for_regen: bool,
    allow_precharge_behind_cover: bool,
    avoid_open_laser_slowdown: bool,
) -> CombatDecision {
    let me = &obs.self_obs;
    let mut out = CombatDecision::default();
    out.desired_range = preferred_range(me, target);

    for w in me.weapons.iter().flatten() {
        if w.kind == WeaponKind::Laser {
            out.laser_charge_s = w.charge.unwrap_or(0.0);
            out.laser_cooldown_s = w.cooldown;
        }
    }

    let gun_needs_passive_reload = me.weapons.iter().any(|w| {
        if let Some(w) = w {
            if w.kind == WeaponKind::Gun {
                let mag = w.stat("mag", 12.0);
                let ammo = w.ammo.unwrap_or(mag);
                let reloading = w.reload.unwrap_or(0.0) > 0.0;
                return !reloading && ammo < (mag * 0.45).max(4.0);
            }
        }
        false
    });

    let Some(t) = target else {
        out.reload = gun_needs_passive_reload;
        release_untargeted_laser(&mut out, me);
        return out;
    };

    let muzzle = muzzle_pos(me);
    let dist = dist3d(muzzle, t.pos);
    let horiz_dist = dist2d([muzzle[0], muzzle[1]], [t.pos[0], t.pos[1]]).max(0.5);
    let has_los = map.has_los(muzzle, t.pos);
    out.has_los_to_target = has_los;
    out.target_pos = Some(t.pos);

    let min_pitch = me.stat("min_pitch", -0.7);
    let max_pitch = me.stat("max_pitch", 1.3);
    let turn_rate = me.stat("turn_rate", 4.0);
    let pitch_rate = me.stat("pitch_rate", 3.0);
    let cube_mult = 1.0 + cfg.cubes.stat_cap.min(me.cubes as f64) * cfg.cubes.damage_per_cube;

    let mut best_aim: Option<(i32, [f64; 2])> = None;
    let dual_loadout = me.weapons.iter().flatten().count() > 1;
    let mut shot_alignment: [Option<ShotAlignment>; 2] = [None, None];
    let mut set_aim = |prio: i32, a: [f64; 2]| {
        if best_aim.is_none_or(|(p, _)| prio > p) {
            best_aim = Some((prio, a));
        }
    };

    let direct_aim = aim_angles(muzzle, t.pos);
    set_aim(1, direct_aim);

    // Only suppress shooting CRATES while healing — ALWAYS kill NPC drones shooting us at close range!
    if hold_fire_for_regen && t.is_crate {
        out.aim = best_aim.map(|(_, a)| a);
        out.reload = gun_needs_passive_reload;
        release_untargeted_laser(&mut out, me);
        return out;
    }

    let gun_reloading_now = me.weapons.iter().flatten().any(|w| {
        w.kind == WeaponKind::Gun && w.reload.unwrap_or(0.0) > 0.1
    });

    for (slot, wopt) in me.weapons.iter().enumerate() {
        let Some(w) = wopt else { continue };
        match w.kind {
            WeaponKind::Laser => {
                let range = w.stat("range", 280.0);
                let full_charge = w.stat("charge", 0.75);
                let min_charge = w.stat("min_charge", 0.15);
                let min_frac = w.stat("min_frac", 0.35);
                let hold_s = w.stat("hold_s", 2.0);
                let max_dmg = w.stat("damage", 315.0);
                let cur_charge = w.charge.unwrap_or(0.0);

                // Lead by 0.05 s accounting for BOTH target velocity AND our own movement!
                let muzzle_next = [
                    muzzle[0] + me.vel[0] * 0.05,
                    muzzle[1] + me.vel[1] * 0.05,
                    muzzle[2] + me.vel[2] * 0.05,
                ];
                let lead_pos = [
                    t.pos[0] + t.vel[0] * 0.05,
                    t.pos[1] + t.vel[1] * 0.05,
                    t.pos[2],
                ];
                let desired = aim_angles(muzzle_next, lead_pos);

                if cur_charge > 0.0 {
                    // Require actual me.aim to be close (<= 0.055 rad slew margin) AND post-slew dead-center
                    // so hitscan laser lands 95%+ regardless of pre-slew vs post-slew server tick order!
                    let next_aim = slew_one_tick(me.aim, desired, turn_rate, pitch_rate);
                    let cur_yaw_err = wrap_angle(desired[0] - me.aim[0]).abs();
                    let cur_pitch_err = (desired[1] - me.aim[1]).abs();
                    let yaw_err = wrap_angle(desired[0] - next_aim[0]).abs();
                    let pitch_err = (desired[1] - next_aim[1]).abs();
                    let yaw_tol = ((t.radius * 0.80) / horiz_dist).atan().max(0.004);
                    let pitch_tol = ((t.half_height * 0.78) / dist.max(1.0)).atan().max(0.004);
                    let aligned = yaw_err <= yaw_tol
                        && pitch_err <= pitch_tol
                        && cur_yaw_err <= (yaw_tol + 0.045)
                        && cur_pitch_err <= (pitch_tol + 0.04);
                    let los_next = has_los && map.has_los(muzzle_next, lead_pos);

                    let frac = (cur_charge / full_charge.max(0.05)).clamp(0.0, 1.0);
                    let cookoff = if t.has_mortar { 1.6 } else { 1.0 };
                    let est_dmg =
                        max_dmg * (min_frac + (1.0 - min_frac) * frac) * cube_mult * cookoff;

                    // Charging does not shoot: let a ready gun track its lead until
                    // the laser needs to prepare a release. Reserve aim before the
                    // full-charge window and before unavoidable auto-discharge.
                    let prepare_release = urgent_move
                        || cur_charge + 0.15 >= full_charge
                        || (cur_charge >= min_charge && est_dmg >= t.hp * 1.05)
                        || cur_charge + 0.10 >= full_charge + hold_s;
                    set_aim(if prepare_release { 100 } else { 40 }, desired);

                    let should_release = los_next
                        && dist <= range - 1.0
                        && aligned
                        && (cur_charge >= full_charge - 0.05
                            || (cur_charge >= min_charge && est_dmg >= t.hp * 1.05)
                            || cur_charge >= full_charge + hold_s - 0.25);

                    if should_release {
                        out.fire[slot] = false;
                        out.charging_laser = false;
                    } else if urgent_move {
                        // False requests release, even below min_charge (the server
                        // then fires at the minimum). Keep this laser's aim selected.
                        out.fire[slot] = false;
                        out.charging_laser = false;
                    } else {
                        out.fire[slot] = true;
                        out.charging_laser = true;
                    }
                } else if w.cooldown <= 0.05
                    && obs.t >= world.shorted_until_t
                    && !urgent_move
                    && dist <= range - 2.0
                {
                    let ok_target = t.is_mech || (!t.is_crate) || t.hp >= 220.0;
                    let can_start = if has_los {
                        !avoid_open_laser_slowdown || t.hp <= max_dmg * 1.1
                    } else {
                        allow_precharge_behind_cover && t.is_mech
                    };
                    if ok_target && can_start {
                        set_aim(40, desired);
                        out.fire[slot] = true;
                        out.charging_laser = true;
                    }
                }
            }

            WeaponKind::Lightning => {
                let range = w.stat("range", 45.0);
                let cone_rad = w.stat("cone_deg", 30.0).to_radians();
                let e_cost = w.stat("energy", 3.0);
                if dist <= range - 0.4 && has_los && w.cooldown <= 0.10 {
                    let yaw_err = wrap_angle(direct_aim[0] - me.aim[0]).abs();
                    let min_energy = if t.is_mech && (t.hp < 240.0 || t.has_laser) {
                        e_cost
                    } else if t.is_mech {
                        e_cost + 8.0
                    } else {
                        e_cost + 24.0
                    };
                    if me.energy >= min_energy {
                        let l_prio = if yaw_err <= cone_rad * 0.6 { 55 } else { 90 };
                        set_aim(l_prio, direct_aim);
                        shot_alignment[slot] = Some(ShotAlignment {
                            desired: direct_aim,
                            yaw_tolerance: cone_rad * 0.92,
                            pitch_tolerance: f64::INFINITY,
                        });
                        if yaw_err <= cone_rad * 0.92 {
                            out.fire[slot] = true;
                        }
                    }
                }
            }

            WeaponKind::Gun => {
                let speed = w.stat("speed", 252.0);
                let range = w.stat("range", 230.0);
                let ammo = w.ammo.unwrap_or(12.0);
                let reloading = w.reload.unwrap_or(0.0) > 0.0;

                if !reloading && ammo >= 1.0 && dist <= range - 2.0 && has_los {
                    if let Some((raw_pred, _tf)) = solve_linear_intercept(muzzle, speed, t.pos, t.vel)
                    {
                        let pred = if map.has_los(muzzle, raw_pred) {
                            raw_pred
                        } else {
                            // If lead point is behind a corner edge, clamp halfway back toward visible body
                            [
                                0.5 * (t.pos[0] + raw_pred[0]),
                                0.5 * (t.pos[1] + raw_pred[1]),
                                0.5 * (t.pos[2] + raw_pred[2]),
                            ]
                        };
                        let desired = aim_angles(muzzle, pred);
                        // A gun that cannot shoot during this action should not
                        // prevent a ready mortar/arc from using the shared torso.
                        set_aim(if dual_loadout && w.cooldown > 0.10 { 25 } else { 65 }, desired);
                        let next_aim = slew_one_tick(me.aim, desired, turn_rate, pitch_rate);
                        let yaw_err = wrap_angle(desired[0] - next_aim[0]).abs();
                        let pitch_err = (desired[1] - next_aim[1]).abs();
                        let yaw_tol = ((t.radius * 1.45) / horiz_dist).atan().max(0.045);
                        let pitch_tol = ((t.half_height * 1.45) / dist.max(1.0)).atan().max(0.045);
                        shot_alignment[slot] = Some(ShotAlignment {
                            desired,
                            yaw_tolerance: yaw_tol,
                            pitch_tolerance: pitch_tol,
                        });
                        if yaw_err <= yaw_tol && pitch_err <= pitch_tol {
                            out.fire[slot] = true;
                        }
                    }
                }
                if !out.fire[slot] && !has_los && gun_needs_passive_reload {
                    out.reload = true;
                }
            }

            WeaponKind::Mortar => {
                let speed = w.stat("speed", 72.0);
                let gravity = w.stat("gravity", 22.0);
                let splash_r = w.stat("splash_radius", 7.0);
                if w.cooldown <= 0.10
                    && horiz_dist >= (splash_r * 2.2).max(16.0)
                    && horiz_dist <= 230.0
                {
                    let origin = mortar_origin_pos(me);
                    let mut aim_pt = [t.pos[0], t.pos[1], t.pos[2] - t.half_height + 0.5];
                    let mut chosen: Option<(f64, f64)> = None;

                    for _ in 0..2 {
                        let d_h = dist2d([origin[0], origin[1]], [aim_pt[0], aim_pt[1]]);
                        let dz = aim_pt[2] - origin[2];
                        if let Some((low_p, high_p)) = solve_mortar_pitches(d_h, dz, speed, gravity)
                        {
                            let yaw = (aim_pt[1] - origin[1]).atan2(aim_pt[0] - origin[0]);
                            let mut picked = None;
                            for p in [low_p, high_p] {
                                if p >= min_pitch + 0.02 && p <= max_pitch - 0.02 {
                                    let tf = d_h / (speed * p.cos()).max(1.0);
                                    if tf <= 6.8
                                        && mortar_arc_clear(map, origin, yaw, p, speed, gravity, tf)
                                    {
                                        picked = Some((yaw, p, tf));
                                        break;
                                    }
                                }
                            }
                            if let Some((yaw, p, tf)) = picked {
                                chosen = Some((yaw, p));
                                aim_pt = [
                                    t.pos[0] + t.vel[0] * tf * 0.75,
                                    t.pos[1] + t.vel[1] * tf * 0.75,
                                    t.pos[2] - t.half_height + 0.5,
                                ];
                            } else {
                                chosen = None;
                                break;
                            }
                        } else {
                            chosen = None;
                            break;
                        }
                    }

                    if let Some((yaw, pitch)) = chosen {
                        // Lob mortar when enemy is behind cover (!has_los) OR while our Gun is reloading (zero DPS lost!)
                        let prio = if !has_los {
                            85
                        } else if gun_reloading_now && horiz_dist <= 135.0 {
                            75
                        } else {
                            45
                        };
                        set_aim(prio, [yaw, pitch]);
                        shot_alignment[slot] = Some(ShotAlignment {
                            desired: [yaw, pitch],
                            yaw_tolerance: 0.09,
                            pitch_tolerance: 0.07,
                        });
                        let next_aim = slew_one_tick(me.aim, [yaw, pitch], turn_rate, pitch_rate);
                        let yaw_err = wrap_angle(yaw - next_aim[0]).abs();
                        let pitch_err = (pitch - next_aim[1]).abs();
                        if yaw_err <= 0.09 && pitch_err <= 0.07 {
                            out.fire[slot] = true;
                        }
                    }
                }
            }
        }
    }

    if let (Some(gun_slot), Some(mortar_slot)) =
        (me.weapon_slot(WeaponKind::Gun), me.weapon_slot(WeaponKind::Mortar))
    {
        if let (Some(gun), Some(mortar), Some(gun_plan), Some(mortar_plan)) = (
            &me.weapons[gun_slot], &me.weapons[mortar_slot],
            shot_alignment[gun_slot], shot_alignment[mortar_slot],
        ) {
            // A valid gun plan implies ammo, no reload, and line of sight. Keep
            // reload/occluded mortar preparation and all other loadouts intact.
            if gun.cooldown > 0.10 && !mortar_fits_gun_cooldown_window(
                me, gun_plan, mortar_plan, gun.cooldown, mortar.cooldown,
            ) {
                best_aim = Some((65, gun_plan.desired));
            }
        }
    }

    out.aim = best_aim.map(|(_, a)| a);
    if dual_loadout {
        if let Some(command) = out.aim {
            for (slot, plan) in shot_alignment.iter().enumerate() {
                let (Some(plan), Some(w)) = (plan, &me.weapons[slot]) else { continue };
                // Preserve the existing tolerances; only fix which actual aim
                // they are evaluated against. Never mask a laser trigger here:
                // false releases a charge, unlike the other weapons.
                let aligned = plan.accepts(shot_aim(me, command, w.cooldown));
                out.fire[slot] = if w.kind == WeaponKind::Lightning {
                    out.fire[slot] && aligned
                } else {
                    aligned
                };
            }
        }
    }
    out
}

fn preferred_range(me: &SelfObs, target: Option<&CombatTarget>) -> f64 {
    let has_lightning = me.has_weapon(WeaponKind::Lightning);
    let has_laser = me.has_weapon(WeaponKind::Laser);
    let has_gun = me.has_weapon(WeaponKind::Gun);
    let lightning_range = me
        .weapons
        .iter()
        .flatten()
        .find(|w| w.kind == WeaponKind::Lightning)
        .map(|w| w.stat("range", 45.0))
        .unwrap_or(45.0);

    if let Some(t) = target {
        // NEVER walk into enemy Lightning range (45m) unless we have Lightning + clear advantage!
        if t.has_lightning && (!has_lightning || me.hp < t.hp + 150.0) {
            return if has_laser { 105.0 } else { 78.0 };
        }
        // With Lightning vs non-Lightning (especially Laser!), stay inside 45m arc range to deal 240 DPS + permashort!
        if has_lightning && !t.has_lightning && me.energy > 12.0 && me.hp / me.max_hp.max(1.0) > 0.28 {
            return (lightning_range * 0.80).max(28.0);
        }
        if has_laser && !t.has_laser {
            return 108.0;
        }
        if has_laser && t.has_mortar {
            return 108.0;
        }
    }

    if has_lightning && !has_laser && !has_gun {
        return lightning_range * 0.78;
    }
    if has_laser {
        return 92.0;
    }
    if has_lightning && has_gun && me.energy > 14.0 && me.hp / me.max_hp.max(1.0) > 0.30 {
        return (lightning_range * 0.80).max(30.0);
    }
    74.0
}

/// Evaluate holistic 2-weapon loadout synergy (Gun+Laser is S-tier; always dump Mortar!).
pub fn loadout_score(a: Option<WeaponKind>, b: Option<WeaponKind>, me: &SelfObs) -> f64 {
    let count_upg = |prefix: &str| -> f64 {
        me.upgrades.iter().filter(|u| u.starts_with(prefix)).count() as f64
    };
    let bonus = |k: WeaponKind| -> f64 {
        match k {
            WeaponKind::Laser => count_upg("laser_") * 24.0,
            WeaponKind::Lightning => {
                count_upg("lightning_") * 22.0
                    + if me.upgrades.iter().any(|u| u == "reactor" || u == "capacitor") {
                        8.0
                    } else {
                        0.0
                    }
            }
            WeaponKind::Gun => count_upg("gun_") * 18.0,
            WeaponKind::Mortar => count_upg("mortar_") * 12.0,
        }
    };

    let (Some(w0), Some(w1)) = (a, b) else {
        let single = a.or(b);
        return match single {
            Some(WeaponKind::Laser) => 125.0 + bonus(WeaponKind::Laser),
            Some(WeaponKind::Lightning) => 104.0 + bonus(WeaponKind::Lightning),
            Some(WeaponKind::Gun) => 90.0 + bonus(WeaponKind::Gun),
            Some(WeaponKind::Mortar) => 70.0 + bonus(WeaponKind::Mortar),
            None => 0.0,
        };
    };

    let pair_base = match (w0, w1) {
        (WeaponKind::Gun, WeaponKind::Laser) | (WeaponKind::Laser, WeaponKind::Gun) => 232.0,
        (WeaponKind::Laser, WeaponKind::Lightning) | (WeaponKind::Lightning, WeaponKind::Laser) => {
            212.0
        }
        (WeaponKind::Gun, WeaponKind::Lightning) | (WeaponKind::Lightning, WeaponKind::Gun) => 204.0,
        (WeaponKind::Laser, WeaponKind::Mortar) | (WeaponKind::Mortar, WeaponKind::Laser) => 152.0,
        (WeaponKind::Lightning, WeaponKind::Mortar) | (WeaponKind::Mortar, WeaponKind::Lightning) => {
            148.0
        }
        (WeaponKind::Gun, WeaponKind::Mortar) | (WeaponKind::Mortar, WeaponKind::Gun) => 145.0,
        _ => 100.0,
    };
    pair_base + bonus(w0) + bonus(w1)
}

/// Decide whether a visible pickup improves our loadout; returns `Some(drop_slot_opt)`.
pub fn is_pickup_desirable(kind: WeaponKind, me: &SelfObs) -> Option<Option<usize>> {
    if me.has_weapon(kind) {
        return None;
    }
    if me.empty_slot().is_some() {
        if kind == WeaponKind::Gun {
            return None;
        }
        return Some(None);
    }
    let w0 = me.weapons[0].as_ref().map(|w| w.kind);
    let w1 = me.weapons[1].as_ref().map(|w| w.kind);
    let cur_score = loadout_score(w0, w1, me);
    let mut best_swap: Option<(f64, usize)> = None;

    for (slot, wopt) in me.weapons.iter().enumerate() {
        let Some(w) = wopt else { continue };
        if w.kind == WeaponKind::Laser && w.charge.unwrap_or(0.0) > 0.0 {
            continue;
        }
        let new_score = if slot == 0 {
            loadout_score(Some(kind), w1, me)
        } else {
            loadout_score(w0, Some(kind), me)
        };
        let gain = new_score - cur_score;
        if gain >= 10.0 && best_swap.is_none_or(|(bg, _)| gain > bg) {
            best_swap = Some((gain, slot));
        }
    }
    best_swap.map(|(_, slot)| Some(slot))
}

pub fn choose_immediate_pickup(obs: &Observation, cfg: &Config) -> (Option<u64>, Option<usize>) {
    let me = &obs.self_obs;
    let reach = cfg.loot.pickup_radius;
    let mut best: Option<(f64, u64, Option<usize>)> = None;

    for p in &obs.pickups {
        let dh = dist2d([me.pos[0], me.pos[1]], [p.pos[0], p.pos[1]]);
        let dz = (me.pos[2] - p.pos[2]).abs();
        if dh >= reach - 0.1 || dz >= 3.8 {
            continue;
        }
        if let Some(drop_slot) = is_pickup_desirable(p.kind, me) {
            let val = match p.kind {
                WeaponKind::Laser => 110.0,
                WeaponKind::Lightning => 104.0,
                WeaponKind::Gun => 85.0,
                WeaponKind::Mortar => 60.0,
            } - dh * 0.5;
            if best.is_none_or(|(bv, _, _)| val > bv) {
                best = Some((val, p.id, drop_slot));
            }
        }
    }

    match best {
        Some((_, id, drop)) => (Some(id), drop),
        None => (None, None),
    }
}

pub fn choose_upgrade(offer: &Offer, obs: &Observation) -> usize {
    let me = &obs.self_obs;
    let hp_frac = me.hp / me.max_hp.max(1.0);
    let late = obs.zone.radius < 150.0;
    let has_lightning = me.has_weapon(WeaponKind::Lightning);
    let has_laser = me.has_weapon(WeaponKind::Laser);

    let mut best_idx = 0usize;
    let mut best_score = f64::NEG_INFINITY;

    for (i, ch) in offer.choices.iter().enumerate() {
        let mut s = match ch.id.as_str() {
            "nanites" => 122.0 + if hp_frac < 0.80 { 22.0 } else { 0.0 },
            "armor" => 108.0 + if late { 20.0 } else { 0.0 },
            "plating" => {
                100.0 + if late { 24.0 } else { 0.0 } + if hp_frac < 0.65 { 14.0 } else { 0.0 }
            }
            "laser_charge" => 112.0,
            "lightning_range" => 110.0,
            "laser_damage" => 102.0,
            "lightning_efficiency" => 100.0,
            "servos" => 96.0,
            "gun_rate" => 94.0,
            "gun_damage" => 92.0,
            "lightning_chain" => 92.0,
            "gun_velocity" => 82.0,
            "gyros" => 82.0 + if has_laser { 10.0 } else { 0.0 },
            "reactor" => 80.0 + if has_lightning { 16.0 } else { 0.0 },
            "gun_mag" => 78.0,
            "jets" => 78.0,
            "capacitor" => 74.0 + if has_lightning { 14.0 } else { 0.0 },
            "sensors" => 72.0,
            "mortar_reload" => 65.0,
            "mortar_splash" => 62.0,
            "mortar_velocity" => 58.0,
            "laser_pierce" => 56.0,
            _ => 50.0,
        };
        if ch.id == "nanites" && me.upgrades.iter().any(|u| u == "nanites") {
            // 2nd nanites stack (2.0s delay, 115 HP/s!) is STILL S-tier!
            s -= 3.0;
        }
        if s > best_score {
            best_score = s;
            best_idx = i;
        }
    }
    best_idx
}

pub fn best_nearby_pickup(
    obs: &Observation,
    world: &WorldState,
    max_dist: f64,
    zone_center: [f64; 2],
    zone_radius: f64,
) -> Option<(Vec3, f64)> {
    let me = &obs.self_obs;
    let my2 = [me.pos[0], me.pos[1]];
    let mut best: Option<(f64, Vec3)> = None;
    for p in world.pickups.values() {
        if is_pickup_desirable(p.kind, me).is_none() {
            continue;
        }
        let p2 = [p.pos[0], p.pos[1]];
        let d = dist2d(my2, p2);
        if d > max_dist {
            continue;
        }
        if dist2d(p2, zone_center) > (zone_radius - 5.0).max(5.0) {
            continue;
        }
        let dz = p.pos[2] - me.pos[2];
        if dz > 22.0 {
            continue;
        }

        // Only avoid pickups literally being camped at point-blank by an enemy (never starve our loadout!)
        let mut danger_pen = 0.0;
        for e in world.enemies.values() {
            if !e.visible_now {
                continue;
            }
            let ed_p = dist2d([e.pos[0], e.pos[1]], p2);
            if ed_p < 15.0 && ed_p < d && d > 14.0 {
                danger_pen += 55.0;
            }
        }

        let base_w = match p.kind {
            WeaponKind::Laser => 165.0,
            WeaponKind::Lightning => 124.0,
            WeaponKind::Gun => 80.0,
            WeaponKind::Mortar => 80.0,
        };
        let score = base_w - d * 1.05 - dz.max(0.0) * 1.4 - danger_pen;
        if best.is_none_or(|(bs, _)| score > bs) {
            best = Some((score, p.pos));
        }
    }
    best.map(|(s, pos)| (pos, s))
}

#[allow(dead_code)]
pub fn check_visible_pickup(_p: &PickupObs) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{RawObservation, WeaponObs};
    use serde_json::json;
    use std::collections::HashMap;

    fn config() -> Config {
        serde_json::from_value(json!({
            "vision":{"range":200,"fov_deg":120,"near_range":45},
            "sound":{"bearing_noise_deg":7,"near":60,"mid":160,
                "step_speed":5,"step_every":0.6},
            "weapons":{},"loot":{"pickup_radius":4,"equip_s":1},
            "cubes":{},"zone":{"start_center":[0,0],"start_radius":560}
        })).unwrap()
    }

    fn weapon(kind: WeaponKind) -> WeaponObs {
        WeaponObs {
            kind,cooldown:0.0,ammo:Some(12.0),reload:Some(0.0),
            charge:Some(0.0),stats:HashMap::new(),
        }
    }

    fn observation(weapons: [Option<WeaponObs>; 2]) -> Observation {
        let raw: RawObservation = serde_json::from_value(json!({
            "tick":100,"t":5,"alive":16,
            "self":{"id":0,"pos":[0,0,0],"vel":[0,0,0],"aim":[0,0],
                "hp":1400,"max_hp":1400,"energy":100,"max_energy":100,
                "grounded":true,"cubes":0,"weapons":[null,null],"in_zone":true},
            "zone":{"center":[0,0],"radius":500,"next_center":[0,0],
                "next_radius":380,"shrink_start":200,"shrink_end":240,"dps":20}
        })).unwrap();
        let mut obs: Observation = raw.into();
        obs.self_obs.weapons = weapons;
        obs
    }

    fn target(vy: f64) -> CombatTarget {
        CombatTarget {
            is_mech:true,is_crate:false,pos:[100.0,0.0,4.5],vel:[0.0,vy,0.0],
            hp:1400.0,radius:2.5,half_height:4.5,has_mortar:false,
            has_laser:false,has_lightning:false,score:1.0,
        }
    }

    fn decide(obs: &Observation, target: Option<&CombatTarget>, urgent: bool) -> CombatDecision {
        evaluate_combat(obs,&config(),&ArenaMap::empty(),&WorldState::new(16),
            target,urgent,false,false,false)
    }

    #[test]
    fn gun_tracks_its_lead_during_charge_in_either_slot() {
        let mut laser = weapon(WeaponKind::Laser);
        laser.charge = Some(0.3);
        let mut obs = observation([Some(weapon(WeaponKind::Gun)),Some(laser)]);
        let t = target(19.5);
        let (hit,_) = solve_linear_intercept(muzzle_pos(&obs.self_obs),252.0,t.pos,t.vel).unwrap();
        let expected = aim_angles(muzzle_pos(&obs.self_obs),hit);
        let d = decide(&obs,Some(&t),false);
        assert_eq!(d.fire,[true,true]);
        assert_eq!(d.aim,Some(expected));
        assert!(d.charging_laser);
        obs.self_obs.weapons.swap(0,1);
        let swapped = decide(&obs,Some(&t),false);
        assert_eq!(d.aim,swapped.aim);
        assert_eq!(d.fire,[swapped.fire[1],swapped.fire[0]]);
    }

    #[test]
    fn release_owns_aim_and_only_compatible_gun_shots_fire() {
        let mut laser = weapon(WeaponKind::Laser);
        laser.charge = Some(0.75);
        let mut obs = observation([Some(weapon(WeaponKind::Gun)),Some(laser)]);
        let t = target(19.5);
        obs.self_obs.aim = aim_angles(muzzle_pos(&obs.self_obs),[100.0,0.975,4.5]);
        let d = decide(&obs,Some(&t),false);
        assert_eq!(d.fire,[false,false]);
        assert!(!d.charging_laser);
        assert!((d.aim.unwrap()[0]-obs.self_obs.aim[0]).abs()<1e-12);
        assert!((d.aim.unwrap()[1]-obs.self_obs.aim[1]).abs()<1e-12);

        // Stationary targets permit both a laser release and a gun shot.
        let stationary = target(0.0);
        obs.self_obs.aim = aim_angles(muzzle_pos(&obs.self_obs),stationary.pos);
        let compatible = decide(&obs,Some(&stationary),false);
        assert_eq!(compatible.fire,[true,false]);
    }

    #[test]
    fn misaligned_laser_holds_while_turning_for_release() {
        let mut laser = weapon(WeaponKind::Laser);
        laser.charge = Some(0.75);
        let mut obs = observation([Some(weapon(WeaponKind::Gun)),Some(laser)]);
        obs.self_obs.aim = [1.0,0.0];
        let d = decide(&obs,Some(&target(0.0)),false);
        assert!(d.fire[1]);
        assert!(d.charging_laser);
        assert!(!d.fire[0]);
        assert_eq!(d.aim.unwrap()[0],0.0);
    }

    #[test]
    fn conflicting_mortar_does_not_fire_along_gun_pitch() {
        let mut obs = observation([
            Some(weapon(WeaponKind::Gun)),Some(weapon(WeaponKind::Mortar)),
        ]);
        let d = decide(&obs,Some(&target(0.0)),false);
        assert_eq!(d.fire,[true,false]);
        assert!(d.aim.unwrap()[1]<0.0);
        obs.self_obs.weapons.swap(0,1);
        let swapped = decide(&obs,Some(&target(0.0)),false);
        assert_eq!(swapped.aim,d.aim);
        assert_eq!(swapped.fire,[false,true]);
    }

    #[test]
    fn mortar_uses_gun_cooldown_and_reload_windows() {
        let mut gun = weapon(WeaponKind::Gun);
        gun.cooldown = 0.2;
        let mut obs = observation([Some(gun),Some(weapon(WeaponKind::Mortar))]);
        let d = decide(&obs,Some(&target(0.0)),false);
        assert!(d.fire[1]);
        assert!(d.aim.unwrap()[1]>0.1);
        obs.self_obs.weapons[0].as_mut().unwrap().reload = Some(1.0);
        let reload = decide(&obs,Some(&target(0.0)),false);
        assert!(!reload.fire[0]);
        assert!(reload.fire[1]);
    }

    // Advance only torso slew and gun/mortar readiness for one two-tick action.
    // This checks the policy across decisions without claiming full hit physics.
    fn advance_gun_mortar_action(obs: &mut Observation, d: &CombatDecision) -> [bool; 2] {
        let mut fired = [false; 2];
        let command = d.aim.unwrap();
        for _ in 0..2 {
            obs.self_obs.aim = slew_one_tick(
                obs.self_obs.aim, command,
                obs.self_obs.stat("turn_rate", 4.0),
                obs.self_obs.stat("pitch_rate", 3.0),
            );
            for (slot, w) in obs.self_obs.weapons.iter_mut().enumerate() {
                let Some(w) = w else { continue };
                w.cooldown = (w.cooldown - 0.05).max(0.0);
                if d.fire[slot] && w.cooldown == 0.0 && w.reload.unwrap_or(0.0) == 0.0 {
                    fired[slot] = true;
                    w.cooldown = w.stat("cooldown", if w.kind == WeaponKind::Gun { 0.24 } else { 2.6 });
                }
            }
            obs.tick += 1;
            obs.t += 0.05;
        }
        fired
    }

    fn cooldown_window_fixture(distance: f64, gun_cd: f64, mortar_cd: f64, gun_slot: usize)
        -> (Observation, CombatTarget)
    {
        let mut gun = weapon(WeaponKind::Gun);
        gun.cooldown = gun_cd;
        let mut mortar = weapon(WeaponKind::Mortar);
        mortar.cooldown = mortar_cd;
        let mut obs = observation([Some(gun), Some(mortar)]);
        if gun_slot == 1 { obs.self_obs.weapons.swap(0, 1); }
        let mut t = target(0.0);
        t.pos[0] = distance;
        obs.self_obs.aim = aim_angles(muzzle_pos(&obs.self_obs), t.pos);
        (obs, t)
    }

    #[test]
    fn cooldown_window_keeps_gun_when_mortar_cannot_fire_in_time() {
        for distance in [130.0, 160.0] {
            for gun_slot in 0..2 {
                let (mut obs, t) = cooldown_window_fixture(distance, 0.14, 0.0, gun_slot);
                let gun_aim = obs.self_obs.aim;
                let first = decide(&obs, Some(&t), false);
                assert_eq!(first.aim, Some(gun_aim), "distance={distance}, slot={gun_slot}");
                assert!(!first.fire[1 - gun_slot]);
                assert_eq!(advance_gun_mortar_action(&mut obs, &first), [false, false]);

                let second = decide(&obs, Some(&t), false);
                assert!(second.fire[gun_slot]);
                let fired = advance_gun_mortar_action(&mut obs, &second);
                assert!(fired[gun_slot], "ready gun must not lose its next firing window");
                assert!(!fired[1 - gun_slot]);
            }
        }
    }

    #[test]
    fn cooldown_window_preserves_a_mortar_shot_and_timely_gun_return() {
        // At 100 m the ready mortar fits one tick. At 160 m a cooling mortar
        // needs two ticks, and the gun's longer window allows the return slew.
        for (distance, gun_cd, mortar_cd) in [(100.0, 0.14, 0.0), (160.0, 0.19, 0.08)] {
            for gun_slot in 0..2 {
                let (mut obs, t) = cooldown_window_fixture(distance, gun_cd, mortar_cd, gun_slot);
                let first = decide(&obs, Some(&t), false);
                assert!(first.fire[1 - gun_slot]);
                let first_fired = advance_gun_mortar_action(&mut obs, &first);
                assert!(first_fired[1 - gun_slot]);
                assert!(!first_fired[gun_slot]);

                let second = decide(&obs, Some(&t), false);
                let second_fired = advance_gun_mortar_action(&mut obs, &second);
                assert!(second_fired[gun_slot], "distance={distance}, slot={gun_slot}");
                assert!(!second_fired[1 - gun_slot]);
            }
        }
    }

    #[test]
    fn cooldown_window_rejects_a_mortar_shot_that_delays_ready_gun() {
        for gun_slot in 0..2 {
            let (mut obs, t) = cooldown_window_fixture(160.0, 0.14, 0.08, gun_slot);
            let first = decide(&obs, Some(&t), false);
            assert!(!first.fire[1 - gun_slot]);
            assert_eq!(advance_gun_mortar_action(&mut obs, &first), [false, false]);
            let second = decide(&obs, Some(&t), false);
            let fired = advance_gun_mortar_action(&mut obs, &second);
            assert!(fired[gun_slot]);
            assert!(!fired[1 - gun_slot]);
        }
    }

    #[test]
    fn reloading_gun_still_allows_multi_decision_mortar_preparation() {
        for gun_slot in 0..2 {
            let (mut obs, t) = cooldown_window_fixture(160.0, 0.14, 0.0, gun_slot);
            obs.self_obs.weapons[gun_slot].as_mut().unwrap().reload = Some(1.0);
            let first = decide(&obs, Some(&t), false);
            assert!(first.aim.unwrap()[1] > 0.3);
            assert_eq!(advance_gun_mortar_action(&mut obs, &first), [false, false]);
            let second = decide(&obs, Some(&t), false);
            let fired = advance_gun_mortar_action(&mut obs, &second);
            assert!(!fired[gun_slot]);
            assert!(fired[1 - gun_slot]);
        }
    }

    #[test]
    fn cooldown_on_second_tick_uses_second_tick_slew() {
        let mut gun = weapon(WeaponKind::Gun);
        gun.cooldown = 0.08;
        let mut inactive_laser = weapon(WeaponKind::Laser);
        inactive_laser.cooldown = 1.0;
        let mut obs = observation([Some(gun),Some(inactive_laser)]);
        obs.self_obs.aim = [0.3,0.0];
        assert!(decide(&obs,Some(&target(0.0)),false).fire[0]);
        obs.self_obs.weapons[0].as_mut().unwrap().cooldown = 0.0;
        assert!(!decide(&obs,Some(&target(0.0)),false).fire[0]);
    }

    #[test]
    fn lost_target_releases_at_current_aim_without_prolonging_slowdown() {
        let mut laser = weapon(WeaponKind::Laser);
        laser.charge = Some(0.3);
        let mut obs = observation([Some(weapon(WeaponKind::Gun)),Some(laser)]);
        obs.self_obs.aim = [0.8,0.2];
        let release = decide(&obs,None,false);
        assert_eq!(release.aim,Some(obs.self_obs.aim));
        assert_eq!(release.fire,[false,false]);
        assert!(!release.charging_laser);
        let escape = decide(&obs,None,true);
        assert_eq!(escape.aim,Some(obs.self_obs.aim));
        assert_eq!(escape.fire,[false,false]);
        assert!(!escape.charging_laser);

        obs.self_obs.weapons[1].as_mut().unwrap().charge = Some(2.7);
        let deadline = decide(&obs,None,false);
        assert_eq!(deadline.aim,Some(obs.self_obs.aim));
        assert!(!deadline.fire[1]);
        obs.self_obs.weapons[1].as_mut().unwrap().charge = Some(0.05);
        let below_minimum = decide(&obs,None,false);
        assert_eq!(below_minimum.aim,Some(obs.self_obs.aim));
        assert!(!below_minimum.fire[1]);
    }

    #[test]
    fn urgent_escape_requests_release_even_before_minimum_charge() {
        let mut laser = weapon(WeaponKind::Laser);
        laser.charge = Some(0.05);
        let obs = observation([Some(weapon(WeaponKind::Gun)),Some(laser)]);
        let d = decide(&obs,Some(&target(0.0)),true);
        assert_eq!(d.fire,[true,false]);
        assert!(!d.charging_laser);
        assert!(d.aim.is_some());
    }

    #[test]
    fn shorted_laser_does_not_start_or_steal_gun_aim() {
        let obs = observation([Some(weapon(WeaponKind::Gun)),Some(weapon(WeaponKind::Laser))]);
        let mut world = WorldState::new(16);
        world.shorted_until_t = 10.0;
        let d = evaluate_combat(&obs,&config(),&ArenaMap::empty(),&world,
            Some(&target(19.5)),false,false,false,false);
        assert_eq!(d.fire,[true,false]);
        assert!(d.aim.unwrap()[0]>0.07);
    }

    #[test]
    fn close_pressure_keeps_gun_and_arc_return_fire() {
        let obs = observation([
            Some(weapon(WeaponKind::Gun)),Some(weapon(WeaponKind::Lightning)),
        ]);
        let mut close = target(12.0);
        close.pos[0] = 30.0;
        let d = decide(&obs,Some(&close),true);
        assert_eq!(d.fire,[true,true]);
    }

    #[test]
    fn charged_laser_does_not_release_at_an_occluded_corner() {
        // A real opaque wall fixture verifies that retaining a charge is distinct
        // from firing, and that a missed gun shot is not manufactured by arbitration.
        let dir = std::env::temp_dir().join(format!("mechbot-combat-wall-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("world.json"),json!({
            "bounds":{"minX":-10,"maxX":110,"minY":-10,"maxY":10},
            "terrain":{"minX":-10,"minY":-10,"step":60,"nx":3,"ny":3,"scale":0.01},
            "prisms":[{"base":0,"top":20,"ring":[45,-10,55,-10,55,10,45,10]}]
        }).to_string()).unwrap();
        std::fs::write(dir.join("terrain.i16"),[0u8;18]).unwrap();
        let map = ArenaMap::load(&crate::protocol::HelloMap {
            dir:dir.to_string_lossy().into_owned(),
            files:vec!["world.json".into(),"terrain.i16".into()],
        });
        let mut laser = weapon(WeaponKind::Laser);
        laser.charge = Some(0.75);
        let obs = observation([Some(weapon(WeaponKind::Gun)),Some(laser)]);
        let t = target(0.0);
        assert!(map.loaded);
        assert!(!map.has_los(muzzle_pos(&obs.self_obs),t.pos));
        let d = evaluate_combat(&obs,&config(),&map,&WorldState::new(16),
            Some(&t),false,false,true,false);
        assert_eq!(d.fire,[false,true]);
        assert!(d.charging_laser);
        std::fs::remove_file(dir.join("world.json")).unwrap();
        std::fs::remove_file(dir.join("terrain.i16")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn gun_only_preserves_legacy_aim_fire_reload_and_urgent_behavior() {
        // Golden outcomes from the prior gun-only policy: no new global gate,
        // cooldown filtering, or suppression while escaping.
        for (yaw,vy,cooldown,expected_fire) in [
            (0.0,0.0,0.0,true),(0.0,19.5,0.0,true),
            (0.3,0.0,0.0,false),(0.3,0.0,0.08,false),
            (0.0,0.0,0.2,true),(1.0,19.5,0.0,false),
        ] {
            let mut gun = weapon(WeaponKind::Gun);
            gun.cooldown = cooldown;
            let mut obs = observation([Some(gun),None]);
            obs.self_obs.aim[0] = yaw;
            let t = target(vy);
            let (hit,_) = solve_linear_intercept(muzzle_pos(&obs.self_obs),252.0,t.pos,t.vel).unwrap();
            let expected_aim = aim_angles(muzzle_pos(&obs.self_obs),hit);
            for urgent in [false,true] {
                let d = decide(&obs,Some(&t),urgent);
                assert_eq!(d.aim,Some(expected_aim));
                assert_eq!(d.fire,[expected_fire,false]);
                assert!(!d.reload);
            }
        }
        let mut gun = weapon(WeaponKind::Gun);
        gun.ammo = Some(3.0);
        let mut obs = observation([Some(gun),None]);
        let idle = decide(&obs,None,false);
        assert!(idle.reload);
        assert_eq!(idle.aim,None);
        obs.self_obs.weapons[0].as_mut().unwrap().reload = Some(1.0);
        assert!(!decide(&obs,Some(&target(0.0)),false).fire[0]);
    }
}
