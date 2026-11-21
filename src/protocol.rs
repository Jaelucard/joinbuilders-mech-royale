#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Vec2 = [f64; 2];
pub type Vec3 = [f64; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeaponKind {
    Gun,
    Mortar,
    Laser,
    Lightning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Mech,
    Npc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DistBand {
    Near,
    Mid,
    Far,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hello {
    pub protocol: String,
    pub slot: u64,
    pub seed: u64,
    pub map: HelloMap,
    pub config: Config,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HelloMap {
    pub dir: String,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub r#match: HashMap<String, f64>,
    #[serde(default)]
    pub bot: HashMap<String, f64>,
    #[serde(default)]
    pub mech: HashMap<String, f64>,
    pub vision: ConfigVision,
    pub sound: ConfigSound,
    pub weapons: ConfigWeapons,
    #[serde(default)]
    pub projectile: HashMap<String, f64>,
    #[serde(default)]
    pub cubes: ConfigCubes,
    #[serde(default)]
    pub crates: HashMap<String, f64>,
    pub loot: ConfigLoot,
    #[serde(default)]
    pub npc: HashMap<String, f64>,
    #[serde(default)]
    pub drops: Option<ConfigDrops>,
    pub zone: ConfigZone,
    #[serde(default)]
    pub debug: DebugCaps,
    #[serde(default)]
    pub upgrades: Vec<UpgradeDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigVision {
    pub range: f64,
    pub fov_deg: f64,
    pub near_range: f64,
    #[serde(default)]
    pub canopy_blocks: bool,
    #[serde(default)]
    pub show_enemy_hp: bool,
    #[serde(default = "default_memory_s")]
    pub memory_s: f64,
}

fn default_memory_s() -> f64 {
    5.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigSound {
    pub bearing_noise_deg: f64,
    pub near: f64,
    pub mid: f64,
    pub step_speed: f64,
    pub step_every: f64,
    #[serde(default)]
    pub range: HashMap<String, f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigWeapons {
    #[serde(default)]
    pub gun: HashMap<String, f64>,
    #[serde(default)]
    pub mortar: HashMap<String, f64>,
    #[serde(default)]
    pub laser: HashMap<String, f64>,
    #[serde(default)]
    pub lightning: HashMap<String, f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigCubes {
    #[serde(default)]
    pub upgrade_thresholds: Vec<f64>,
    #[serde(default = "default_cube_pickup")]
    pub pickup_radius: f64,
    #[serde(default = "default_dmg_per_cube")]
    pub damage_per_cube: f64,
    #[serde(default = "default_stat_cap")]
    pub stat_cap: f64,
    #[serde(default = "default_size_per_cube")]
    pub size_per_cube: f64,
}

fn default_cube_pickup() -> f64 {
    3.5
}
fn default_dmg_per_cube() -> f64 {
    0.04
}
fn default_stat_cap() -> f64 {
    10.0
}
fn default_size_per_cube() -> f64 {
    0.05
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigLoot {
    pub pickup_radius: f64,
    pub equip_s: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDrops {
    #[serde(default)]
    pub times: Vec<f64>,
    pub fall_s: f64,
    pub radius: f64,
    pub capture_s: f64,
    pub cubes: f64,
    pub repair: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigZone {
    pub start_center: Vec2,
    pub start_radius: f64,
    #[serde(default)]
    pub phases: Vec<ZonePhase>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZonePhase {
    pub hold_s: f64,
    pub shrink_s: f64,
    pub radius: f64,
    pub dps: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DebugCaps {
    #[serde(default = "default_cap_64")]
    pub max_lines: usize,
    #[serde(default = "default_cap_64")]
    pub max_points: usize,
    #[serde(default = "default_cap_16")]
    pub max_text: usize,
    #[serde(default = "default_cap_80")]
    pub max_text_chars: usize,
    #[serde(default = "default_cap_bytes")]
    pub max_bytes: usize,
    #[serde(default = "default_cap_match_bytes")]
    pub max_match_bytes: usize,
    #[serde(default = "default_hold_s")]
    pub hold_s: f64,
}

impl Default for DebugCaps {
    fn default() -> Self {
        Self {
            max_lines: 64,
            max_points: 64,
            max_text: 16,
            max_text_chars: 80,
            max_bytes: 8192,
            max_match_bytes: 8_000_000,
            hold_s: 0.5,
        }
    }
}

fn default_cap_64() -> usize {
    64
}
fn default_cap_16() -> usize {
    16
}
fn default_cap_80() -> usize {
    80
}
fn default_cap_bytes() -> usize {
    8192
}
fn default_cap_match_bytes() -> usize {
    8_000_000
}
fn default_hold_s() -> f64 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpgradeDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    #[serde(default)]
    pub requires: Option<WeaponKind>,
    pub max: f64,
    #[serde(default)]
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Effect {
    pub path: String,
    #[serde(default)]
    pub mul: Option<f64>,
    #[serde(default)]
    pub add: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Observation {
    pub tick: u64,
    pub t: f64,
    pub alive: u64,
    pub self_obs: SelfObs,
    #[serde(default)]
    pub mechs: Vec<ActorObs>,
    #[serde(default)]
    pub projectiles: Vec<ProjectileObs>,
    #[serde(default)]
    pub crates: Vec<ItemObs>,
    #[serde(default)]
    pub cubes: Vec<ItemObs>,
    #[serde(default)]
    pub pickups: Vec<PickupObs>,
    #[serde(default)]
    pub sounds: Vec<SoundObs>,
    #[serde(default)]
    pub beams: Vec<BeamObs>,
    #[serde(default)]
    pub events: Vec<serde_json::Value>,
    pub zone: ZoneObs,
    #[serde(default)]
    pub offer: Option<Offer>,
    #[serde(default)]
    pub drops: Vec<DropObs>,
}

/// Custom wrapper so we map JSON `"self"` cleanly to `self_obs`.
#[derive(Debug, Clone, Deserialize)]
pub struct RawObservation {
    pub tick: u64,
    pub t: f64,
    pub alive: u64,
    #[serde(rename = "self")]
    pub self_obs: SelfObs,
    #[serde(default)]
    pub mechs: Vec<ActorObs>,
    #[serde(default)]
    pub projectiles: Vec<ProjectileObs>,
    #[serde(default)]
    pub crates: Vec<ItemObs>,
    #[serde(default)]
    pub cubes: Vec<ItemObs>,
    #[serde(default)]
    pub pickups: Vec<PickupObs>,
    #[serde(default)]
    pub sounds: Vec<SoundObs>,
    #[serde(default)]
    pub beams: Vec<BeamObs>,
    #[serde(default)]
    pub events: Vec<serde_json::Value>,
    pub zone: ZoneObs,
    #[serde(default)]
    pub offer: Option<Offer>,
    #[serde(default)]
    pub drops: Vec<DropObs>,
}

impl From<RawObservation> for Observation {
    fn from(r: RawObservation) -> Self {
        Self {
            tick: r.tick,
            t: r.t,
            alive: r.alive,
            self_obs: r.self_obs,
            mechs: r.mechs,
            projectiles: r.projectiles,
            crates: r.crates,
            cubes: r.cubes,
            pickups: r.pickups,
            sounds: r.sounds,
            beams: r.beams,
            events: r.events,
            zone: r.zone,
            offer: r.offer,
            drops: r.drops,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelfObs {
    pub id: u64,
    pub pos: Vec3,
    pub vel: Vec3,
    pub aim: Vec2,
    pub hp: f64,
    pub max_hp: f64,
    pub energy: f64,
    pub max_energy: f64,
    pub grounded: bool,
    pub cubes: u64,
    #[serde(default)]
    pub upgrades: Vec<String>,
    pub weapons: [Option<WeaponObs>; 2],
    pub in_zone: bool,
    #[serde(default)]
    pub stats: HashMap<String, f64>,
}

impl SelfObs {
    pub fn stat(&self, key: &str, default: f64) -> f64 {
        self.stats.get(key).copied().unwrap_or(default)
    }

    pub fn has_weapon(&self, kind: WeaponKind) -> bool {
        self.weapons
            .iter()
            .any(|w| w.as_ref().is_some_and(|w| w.kind == kind))
    }

    pub fn weapon_slot(&self, kind: WeaponKind) -> Option<usize> {
        self.weapons
            .iter()
            .position(|w| w.as_ref().is_some_and(|w| w.kind == kind))
    }

    pub fn empty_slot(&self) -> Option<usize> {
        self.weapons.iter().position(|w| w.is_none())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeaponObs {
    pub kind: WeaponKind,
    pub cooldown: f64,
    #[serde(default)]
    pub ammo: Option<f64>,
    #[serde(default)]
    pub reload: Option<f64>,
    #[serde(default)]
    pub charge: Option<f64>,
    #[serde(default)]
    pub stats: HashMap<String, f64>,
}

impl WeaponObs {
    pub fn stat(&self, key: &str, default: f64) -> f64 {
        self.stats.get(key).copied().unwrap_or(default)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActorObs {
    pub id: u64,
    pub kind: ActorKind,
    pub pos: Vec3,
    pub vel: Vec3,
    pub aim: Vec2,
    #[serde(default)]
    pub hp: Option<f64>,
    #[serde(default)]
    pub max_hp: Option<f64>,
    #[serde(default)]
    pub cubes: Option<u64>,
    #[serde(default)]
    pub weapons: Vec<WeaponKind>,
    #[serde(default)]
    pub charging: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectileObs {
    pub id: u64,
    pub kind: String,
    pub pos: Vec3,
    pub vel: Vec3,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ItemObs {
    pub id: u64,
    pub pos: Vec3,
    #[serde(default)]
    pub hp: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PickupObs {
    pub id: u64,
    pub kind: WeaponKind,
    pub pos: Vec3,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SoundObs {
    pub kind: String,
    pub bearing: f64,
    pub dist: DistBand,
    pub age: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BeamObs {
    pub by: u64,
    pub kind: String,
    #[serde(default)]
    pub path: Vec<Vec3>,
    pub age: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZoneObs {
    pub center: Vec2,
    pub radius: f64,
    pub next_center: Vec2,
    pub next_radius: f64,
    pub shrink_start: f64,
    pub shrink_end: f64,
    pub dps: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Offer {
    pub id: u64,
    pub choices: Vec<OfferChoice>,
    pub expires: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfferChoice {
    pub id: String,
    pub name: String,
    pub desc: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DropObs {
    pub id: u64,
    pub pos: Vec3,
    pub radius: f64,
    pub lands_in: f64,
    pub capture_s: f64,
    pub progress: f64,
    pub holder: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EndMsg {
    pub place: u64,
    pub kills: u64,
}

#[derive(Debug, Clone)]
pub enum ServerMessage {
    Hello(Box<Hello>),
    Obs(Box<Observation>),
    End(EndMsg),
    Unknown,
}

pub fn parse_server_message(line: &str) -> Result<ServerMessage, serde_json::Error> {
    let val: serde_json::Value = serde_json::from_str(line)?;
    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match msg_type {
        "hello" => {
            let hello: Hello = serde_json::from_value(val)?;
            Ok(ServerMessage::Hello(Box::new(hello)))
        }
        "obs" => {
            let raw: RawObservation = serde_json::from_value(val)?;
            Ok(ServerMessage::Obs(Box::new(raw.into())))
        }
        "end" => {
            let end: EndMsg = serde_json::from_value(val)?;
            Ok(ServerMessage::End(end))
        }
        _ => Ok(ServerMessage::Unknown),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Ready {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub name: String,
}

impl Ready {
    pub fn new(name: &str) -> Self {
        let clean: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(32)
            .collect();
        Self {
            msg_type: "ready",
            name: clean,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Action {
    pub tick: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#move: Option<Vec2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aim: Option<Vec2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire: Option<[bool; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugDraw>,
}

impl Action {
    pub fn sanitize(&mut self, min_pitch: f64, max_pitch: f64, caps: &DebugCaps) {
        if let Some(m) = &mut self.r#move {
            if !m[0].is_finite() || !m[1].is_finite() {
                *m = [0.0, 0.0];
            } else {
                let len = (m[0] * m[0] + m[1] * m[1]).sqrt();
                if len > 1.0 {
                    m[0] /= len;
                    m[1] /= len;
                }
            }
        }
        if let Some(a) = &mut self.aim {
            if !a[0].is_finite() {
                a[0] = 0.0;
            }
            if !a[1].is_finite() {
                a[1] = 0.0;
            }
            a[1] = a[1].clamp(min_pitch, max_pitch);
        }
        if let Some(d) = &mut self.debug {
            d.trim(caps);
            if d.is_empty() {
                self.debug = None;
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DebugDraw {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<DebugLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<DebugPoint>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<DebugText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl DebugDraw {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
            && self.points.is_empty()
            && self.text.is_empty()
            && self.label.is_none()
    }

    pub fn line(&mut self, a: Vec3, b: Vec3, color: &str) {
        if a.iter().all(|v| v.is_finite()) && b.iter().all(|v| v.is_finite()) {
            self.lines.push(DebugLine {
                a: round3(a),
                b: round3(b),
                color: Some(color.to_string()),
            });
        }
    }

    pub fn point(&mut self, at: Vec3, color: &str, size: f64) {
        if at.iter().all(|v| v.is_finite()) {
            self.points.push(DebugPoint {
                at: round3(at),
                color: Some(color.to_string()),
                size: Some(size.clamp(1.0, 12.0)),
            });
        }
    }

    pub fn set_label(&mut self, label: String) {
        self.label = Some(label);
    }

    pub fn trim(&mut self, caps: &DebugCaps) {
        self.lines.truncate(caps.max_lines);
        self.points.truncate(caps.max_points);
        self.text.truncate(caps.max_text);
        if let Some(l) = &mut self.label {
            if l.len() > caps.max_text_chars {
                l.truncate(caps.max_text_chars);
            }
        }
        for t in &mut self.text {
            if t.text.len() > caps.max_text_chars {
                t.text.truncate(caps.max_text_chars);
            }
        }
        // Keep well under max_bytes
        if let Ok(s) = serde_json::to_string(self) {
            if s.len() > caps.max_bytes {
                self.points.clear();
                self.lines.truncate(8);
            }
        }
    }
}

fn round3(v: Vec3) -> Vec3 {
    [
        (v[0] * 100.0).round() / 100.0,
        (v[1] * 100.0).round() / 100.0,
        (v[2] * 100.0).round() / 100.0,
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugLine {
    pub a: Vec3,
    pub b: Vec3,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugPoint {
    pub at: Vec3,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugText {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<Vec3>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Normalize angle to [-PI, PI].
#[inline]
pub fn wrap_angle(mut a: f64) -> f64 {
    const TWO_PI: f64 = std::f64::consts::TAU;
    a = (a + std::f64::consts::PI).rem_euclid(TWO_PI) - std::f64::consts::PI;
    a
}

#[inline]
pub fn dist2d(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

#[inline]
pub fn dist3d(a: Vec3, b: Vec3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
