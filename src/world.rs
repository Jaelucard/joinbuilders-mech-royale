use crate::protocol::{
    dist2d, wrap_angle, ActorKind, ActorObs, Config, DistBand, Observation, Vec2, Vec3, WeaponKind,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TrackedEnemy {
    pub id: u64,
    pub pos: Vec3,
    pub vel: Vec3,
    pub aim: Vec2,
    pub hp: f64,
    pub max_hp: f64,
    pub cubes: u64,
    pub weapons: Vec<WeaponKind>,
    pub charging: bool,
    pub last_seen_t: f64,
    pub visible_now: bool,
    pub last_damaged_me_t: f64,
    pub aiming_at_me: bool,
}

impl TrackedEnemy {
    pub fn estimated_pos(&self, now: f64) -> Vec3 {
        let dt = (now - self.last_seen_t).clamp(0.0, 2.0);
        [
            self.pos[0] + self.vel[0] * dt * 0.6,
            self.pos[1] + self.vel[1] * dt * 0.6,
            self.pos[2],
        ]
    }

    pub fn has_weapon(&self, kind: WeaponKind) -> bool {
        self.weapons.contains(&kind)
    }

    pub fn size_scale(&self, size_per_cube: f64) -> f64 {
        1.0 + size_per_cube * (self.cubes as f64)
    }
}

#[derive(Debug, Clone)]
pub struct RememberedPickup {
    pub kind: WeaponKind,
    pub pos: Vec3,
    pub last_seen_t: f64,
}

#[derive(Debug, Clone)]
pub struct RememberedItem {
    pub pos: Vec3,
    pub last_seen_t: f64,
}

#[derive(Debug, Clone)]
pub struct TimedSound {
    pub kind: String,
    pub bearing: f64,
    pub dist: DistBand,
    pub at_t: f64,
}

pub struct WorldState {
    pub enemies: HashMap<u64, TrackedEnemy>,
    pub npcs: Vec<ActorObs>,
    pub pickups: HashMap<u64, RememberedPickup>,
    pub cubes: HashMap<u64, RememberedItem>,
    pub crates: HashMap<u64, RememberedItem>,
    pub recent_sounds: Vec<TimedSound>,
    pub last_combat_t: f64,
    pub last_damaged_t: f64,
    pub last_damaged_bearing: Option<f64>,
    pub last_damaged_by: Option<u64>,
    pub shorted_until_t: f64,
    pub my_kills: u64,
    pub prev_alive: u64,
    pub other_deaths: u64,
    pub saw_inter_mech_fight: bool,
    pub last_aim: Vec2,
}

impl WorldState {
    pub fn new(initial_mechs: u64) -> Self {
        Self {
            enemies: HashMap::new(),
            npcs: Vec::new(),
            pickups: HashMap::new(),
            cubes: HashMap::new(),
            crates: HashMap::new(),
            recent_sounds: Vec::new(),
            last_combat_t: -100.0,
            last_damaged_t: -100.0,
            last_damaged_bearing: None,
            last_damaged_by: None,
            shorted_until_t: -100.0,
            my_kills: 0,
            prev_alive: initial_mechs,
            other_deaths: 0,
            saw_inter_mech_fight: false,
            last_aim: [0.0, 0.0],
        }
    }

    pub fn update(&mut self, obs: &Observation, cfg: &Config) {
        let now = obs.t;
        let me = &obs.self_obs;
        let my_pos = me.pos;

        if !me.in_zone {
            self.last_combat_t = now;
        }

        // Process events
        let mut kills_this_tick = 0u64;
        for ev in &obs.events {
            let etype = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match etype {
                "damaged" => {
                    self.last_combat_t = now;
                    self.last_damaged_t = now;
                    if let Some(b) = ev.get("bearing").and_then(|v| v.as_f64()) {
                        self.last_damaged_bearing = Some(b);
                    }
                    if let Some(by) = ev.get("by").and_then(|v| v.as_u64()) {
                        self.last_damaged_by = Some(by);
                        if let Some(e) = self.enemies.get_mut(&by) {
                            e.last_damaged_me_t = now;
                        }
                    }
                }
                "hit" => {
                    self.last_combat_t = now;
                }
                "effect" => {
                    let eff = ev.get("effect").and_then(|v| v.as_str()).unwrap_or("");
                    if eff == "short" {
                        let short_s = cfg.weapons.lightning.get("short_s").copied().unwrap_or(2.0);
                        self.shorted_until_t = now + short_s;
                    }
                }
                "kill" => {
                    kills_this_tick += 1;
                    self.my_kills += 1;
                    if let Some(tid) = ev.get("target").and_then(|v| v.as_u64()) {
                        self.enemies.remove(&tid);
                    }
                }
                "took" => {
                    if let Some(pid) = ev.get("id").and_then(|v| v.as_u64()) {
                        self.pickups.remove(&pid);
                    }
                }
                "take_failed" => {
                    let reason = ev.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    if reason == "gone" {
                        if let Some(pid) = ev.get("id").and_then(|v| v.as_u64()) {
                            self.pickups.remove(&pid);
                        }
                    }
                }
                _ => {}
            }
        }

        // Detect whether other mechs are fighting/killing each other (FFA vs Practice Squad)
        if obs.alive < self.prev_alive {
            let dropped = self.prev_alive - obs.alive;
            if dropped > kills_this_tick && obs.zone.radius > 40.0 {
                self.other_deaths += dropped - kills_this_tick;
            }
        }
        self.prev_alive = obs.alive;

        // Mark all tracked enemies unseen before updating visible list
        for e in self.enemies.values_mut() {
            e.visible_now = false;
            e.charging = false;
            e.aiming_at_me = false;
        }

        self.npcs.clear();
        let mut vis_mechs: Vec<&ActorObs> = Vec::new();
        for a in &obs.mechs {
            match a.kind {
                ActorKind::Npc => self.npcs.push(a.clone()),
                ActorKind::Mech => {
                    vis_mechs.push(a);
                    let to_me_yaw = (my_pos[1] - a.pos[1]).atan2(my_pos[0] - a.pos[0]);
                    let aim_err = wrap_angle(to_me_yaw - a.aim[0]).abs();
                    let aiming_at_me = aim_err < 0.35;
                    let entry = self.enemies.entry(a.id).or_insert_with(|| TrackedEnemy {
                        id: a.id,
                        pos: a.pos,
                        vel: a.vel,
                        aim: a.aim,
                        hp: a.hp.unwrap_or(1400.0),
                        max_hp: a.max_hp.unwrap_or(1400.0),
                        cubes: a.cubes.unwrap_or(0),
                        weapons: a.weapons.clone(),
                        charging: a.charging,
                        last_seen_t: now,
                        visible_now: true,
                        last_damaged_me_t: -100.0,
                        aiming_at_me,
                    });
                    entry.pos = a.pos;
                    entry.vel = a.vel;
                    entry.aim = a.aim;
                    if let Some(hp) = a.hp {
                        entry.hp = hp;
                    }
                    if let Some(mhp) = a.max_hp {
                        entry.max_hp = mhp;
                    }
                    if let Some(c) = a.cubes {
                        entry.cubes = c;
                    }
                    if !a.weapons.is_empty() {
                        entry.weapons = a.weapons.clone();
                    }
                    entry.charging = a.charging;
                    entry.last_seen_t = now;
                    entry.visible_now = true;
                    entry.aiming_at_me = aiming_at_me;
                }
            }
        }

        // Check if visible mechs are shooting at each other (via beams)
        for b in &obs.beams {
            if b.path.len() >= 2 {
                let end = b.path[b.path.len() - 1];
                for m in &vis_mechs {
                    if m.id != b.by && dist2d([end[0], end[1]], [m.pos[0], m.pos[1]]) < 10.0 {
                        self.saw_inter_mech_fight = true;
                    }
                }
            }
        }

        // Expire stale enemy memories after 10 s
        self.enemies.retain(|_, e| now - e.last_seen_t <= 10.0);

        // Update remembered pickups (remove if we are right next to it and don't see it)
        let seen_pickup_ids: Vec<u64> = obs.pickups.iter().map(|p| p.id).collect();
        for p in &obs.pickups {
            self.pickups.insert(
                p.id,
                RememberedPickup {
                    kind: p.kind,
                    pos: p.pos,
                    last_seen_t: now,
                },
            );
        }
        self.pickups.retain(|id, p| {
            if seen_pickup_ids.contains(id) {
                return true;
            }
            if dist2d([my_pos[0], my_pos[1]], [p.pos[0], p.pos[1]]) < 25.0 {
                return false;
            }
            now - p.last_seen_t <= 25.0
        });

        // Update remembered cubes
        let seen_cube_ids: Vec<u64> = obs.cubes.iter().map(|c| c.id).collect();
        for c in &obs.cubes {
            self.cubes.insert(
                c.id,
                RememberedItem {
                    pos: c.pos,
                    last_seen_t: now,
                },
            );
        }
        self.cubes.retain(|id, c| {
            if seen_cube_ids.contains(id) {
                return true;
            }
            if dist2d([my_pos[0], my_pos[1]], [c.pos[0], c.pos[1]]) < 22.0 {
                return false;
            }
            now - c.last_seen_t <= 15.0
        });

        // Update remembered crates
        let seen_crate_ids: Vec<u64> = obs.crates.iter().map(|c| c.id).collect();
        for c in &obs.crates {
            self.crates.insert(
                c.id,
                RememberedItem {
                    pos: c.pos,
                    last_seen_t: now,
                },
            );
        }
        self.crates.retain(|id, c| {
            if seen_crate_ids.contains(id) {
                return true;
            }
            if dist2d([my_pos[0], my_pos[1]], [c.pos[0], c.pos[1]]) < 25.0 {
                return false;
            }
            now - c.last_seen_t <= 20.0
        });

        // Update recent sounds
        for s in &obs.sounds {
            self.recent_sounds.push(TimedSound {
                kind: s.kind.clone(),
                bearing: s.bearing,
                dist: s.dist,
                at_t: now - s.age,
            });
        }
        self.recent_sounds.retain(|s| now - s.at_t <= 3.5);
    }

    /// True if match behavior strongly looks like 1v15 Practice Squad (enemies never kill each other).
    pub fn looks_like_practice_squad(&self, now: f64) -> bool {
        if self.other_deaths >= 2 || self.saw_inter_mech_fight {
            return false;
        }
        // Early/mid game with zero inter-bot deaths -> assume Practice squad unless proven FFA
        now < 120.0 || self.other_deaths == 0
    }

    pub fn time_until_regen(&self, obs: &Observation) -> f64 {
        let delay = obs.self_obs.stat("regen_delay", 5.0);
        (self.last_combat_t + delay - obs.t).max(0.0)
    }

    pub fn visible_enemies(&self) -> Vec<&TrackedEnemy> {
        self.enemies.values().filter(|e| e.visible_now).collect()
    }

    pub fn active_threat_count(&self, my_pos: Vec3, now: f64) -> usize {
        self.enemies
            .values()
            .filter(|e| {
                e.visible_now
                    && (e.aiming_at_me
                        || now - e.last_damaged_me_t < 4.0
                        || dist2d([my_pos[0], my_pos[1]], [e.pos[0], e.pos[1]]) < 75.0)
            })
            .count()
    }

    pub fn most_urgent_sound(&self, now: f64) -> Option<&TimedSound> {
        self.recent_sounds
            .iter()
            .filter(|s| {
                now - s.at_t < 1.8
                    && matches!(
                        s.kind.as_str(),
                        "laser_charge"
                            | "lightning"
                            | "gun"
                            | "laser"
                            | "mortar"
                            | "boost"
                            | "step"
                            | "jet"
                    )
            })
            .max_by(|a, b| {
                sound_priority(a, now)
                    .partial_cmp(&sound_priority(b, now))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

fn sound_priority(s: &TimedSound, now: f64) -> f64 {
    let dist_w = match s.dist {
        DistBand::Near => 3.0,
        DistBand::Mid => 1.5,
        DistBand::Far => 0.4,
    };
    let kind_w = match s.kind.as_str() {
        "laser_charge" => 4.0,
        "lightning" => 3.5,
        "gun" | "laser" | "mortar" => 2.2,
        "boost" | "jet" => 1.8,
        "step" => 1.5,
        _ => 0.5,
    };
    let recency = (1.8 - (now - s.at_t)).max(0.0);
    dist_w * kind_w * recency
}
