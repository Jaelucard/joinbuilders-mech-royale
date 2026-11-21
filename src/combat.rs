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
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct CombatDecision {
    pub aim: Option<[f64; 2]>,
    pub fire: [bool; 2],
    pub reload: bool,
    pub charging_laser: bool,
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
            // Target moving faster than projectile; fallback to simple distance/speed
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
            target_pos[2] + vz * t * 0.5, // damp vertical extrapolation (jumps/gravity)
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

/// Check if a mortar parabolic trajectory clears buildings/terrain.
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

/// Select the highest-value enemy mech target (or NPC/crate if `allow_farm` and no enemy threat).
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
            // Still consider visible enemies within 100m if we want to track them with torso
            if !e.visible_now || d > 110.0 {
                continue;
            }
        }

        // If holding fire to finish regen, only engage very close pushing threats
        if hold_fire_for_regen && d > 32.0 {
            continue;
        }

        let mut score = 220.0 - d * 0.65;
        if los {
            score += 90.0;
        }
        // Finish low-HP enemies!
        let hp_frac = (e.hp / e.max_hp.max(1.0)).clamp(0.0, 1.0);
        score += (1.0 - hp_frac) * 110.0;
        if e.hp < 320.0 {
            score += 65.0;
        }
        // Threat priority: enemies aiming at us or who recently damaged us
        if e.aiming_at_me {
            score += 45.0;
        }
        if obs.t - e.last_damaged_me_t < 3.5 {
            score += 55.0;
        }
        if e.charging {
            score += 50.0;
        }
        // Weapon triangle bonuses!
        if has_laser && e.has_weapon(WeaponKind::Mortar) {
            score += 60.0; // +60% cookoff!
        }
        if has_lightning && e.has_weapon(WeaponKind::Laser) && d < 65.0 {
            score += 55.0; // short their laser!
        }
        // Rear flank bonus
        let from_enemy_yaw = (me.pos[1] - epos[1]).atan2(me.pos[0] - epos[0]);
        if wrap_angle(from_enemy_yaw - e.aim[0]).abs() > std::f64::consts::FRAC_PI_2 {
            score += 25.0;
        }
        // Slight penalty for stale non-visible memory target (mortar lob only)
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
            score,
        };
        if best_enemy.as_ref().is_none_or(|b| cand.score > b.score) {
            best_enemy = Some(cand);
        }
    }

    if best_enemy.is_some() {
        return best_enemy;
    }

    // If holding fire to regen, do NOT shoot NPCs or crates!
    if hold_fire_for_regen || !allow_farm {
        return None;
    }

    // Consider NPC drones (2 cubes!) or nearby crates (1 cube + 50% weapon!)
    let mut best_farm: Option<CombatTarget> = None;
    for npc in &world.npcs {
        let center = [npc.pos[0], npc.pos[1], npc.pos[2] + 1.75];
        let d = dist3d(muzzle, center);
        if d > 95.0 || !map.has_los(muzzle, center) {
            continue;
        }
        let hp = npc.hp.unwrap_or(350.0);
        let score = 140.0 - d + (350.0 - hp) * 0.25;
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
            score,
        };
        if best_farm.as_ref().is_none_or(|b| cand.score > b.score) {
            best_farm = Some(cand);
        }
    }

    for cr in &obs.crates {
        let center = [cr.pos[0], cr.pos[1], cr.pos[2] + 1.2];
        let d = dist3d(muzzle, center);
        if d > 85.0 || !map.has_los(muzzle, center) {
            continue;
        }
        let hp = cr.hp.unwrap_or(300.0);
        let score = 125.0 - d + (300.0 - hp) * 0.25 + if me.empty_slot().is_some() { 30.0 } else { 0.0 };
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
            score,
        };
        if best_farm.as_ref().is_none_or(|b| cand.score > b.score) {
            best_farm = Some(cand);
        }
    }

    best_farm
}

/// Evaluate aim, trigger control for both weapon slots, and reload timing.
pub fn evaluate_combat(
    obs: &Observation,
    cfg: &Config,
    map: &ArenaMap,
    world: &WorldState,
    target: Option<&CombatTarget>,
    urgent_move: bool,
    hold_fire_for_regen: bool,
) -> CombatDecision {
    let me = &obs.self_obs;
    let mut out = CombatDecision::default();

    // Compute desired combat range based on our loadout
    out.desired_range = preferred_range(me, target);

    // Check proactive reload when no active firing target
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
    let cube_mult = 1.0 + cfg.cubes.stat_cap.min(me.cubes as f64) * cfg.cubes.damage_per_cube;

    // Track best desired aim with priority: Charging Laser > Lightning (close) > Ready Laser > Gun > Mortar
    let mut best_aim: Option<(i32, [f64; 2])> = None;
    let mut set_aim = |prio: i32, a: [f64; 2]| {
        if best_aim.is_none_or(|(p, _)| prio > p) {
            best_aim = Some((prio, a));
        }
    };

    // Always at least face the target if visible
    let direct_aim = aim_angles(muzzle, t.pos);
    set_aim(1, direct_aim);

    if hold_fire_for_regen && (!t.is_mech || dist > 32.0) {
        out.aim = best_aim.map(|(_, a)| a);
        out.reload = gun_needs_passive_reload;
        return out;
    }

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

                // Slightly lead by 0.05s for 1-tick server slew/latency
                let lead_pos = [
                    t.pos[0] + t.vel[0] * 0.05,
                    t.pos[1] + t.vel[1] * 0.05,
                    t.pos[2],
                ];
                let desired = aim_angles(muzzle, lead_pos);

                if cur_charge > 0.0 {
                    // Currently charging!
                    set_aim(100, desired);
                    let yaw_err = wrap_angle(desired[0] - me.aim[0]).abs();
                    let pitch_err = (desired[1] - me.aim[1]).abs();
                    let yaw_tol = (t.radius * 0.80 / horiz_dist).atan().max(0.016);
                    let pitch_tol = (t.half_height * 0.78 / dist.max(1.0)).atan().max(0.016);
                    let aligned = yaw_err <= yaw_tol && pitch_err <= pitch_tol;

                    let frac = (cur_charge / full_charge.max(0.05)).clamp(0.0, 1.0);
                    let cookoff = if t.has_mortar { 1.6 } else { 1.0 };
                    let est_dmg = max_dmg * (min_frac + (1.0 - min_frac) * frac) * cube_mult * cookoff;

                    let should_release = has_los
                        && dist <= range - 1.0
                        && aligned
                        && (cur_charge >= full_charge - 0.06
                            || (cur_charge >= min_charge && est_dmg >= t.hp * 1.05)
                            || cur_charge >= full_charge + hold_s - 0.35);

                    if should_release {
                        // LET GO to fire hitscan beam!
                        out.fire[slot] = false;
                        out.charging_laser = false;
                    } else {
                        // Keep holding charge
                        out.fire[slot] = true;
                        out.charging_laser = true;
                    }
                } else if w.cooldown <= 0.05
                    && obs.t >= world.shorted_until_t
                    && !urgent_move
                    && dist <= range - 2.0
                    && has_los
                {
                    // Don't waste laser cooldown on a tiny crate if an enemy is nearby, otherwise fire away
                    let ok_target = t.is_mech || (!t.is_crate) || t.hp >= 220.0;
                    if ok_target {
                        set_aim(80, desired);
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
                    let min_energy = if t.is_mech && t.hp < 220.0 {
                        e_cost
                    } else if t.is_mech {
                        e_cost + 8.0
                    } else {
                        e_cost + 25.0
                    };
                    if me.energy >= min_energy {
                        // Lower aim priority than gun lead if enemy is already safely inside lightning cone
                        let l_prio = if yaw_err <= cone_rad * 0.6 { 55 } else { 90 };
                        set_aim(l_prio, direct_aim);
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

                if !reloading && ammo >= 1.0 && dist <= range - 2.0 {
                    if let Some((pred, _tf)) = solve_linear_intercept(muzzle, speed, t.pos, t.vel) {
                        let pred_los = has_los && map.has_los(muzzle, pred);
                        if pred_los {
                            let desired = aim_angles(muzzle, pred);
                            set_aim(60, desired);
                            let yaw_err = wrap_angle(desired[0] - me.aim[0]).abs();
                            let pitch_err = (desired[1] - me.aim[1]).abs();
                            let yaw_tol = ((t.radius * 1.35) / horiz_dist).atan().max(0.055);
                            let pitch_tol = ((t.half_height * 1.4) / dist.max(1.0)).atan().max(0.055);
                            if yaw_err <= yaw_tol && pitch_err <= pitch_tol {
                                out.fire[slot] = true;
                            }
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
                if w.cooldown <= 0.10 && horiz_dist >= (splash_r * 2.1).max(15.0) && horiz_dist <= 230.0 {
                    let origin = mortar_origin_pos(me);
                    // Aim at enemy feet so splash hits cleanly!
                    let mut aim_pt = [t.pos[0], t.pos[1], t.pos[2] - t.half_height + 0.5];
                    let mut chosen: Option<(f64, f64)> = None;

                    // 2-step iterative lead
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
                                    if tf <= 6.8 && mortar_arc_clear(map, origin, yaw, p, speed, gravity, tf) {
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
                        // Higher priority if we have no direct LoS (lobbing over buildings!)
                        let prio = if !has_los { 85 } else { 50 };
                        set_aim(prio, [yaw, pitch]);
                        let yaw_err = wrap_angle(yaw - me.aim[0]).abs();
                        let pitch_err = (pitch - me.aim[1]).abs();
                        if yaw_err <= 0.09 && pitch_err <= 0.07 {
                            out.fire[slot] = true;
                        }
                    }
                }
            }
        }
    }

    out.aim = best_aim.map(|(_, a)| a);
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
        if has_lightning && t.has_laser && me.energy > 25.0 {
            return (lightning_range * 0.75).max(20.0);
        }
        if has_laser && t.has_mortar {
            return 95.0; // outrange mortar and cookoff!
        }
    }

    if has_lightning && !has_laser && !has_gun {
        return lightning_range * 0.72;
    }
    if has_lightning && me.energy > 35.0 {
        // Hybrid: comfortable mid-close range if healthy
        if me.hp / me.max_hp.max(1.0) > 0.65 {
            return lightning_range * 0.80;
        }
    }
    if has_laser {
        return 85.0;
    }
    55.0
}

/// Rank weapon desirability given current upgrades.
pub fn weapon_value(kind: WeaponKind, me: &SelfObs) -> f64 {
    let count_upg = |prefix: &str| -> f64 {
        me.upgrades
            .iter()
            .filter(|u| u.starts_with(prefix))
            .count() as f64
    };
    match kind {
        WeaponKind::Laser => 105.0 + count_upg("laser_") * 18.0,
        WeaponKind::Lightning => {
            98.0 + count_upg("lightning_") * 18.0
                + if me.upgrades.iter().any(|u| u == "reactor" || u == "capacitor") {
                    10.0
                } else {
                    0.0
                }
        }
        WeaponKind::Gun => 78.0 + count_upg("gun_") * 16.0,
        WeaponKind::Mortar => 58.0 + count_upg("mortar_") * 16.0,
    }
}

/// Decide whether a visible pickup is worth walking over to grab.
pub fn is_pickup_desirable(kind: WeaponKind, me: &SelfObs) -> Option<Option<usize>> {
    if me.has_weapon(kind) {
        return None;
    }
    if me.empty_slot().is_some() {
        // Don't bother picking up a gun if we already have one, otherwise fill empty slot!
        if kind == WeaponKind::Gun {
            return None;
        }
        return Some(None);
    }
    // Check if swapping improves our loadout without dropping a charging laser
    let new_val = weapon_value(kind, me);
    let mut best_swap: Option<(f64, usize)> = None;
    for (slot, wopt) in me.weapons.iter().enumerate() {
        let Some(w) = wopt else { continue };
        if w.kind == WeaponKind::Laser && w.charge.unwrap_or(0.0) > 0.0 {
            continue; // cannot swap while charging
        }
        let gain = new_val - weapon_value(w.kind, me);
        if gain >= 16.0 && best_swap.is_none_or(|(bg, _)| gain > bg) {
            best_swap = Some((gain, slot));
        }
    }
    best_swap.map(|(_, slot)| Some(slot))
}

/// Return `(take_id, drop_slot)` if a desirable pickup is within reach right now.
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
            let val = weapon_value(p.kind, me) - dh * 0.5;
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

/// Score and pick the strongest upgrade choice from an offer.
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
            "nanites" => 115.0 + if hp_frac < 0.75 { 20.0 } else { 0.0 },
            "armor" => 104.0 + if late { 18.0 } else { 0.0 },
            "plating" => 98.0 + if late { 22.0 } else { 0.0 } + if hp_frac < 0.65 { 12.0 } else { 0.0 },
            "servos" => 92.0,
            "laser_charge" => 106.0,
            "laser_damage" => 96.0,
            "lightning_range" => 108.0,
            "lightning_efficiency" => 99.0,
            "lightning_chain" => 90.0,
            "gun_rate" => 88.0,
            "gun_damage" => 86.0,
            "gun_velocity" => 78.0,
            "gun_mag" => 74.0,
            "gyros" => 80.0 + if has_laser { 10.0 } else { 0.0 },
            "reactor" => 78.0 + if has_lightning { 16.0 } else { 0.0 },
            "jets" => 74.0,
            "capacitor" => 72.0 + if has_lightning { 14.0 } else { 0.0 },
            "sensors" => 70.0,
            "mortar_reload" => 68.0,
            "mortar_splash" => 66.0,
            "mortar_velocity" => 62.0,
            "laser_pierce" => 58.0,
            _ => 50.0,
        };
        // Slight preference for first nanites stack over 2nd, still very high either way
        if ch.id == "nanites" && me.upgrades.iter().any(|u| u == "nanites") {
            s -= 5.0;
        }
        if s > best_score {
            best_score = s;
            best_idx = i;
        }
    }
    best_idx
}

pub fn best_nearby_pickup<'a>(
    obs: &Observation,
    world: &'a WorldState,
    max_dist: f64,
    zone_center: [f64; 2],
    zone_radius: f64,
) -> Option<(Vec3, f64)> {
    let me = &obs.self_obs;
    let mut best: Option<(f64, Vec3)> = None;
    for p in world.pickups.values() {
        if is_pickup_desirable(p.kind, me).is_none() {
            continue;
        }
        let d = dist2d([me.pos[0], me.pos[1]], [p.pos[0], p.pos[1]]);
        if d > max_dist {
            continue;
        }
        if dist2d([p.pos[0], p.pos[1]], zone_center) > (zone_radius - 5.0).max(5.0) {
            continue;
        }
        // Avoid high rooftops we'd spend tons of energy climbing unless very close
        let dz = p.pos[2] - me.pos[2];
        if dz > 28.0 {
            continue;
        }
        let score = weapon_value(p.kind, me) - d * 1.1 - dz.max(0.0) * 1.5;
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
