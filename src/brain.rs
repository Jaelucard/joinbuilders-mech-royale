use crate::combat::{
    best_nearby_pickup, choose_immediate_pickup, choose_upgrade, evaluate_combat, muzzle_pos,
    select_target, CombatTarget,
};
use crate::map::{ArenaMap, RoofAnchor};
use crate::nav::Navigator;
use crate::protocol::{
    dist2d, wrap_angle, Action, Config, DebugDraw, Hello, Observation, Vec2, Vec3, WeaponKind,
};
use crate::world::WorldState;

struct DropHold {
    id: u64,
    lands_at: f64,
    previous_progress: f64,
    last_progress_t: f64,
}

pub struct BotBrain {
    pub cfg: Config,
    pub map: ArenaMap,
    pub nav: Navigator,
    pub world: WorldState,
    prev_wanted_move: bool,
    regen_mode: bool,
    cached_cover: Option<(u64, Vec3)>,
    climb_target: Option<(u64, RoofAnchor)>,
    roam_anchor: Option<Vec2>,
    roam_tick: u64,
    // Unreachable target blacklisting (stops wasting ticks on rooftop cubes / blocked drops!)
    goal_track: Option<(&'static str, Vec2, u64, f64)>,
    drop_hold: Option<DropHold>,
    blacklist: Vec<(Vec2, f64)>,
}

impl BotBrain {
    pub fn new(hello: Hello) -> Self {
        let t0 = std::time::Instant::now();
        let map = ArenaMap::load(&hello.map);
        let nav = Navigator::new(&map);
        let initial_mechs = hello.config.r#match.get("mechs").copied().unwrap_or(16.0) as u64;
        let world = WorldState::new(initial_mechs);
        eprintln!(
            "vanguard-v4 initialized (slot={}, map_loaded={}, prisms={}, roofs={}, nav={}x{}) in {:.1}ms",
            hello.slot,
            map.loaded,
            map.prisms.len(),
            map.roof_anchors.len(),
            map.nav_nx,
            map.nav_ny,
            t0.elapsed().as_secs_f64() * 1000.0
        );
        Self {
            cfg: hello.config,
            map,
            nav,
            world,
            prev_wanted_move: false,
            regen_mode: false,
            cached_cover: None,
            climb_target: None,
            roam_anchor: None,
            roam_tick: 0,
            goal_track: None,
            drop_hold: None,
            blacklist: Vec::new(),
        }
    }

    fn is_blacklisted(&self, pt: Vec2, now: f64) -> bool {
        self.blacklist
            .iter()
            .any(|(bp, exp)| now < *exp && dist2d(*bp, pt) < 11.0)
    }

    // Returns true while intentionally holding inside a drop ring.
    fn track_goal_progress(
        &mut self,
        label: &'static str,
        goal: Vec2,
        obs: &Observation,
    ) -> bool {
        let pos2 = [obs.self_obs.pos[0], obs.self_obs.pos[1]];
        let tick = obs.tick;
        let now = obs.t;
        let trackable = matches!(
            label,
            "CUBE" | "KILL_LOOT" | "LOOT_WPN" | "SEEK_CRATE" | "ROOF_APPROACH" | "ROOF_CLIMB" | "REGEN_ROOF" | "DROP"
        );
        if !trackable {
            self.goal_track = None;
            self.drop_hold = None;
            return false;
        }

        // Reaching a drop's ring is progress. Waiting for it to land or for our
        // capture clock is not failed travel, even if we never reach its center.
        let arrived_drop = obs.drops.iter().find(|drop| {
            label == "DROP"
                && dist2d(goal, [drop.pos[0], drop.pos[1]]) < 1.0
                && dist2d(pos2, [drop.pos[0], drop.pos[1]]) <= drop.radius
                && ((obs.self_obs.pos[2] - drop.pos[2]).abs() < 4.0
                    || drop.holder == Some(obs.self_obs.id))
        });
        if let Some(drop) = arrived_drop {
            self.goal_track = None;
            if self.drop_hold.as_ref().is_none_or(|hold| hold.id != drop.id) {
                self.drop_hold = Some(DropHold {
                    id: drop.id,
                    lands_at: now + drop.lands_in.max(0.0),
                    previous_progress: 0.0,
                    last_progress_t: now,
                });
            }
            let hold = self.drop_hold.as_mut().unwrap();
            if drop.lands_in > 0.0 && now <= hold.lands_at + 1.0 {
                hold.last_progress_t = now;
                return true;
            }
            if drop.holder == Some(obs.self_obs.id)
                && drop.progress > hold.previous_progress + 1e-6
            {
                hold.last_progress_t = now;
            }
            hold.previous_progress = drop.progress;
            // Renew the inactivity clock for a progressing recapture, even after
            // a near-complete attempt resets. A separate absolute deadline bounds
            // repeated contest/reset cycles without abandoning normal recapture.
            let capture_s = drop.capture_s.max(0.0);
            if now - hold.last_progress_t > capture_s + 1.0
                || now > hold.lands_at + 3.0 * capture_s + 2.0
            {
                self.blacklist.push((goal, now + 28.0));
                self.drop_hold = None;
            }
            return true;
        }
        self.drop_hold = None;
        let cur_d = dist2d(pos2, goal);
        let timeout_ticks: u64 = if label == "DROP" { 85 } else { 36 };
        if let Some((prev_lbl, prev_pt, start_tick, best_d)) = self.goal_track {
            if prev_lbl == label && dist2d(prev_pt, goal) < 6.0 {
                if cur_d < best_d - 4.0 {
                    // Making real progress!
                    self.goal_track = Some((label, goal, tick, cur_d));
                } else if tick.saturating_sub(start_tick) >= timeout_ticks {
                    // Stalled on unreachable rooftop/blocked target -> blacklist for 28s!
                    self.blacklist.push((goal, now + 28.0));
                    self.goal_track = None;
                    if label == "ROOF_APPROACH" || label == "ROOF_CLIMB" {
                        self.climb_target = None;
                    }
                }
                return false;
            }
        }
        self.goal_track = Some((label, goal, tick, cur_d));
        false
    }

    pub fn act(&mut self, obs: &Observation) -> Action {
        self.world.update(obs, &self.cfg);
        self.blacklist.retain(|(_, exp)| obs.t < *exp);

        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let hp_frac = me.hp / me.max_hp.max(1.0);
        let walk_speed = me.stat("walk_speed", 11.7);
        let on_roof = me.grounded && self.map.is_on_roof(pos);

        let mut dbg = DebugDraw::default();
        let mut act = Action {
            tick: obs.tick,
            ..Default::default()
        };

        // 1. Upgrade offer handling
        if let Some(offer) = &obs.offer {
            if !offer.choices.is_empty() {
                act.pick = Some(choose_upgrade(offer, obs));
            }
        }

        // 2. Immediate weapon pickup / smart swap if standing on one
        let (take_id, drop_slot) = choose_immediate_pickup(obs, &self.cfg);
        act.take = take_id;
        act.drop = drop_slot;

        // 3. Zone safety & urgency
        let z = &obs.zone;
        let d_cur = dist2d(pos2, z.center);
        let d_next = dist2d(pos2, z.next_center);
        let safe_next_r = (z.next_radius - 12.0).max(4.0);
        let dist_to_next_safe = (d_next - safe_next_r).max(0.0);
        let time_to_shrink_end = (z.shrink_end - obs.t).max(0.0);
        let est_travel_s = dist_to_next_safe / (walk_speed * 0.68).max(4.0);

        let urgent_zone = !me.in_zone
            || (obs.t >= z.shrink_start - 4.0 && d_cur > (z.radius - 26.0).max(6.0))
            || (dist_to_next_safe > 0.0 && est_travel_s + 11.0 >= time_to_shrink_end);

        // Active safe circle for tactical cover/peeking (uses current circle during hold phases!)
        let (safe_c, safe_r) = if obs.t < z.shrink_start - 10.0 {
            (z.center, (z.radius - 8.0).max(10.0))
        } else {
            (z.center, (z.radius - 20.0).max(z.next_radius))
        };

        // 4. Assess visible enemies, threats, and ENEMY LASER CHARGE danger!
        let my_eye = [pos[0], pos[1], pos[2] + me.stat("eye_height", 8.0)];
        let lightning_range = me
            .weapons
            .iter()
            .flatten()
            .find(|w| w.kind == WeaponKind::Lightning)
            .map(|w| w.stat("range", 45.0))
            .unwrap_or(0.0);

        let mut vis_enemy_count = 0usize;
        let mut los_enemies_close = 0usize;
        let mut any_los_aiming = false;
        let mut lowest_vis_enemy_hp = f64::INFINITY;
        let mut enemy_laser_on_cd = false;
        let mut charging_los_snipers: Vec<Vec3> = Vec::new();
        let mut all_vis_threat_eyes: Vec<Vec3> = Vec::new();

        for e in self.world.visible_enemies() {
            vis_enemy_count += 1;
            let ed = dist2d(pos2, [e.pos[0], e.pos[1]]);
            let e_eye = [e.pos[0], e.pos[1], e.pos[2] + 7.5];
            all_vis_threat_eyes.push(e_eye);
            let has_los_to_e = self.map.has_los(my_eye, e_eye);
            if has_los_to_e && ed < 145.0 {
                los_enemies_close += 1;
                lowest_vis_enemy_hp = lowest_vis_enemy_hp.min(e.hp);
            }
            if has_los_to_e && (e.aiming_at_me || obs.t - e.last_damaged_me_t < 3.0) {
                any_los_aiming = true;
            }
            if e.laser_cd_until > obs.t + 0.3 {
                enemy_laser_on_cd = true;
            }
            if e.charging && has_los_to_e && ed < 275.0 {
                let can_short =
                    lightning_range > 0.0 && ed <= lightning_range + 2.0 && me.energy >= 6.0;
                if !can_short {
                    charging_los_snipers.push(e_eye);
                }
            }
        }

        if all_vis_threat_eyes.is_empty() && obs.t - self.world.last_damaged_t < 4.5 {
            if let Some(b) = self.world.last_damaged_bearing {
                all_vis_threat_eyes.push([
                    pos[0] + b.cos() * 65.0,
                    pos[1] + b.sin() * 65.0,
                    pos[2] + 7.5,
                ]);
            }
        }

        // Keep recently-seen threats (up to 16 s!) for REGEN_COVER so vision.memory_s (5.0 s) expiring
        // at the EXACT same second regen_delay (5.0 s) finishes NEVER kicks us out of cover at 25% HP!
        let mut regen_threat_eyes = all_vis_threat_eyes.clone();
        if regen_threat_eyes.is_empty() {
            for e in self.world.enemies.values() {
                if !e.visible_now && obs.t - e.last_seen_t <= 16.0 {
                    regen_threat_eyes.push([e.pos[0], e.pos[1], e.pos[2] + 7.5]);
                }
            }
        }

        // EMERGENCY LASER-DUCK: enemy charging a 315-dmg hitscan laser at us!
        // An own charging laser is not proof of an imminent kill: the target,
        // actual damage, alignment and incoming shot timing would all need proof.
        let laser_duck_cover =
            if !urgent_zone && !charging_los_snipers.is_empty() {
                self.map
                    .find_cover_pos(pos, &charging_los_snipers, safe_c, safe_r, 19.0, false)
                    .filter(|c| {
                        let ce = [c[0], c[1], c[2] + 7.5];
                        charging_los_snipers.iter().all(|&s| !self.map.has_los(ce, s))
                    })
            } else {
                None
            };
        let doing_laser_duck = laser_duck_cover.is_some();

        let active_threats = self.world.active_threat_count(pos, obs.t);
        let has_nanites = me.upgrades.iter().any(|u| u == "nanites");
        let regen_eta = self.world.time_until_regen(obs);

        // KILL-FINISH OVERRIDE: only stay in 1v1 when enemy is truly low HP or we have a decisive HP lead!
        // Never trade down to 150 HP in Practice where Hunter #2/#3 will immediately finish us!
        let finish_hp = if me.has_weapon(WeaponKind::Laser) {
            420.0
        } else {
            340.0
        };
        let kill_finish_override = active_threats <= 1
            && los_enemies_close == 1
            && ((lowest_vis_enemy_hp <= finish_hp && hp_frac > 0.18)
                || (me.hp > lowest_vis_enemy_hp + 200.0 && hp_frac > 0.34));

        // Hysteresis on cover-regen mode: preserve HP so we arrive at Hunter #2/#3 healthy!
        let enter_regen = !urgent_zone
            && !kill_finish_override
            && me.in_zone
            && !all_vis_threat_eyes.is_empty()
            && ((hp_frac < 0.48 && active_threats >= 1)
                || (hp_frac < 0.62 && active_threats >= 2)
                || (hp_frac < 0.68 && has_nanites && regen_eta <= 1.5 && active_threats >= 1)
                || (hp_frac < 0.48 && obs.t - self.world.last_damaged_t < 2.2));
        let exit_hp = if has_nanites { 0.88 } else { 0.82 };
        if enter_regen {
            self.regen_mode = true;
        } else if urgent_zone
            || kill_finish_override
            || !me.in_zone
            || hp_frac >= exit_hp
            || (regen_threat_eyes.is_empty() && obs.t - self.world.last_damaged_t >= 16.0)
        {
            self.regen_mode = false;
        }
        let want_regen_retreat = self.regen_mode;
        let hold_fire_for_regen =
            (self.regen_mode || (hp_frac < 0.72 && regen_eta <= 1.8)) && !any_los_aiming;

        // 5. Should we farm cubes?
        let want_cubes = me.cubes < 10
            || self
                .cfg
                .cubes
                .upgrade_thresholds
                .iter()
                .any(|&th| th as u64 > me.cubes && (th as u64) - me.cubes <= 2);

        // 6. Combat target & laser peek pair check
        // Protect out-of-combat regen: don't shoot crates/distant NPCs while hurt (< 80% HP), let regen top off!
        let protect_regen_from_farm = me.in_zone && hp_frac < if has_nanites { 0.86 } else { 0.78 };
        let allow_farm = !urgent_zone
            && !want_regen_retreat
            && !doing_laser_duck
            && !protect_regen_from_farm
            && (want_cubes || me.empty_slot().is_some() || !me.has_weapon(WeaponKind::Laser));
        let target = select_target(
            obs,
            &self.cfg,
            &self.map,
            &self.world,
            allow_farm,
            hold_fire_for_regen,
        );

        let peek_pair = if !urgent_zone
            && !want_regen_retreat
            && !doing_laser_duck
            && me.has_weapon(WeaponKind::Laser)
        {
            target.as_ref().and_then(|t| {
                if t.is_mech && dist2d(pos2, [t.pos[0], t.pos[1]]) > 38.0 {
                    self.map.find_peek_pair(pos, t.pos, safe_c, safe_r)
                } else {
                    None
                }
            })
        } else {
            None
        };

        let allow_precharge_behind_cover =
            peek_pair.is_some_and(|(hide, _)| dist2d(pos2, [hide[0], hide[1]]) < 8.5);
        // If enemy just fired their laser (on 1.65s cooldown!), it's safe to charge our laser aggressively!
        let avoid_open_laser_slowdown =
            doing_laser_duck || (los_enemies_close >= 2 && hp_frac < 0.78 && !enemy_laser_on_cd);

        let combat = evaluate_combat(
            obs,
            &self.cfg,
            &self.map,
            &self.world,
            target.as_ref(),
            // Regen mode can still be exposed or stationary: do not turn it
            // into a blanket laser firing ban. Actual zone/duck escapes remain urgent.
            urgent_zone || doing_laser_duck,
            hold_fire_for_regen,
            allow_precharge_behind_cover,
            avoid_open_laser_slowdown,
        );

        act.fire = Some(combat.fire);
        act.reload = Some(combat.reload);

        // 7. Determine Macro Movement Goal
        let (goal_pos3, state_label, want_boost_macro, stay_on_roof) = self.choose_macro_goal(
            obs,
            urgent_zone,
            laser_duck_cover,
            want_regen_retreat,
            want_cubes,
            los_enemies_close,
            vis_enemy_count,
            target.as_ref(),
            &combat,
            peek_pair,
            &all_vis_threat_eyes,
            &regen_threat_eyes,
            safe_c,
            safe_r,
        );

        let goal2 = [goal_pos3[0], goal_pos3[1]];
        let dist_to_goal = dist2d(pos2, goal2);

        // Track goal progress & auto-blacklist unreachable rooftop cubes/drops!
        let holding_drop = self.track_goal_progress(state_label, goal2, obs);

        // Update stuck detector before pathing: ONLY enable 2.4s oscillation loop detection during pure travel states
        // (NEVER during combat strafing, corner-peeking, or holding cover!).
        let is_travel_state = matches!(
            state_label,
            "ZONE" | "PATROL" | "LOOT_WPN" | "CUBE" | "KILL_LOOT" | "DROP" | "SEEK_CRATE" | "ROOF_APPROACH"
        );
        let stuck = self.nav.update_stuck(
            &self.map,
            obs.tick,
            pos,
            self.prev_wanted_move && !holding_drop,
            is_travel_state && !holding_drop,
        );
        let stuck_n = self.nav.stuck_ticks();
        // When ON a rooftop, walk straight toward goal2 (either staying on roof OR dropping off the front edge toward zone!)
        let target_is_higher =
            goal_pos3[2] - pos[2] > 1.3 && dist_to_goal < 14.0 && state_label != "ZONE";
        let waypoint = if on_roof || (target_is_higher && dist_to_goal <= 6.5) {
            goal2
        } else {
            self.nav.next_waypoint(&self.map, obs.tick, pos, goal2)
        };

        // 8. 16-Direction Micro-Steering
        let (move_dir, dodge_boost, need_jump_or_jet) = self.compute_micro_move(
            obs,
            waypoint,
            goal_pos3,
            target.as_ref(),
            combat.desired_range,
            urgent_zone,
            want_regen_retreat || doing_laser_duck,
            stay_on_roof && on_roof,
            stuck,
            stuck_n,
        );

        let move_mag = (move_dir[0] * move_dir[0] + move_dir[1] * move_dir[1]).sqrt();
        // Track stuckness when wanting to travel (exclude stationary/cover/peek states!)
        let holding_cover = matches!(
            state_label,
            "ROOF_HOLD" | "REGEN_COVER" | "LASER_PRECHG" | "LASER_HIDE" | "LASER_PEEK"
        );
        self.prev_wanted_move = dist_to_goal > 4.0 && !holding_cover && !holding_drop;
        act.r#move = Some(move_dir);

        // 9. Jump & Boost / Jetpack decision
        let climb_jet = target_is_higher && me.energy > 14.0;
        let mut jump = false;
        let mut boost = false;

        if climb_jet || need_jump_or_jet {
            if me.grounded {
                jump = true;
            } else if me.energy > 8.0 {
                boost = true;
            }
        } else if stuck {
            // While stuck on ground, conserve energy to jet over walls instead of draining on ground boost!
            boost = false;
        } else if me.grounded {
            let reserve = if me.has_weapon(WeaponKind::Lightning) {
                24.0
            } else {
                16.0
            };
            if (dodge_boost || doing_laser_duck || urgent_zone) && me.energy > 6.0 {
                boost = true;
            } else if want_boost_macro && me.energy > reserve && move_mag > 0.5 {
                boost = true;
            } else if !combat.charging_laser
                && target.as_ref().is_some_and(|t| t.is_mech)
                && me.energy > reserve + 14.0
                && move_mag > 0.6
            {
                boost = true;
            }
        }
        act.jump = Some(jump);
        act.boost = Some(boost);

        // 10. Torso Aim Discipline (Strict Rear-Armor Protection > Combat > Threat Scan)
        let desired_aim = self.choose_torso_aim(obs, &combat, move_dir);
        self.world.last_aim = desired_aim;
        act.aim = Some(desired_aim);

        // 11. Compact Debug Drawing
        dbg.set_label(format!(
            "{}{} | hp:{:.0} c:{} e:{}",
            state_label,
            if on_roof { "^" } else { "" },
            me.hp,
            me.cubes,
            vis_enemy_count
        ));
        dbg.point([goal2[0], goal2[1], goal_pos3[2] + 1.0], "#40c0ff", 5.0);
        if let Some(tp) = combat.target_pos {
            dbg.line(muzzle_pos(me), tp, "#ff5040");
        }
        for wp in self.nav.remaining_path().iter().take(4) {
            dbg.point([wp[0], wp[1], pos[2] + 1.0], "#60ff80", 3.0);
        }
        act.debug = Some(dbg);

        let min_pitch = me.stat("min_pitch", -0.7);
        let max_pitch = me.stat("max_pitch", 1.3);
        act.sanitize(min_pitch, max_pitch, &self.cfg.debug);
        act
    }

    #[allow(clippy::too_many_arguments)]
    fn choose_macro_goal(
        &mut self,
        obs: &Observation,
        urgent_zone: bool,
        laser_duck_cover: Option<Vec3>,
        want_regen_retreat: bool,
        want_cubes: bool,
        los_enemies_close: usize,
        vis_enemy_count: usize,
        target: Option<&CombatTarget>,
        combat: &crate::combat::CombatDecision,
        peek_pair: Option<(Vec3, Vec3)>,
        all_vis_threat_eyes: &[Vec3],
        regen_threat_eyes: &[Vec3],
        safe_c: Vec2,
        safe_r: f64,
    ) -> (Vec3, &'static str, bool, bool) {
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let z = &obs.zone;
        let hp_frac = me.hp / me.max_hp.max(1.0);
        let on_roof = me.grounded && self.map.is_on_roof(pos);
        let under_direct_fire = los_enemies_close >= 1 || obs.t - self.world.last_damaged_t < 1.5;
        let threats = self.world.active_threat_count(pos, obs.t);

        // Priority 1: Zone urgency — ALWAYS snap goal to a real walkable cell inside next_center!
        if urgent_zone {
            self.climb_target = None;
            let zpt = if let Some((gx, gy)) =
                self.map
                    .nearest_walkable_cell(z.next_center[0], z.next_center[1], 25)
            {
                self.map.nav_to_world(gx, gy)
            } else {
                z.next_center
            };
            let gz = self.map.terrain_z(zpt[0] as f32, zpt[1] as f32) as f64;
            return ([zpt[0], zpt[1], gz], "ZONE", true, false);
        }

        // Priority 2: EMERGENCY LASER-DUCK behind cover before 315-dmg beam fires!
        if let Some(cov) = laser_duck_cover {
            let stay = on_roof && (cov[2] - pos[2]).abs() < 1.8;
            return (cov, "LASER_DUCK", true, stay);
        }

        // Priority 3: Retreat to hard cover & STAY HIDDEN until genuinely healed to 82-88% HP!
        if want_regen_retreat {
            self.climb_target = None;
            let cov_threats = if !all_vis_threat_eyes.is_empty() {
                all_vis_threat_eyes
            } else {
                regen_threat_eyes
            };
            if cov_threats.is_empty() {
                // Already safe & hidden while regen ticks at 45-75 HP/s — sit silent until healed!
                return (pos, "REGEN_COVER", false, on_roof);
            }
            let cover = self.get_or_update_cover(obs, cov_threats, safe_c, safe_r, 44.0);
            if let Some(cov) = cover {
                let stay = on_roof && (cov[2] - pos[2]).abs() < 1.8;
                let need_sprint = dist2d(pos2, [cov[0], cov[1]]) > 5.0 && !all_vis_threat_eyes.is_empty();
                return (cov, "REGEN_COVER", need_sprint, stay);
            }
            if let Some(th) = cov_threats.first() {
                let away_x = pos[0] - th[0];
                let away_y = pos[1] - th[1];
                let len = (away_x * away_x + away_y * away_y).sqrt().max(1.0);
                let gx = pos[0] + (away_x / len) * 35.0;
                let gy = pos[1] + (away_y / len) * 35.0;
                if dist2d([gx, gy], safe_c) < safe_r {
                    return ([gx, gy, pos[2]], "REGEN_KITE", !all_vis_threat_eyes.is_empty(), false);
                }
            }
            return ([z.next_center[0], z.next_center[1], pos[2]], "REGEN_ZONE", true, false);
        }

        // Priority 4a: Instant loot RIGHT AT OUR FEET (< 14 m) — scoop without detouring!
        if want_cubes {
            let instant_cube = self
                .world
                .cubes
                .values()
                .filter(|c| {
                    let c2 = [c.pos[0], c.pos[1]];
                    dist2d(pos2, c2) < 14.0
                        && (c.pos[2] - pos[2]).abs() < 3.0
                        && dist2d(c2, safe_c) < safe_r
                        && !self.is_blacklisted(c2, obs.t)
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.pos[0], a.pos[1]])
                        .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(c) = instant_cube {
                return (c.pos, "CUBE", false, on_roof && (c.pos[2] - pos[2]).abs() < 2.0);
            }
        }
        let immediate_wpn_r = if (me.empty_slot().is_some() || !me.has_weapon(WeaponKind::Laser))
            && hp_frac > 0.58
        {
            46.0
        } else {
            18.0
        };
        if let Some((p_pos, _)) =
            best_nearby_pickup(obs, &self.world, immediate_wpn_r, safe_c, safe_r)
        {
            let p2 = [p_pos[0], p_pos[1]];
            if (p_pos[2] - pos[2]).abs() < 3.5 && !self.is_blacklisted(p2, obs.t) {
                return (
                    p_pos,
                    "LOOT_WPN",
                    me.empty_slot().is_some() || !me.has_weapon(WeaponKind::Laser),
                    on_roof && (p_pos[2] - pos[2]).abs() < 2.0,
                );
            }
        }

        // Priority 4b: Outnumbered (>= 2 threats) ALWAYS takes cover/isolates BEFORE chasing distant pickups!
        if threats >= 2 {
            if let Some(t) = target.filter(|t| t.is_mech) {
                let can_slice = threats == 2 && hp_frac > 0.65 && me.empty_slot().is_none();
                if can_slice {
                    let others: Vec<Vec3> = all_vis_threat_eyes
                        .iter()
                        .copied()
                        .filter(|&e| dist2d([e[0], e[1]], [t.pos[0], t.pos[1]]) > 8.0)
                        .collect();
                    if let Some(pie_pos) = self.map.find_slice_pie_pos(
                        pos,
                        t.pos,
                        &others,
                        safe_c,
                        safe_r,
                        combat.desired_range,
                    ) {
                        let stay = on_roof && (pie_pos[2] - pos[2]).abs() < 1.8;
                        return (pie_pos, "SLICE_PIE", true, stay);
                    }
                }
            }
            if let Some(cov) =
                self.get_or_update_cover(obs, all_vis_threat_eyes, safe_c, safe_r, 44.0)
            {
                let stay = on_roof && (cov[2] - pos[2]).abs() < 1.8;
                return (cov, "KITE_COVER", true, stay);
            }
        }

        // Priority 4c: Safe medium-range weapon pickups & cubes (enemy-danger-filtered!)
        let holding_mortar = me.has_weapon(WeaponKind::Mortar);
        let need_laser = !me.has_weapon(WeaponKind::Laser);
        let pickup_search_r = if under_direct_fire {
            if me.empty_slot().is_some() && hp_frac > 0.62 {
                52.0
            } else if holding_mortar && hp_frac > 0.68 {
                40.0
            } else {
                20.0
            }
        } else if me.empty_slot().is_some() || holding_mortar {
            165.0
        } else if need_laser {
            125.0
        } else {
            65.0
        };
        if let Some((p_pos, _)) =
            best_nearby_pickup(obs, &self.world, pickup_search_r, safe_c, safe_r)
        {
            let p2 = [p_pos[0], p_pos[1]];
            let dz = p_pos[2] - pos[2];
            let same_level = dz.abs() < 3.5
                || (!under_direct_fire && dz > 0.0 && dz < 22.0 && (me.energy >= 52.0 || !me.grounded));
            if same_level
                && !self.is_blacklisted(p2, obs.t)
                && (hp_frac > 0.55 || !under_direct_fire)
            {
                let sprint = (me.empty_slot().is_some() || holding_mortar)
                    && (obs.t < 20.0 || me.energy > 45.0)
                    && dz.abs() < 3.5;
                return (
                    p_pos,
                    "LOOT_WPN",
                    sprint,
                    on_roof && (p_pos[2] - pos[2]).abs() < 2.0,
                );
            }
        }

        if want_cubes && !under_direct_fire {
            let nearest_cube = self
                .world
                .cubes
                .values()
                .filter(|c| {
                    let c2 = [c.pos[0], c.pos[1]];
                    dist2d(pos2, c2) < 75.0
                        && (c.pos[2] - pos[2]).abs() < 3.5
                        && dist2d(c2, safe_c) < safe_r
                        && !self.is_blacklisted(c2, obs.t)
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.pos[0], a.pos[1]])
                        .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(c) = nearest_cube {
                return (
                    c.pos,
                    "CUBE",
                    false, // Walk quietly to cubes to save energy & stay silent!
                    on_roof && (c.pos[2] - pos[2]).abs() < 2.0,
                );
            }
        }

        // Priority 5: Active 1v1 Combat (Laser Corner-Peek + Rooftop Fortress + Responsive Strafing!)
        if let Some(t) = target {
            if t.is_mech {
                let d = dist2d(pos2, [t.pos[0], t.pos[1]]);
                let practice = self.world.looks_like_practice_squad(obs.t);

                // 5a. Laser Corner-Peek & Pre-Charge Loop!
                if let Some((hide_pos, peek_pos)) = peek_pair {
                    let stay_h = on_roof && (hide_pos[2] - pos[2]).abs() < 1.8;
                    let stay_p = on_roof && (peek_pos[2] - pos[2]).abs() < 1.8;
                    if combat.laser_charge_s >= 0.30 {
                        return (peek_pos, "LASER_PEEK", false, stay_p);
                    } else if combat.laser_cooldown_s > 0.35 && !me.has_weapon(WeaponKind::Gun) {
                        return (
                            hide_pos,
                            "LASER_HIDE",
                            dist2d(pos2, [hide_pos[0], hide_pos[1]]) > 4.0,
                            stay_h,
                        );
                    } else if combat.laser_charge_s > 0.0 || combat.laser_cooldown_s <= 0.25 {
                        return (hide_pos, "LASER_PRECHG", false, stay_h);
                    }
                }

                // 5b. Rooftop Fortress Combat: weave laterally ON the roof (never stand like a statue!)
                if on_roof && dist2d(pos2, safe_c) < safe_r {
                    let dx = t.pos[0] - pos[0];
                    let dy = t.pos[1] - pos[1];
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    if combat.has_los_to_target {
                        let strafe = if ((obs.tick + me.id * 3) / 10) % 2 == 0 {
                            1.0
                        } else {
                            -1.0
                        };
                        let px = -dy / len * strafe * 10.0;
                        let py = dx / len * strafe * 10.0;
                        return ([pos[0] + px, pos[1] + py, pos[2]], "ROOF_FIGHT", false, true);
                    } else {
                        return (
                            [pos[0] + dx / len * 6.0, pos[1] + dy / len * 6.0, pos[2]],
                            "ROOF_PEEK",
                            false,
                            true,
                        );
                    }
                }

                // Only chase occluded enemies around corners to finish a kill (<450 HP) OR when armed & healthy!
                // Never blind-flank full-HP enemies with gun-only into multi-bot squads!
                if !combat.has_los_to_target
                    && d < 120.0
                    && (t.hp < 450.0
                        || (practice && me.empty_slot().is_none() && hp_frac > 0.72 && d < 95.0))
                {
                    return (t.pos, "FLANK", false, false);
                }

                // Responsive lateral strafing (flips every 1.0s — fast enough to juke linear leads!)
                let dx = pos[0] - t.pos[0];
                let dy = pos[1] - t.pos[1];
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let ux = dx / len;
                let uy = dy / len;
                let strafe_sign = if ((obs.tick + me.id * 3) / 10) % 2 == 0 {
                    1.0
                } else {
                    -1.0
                };
                let tx = -uy * strafe_sign;
                let ty = ux * strafe_sign;

                let goal_r = combat.desired_range.clamp(24.0, 120.0);
                let mut gx = t.pos[0] + ux * goal_r + tx * 24.0;
                let mut gy = t.pos[1] + uy * goal_r + ty * 24.0;
                if dist2d([gx, gy], safe_c) > safe_r {
                    gx = z.next_center[0];
                    gy = z.next_center[1];
                }
                return ([gx, gy, pos[2]], "COMBAT", d > goal_r + 22.0, on_roof);
            }
        }

        // Priority 6: Collect Kill-Loot Drops (cubes + guaranteed dropped secondary weapons!)
        if want_cubes || !me.has_weapon(WeaponKind::Laser) {
            let nearest_kill_drop = self
                .world
                .kill_drops
                .iter()
                .filter(|(kp, _)| {
                    let k2 = [kp[0], kp[1]];
                    dist2d(pos2, k2) < 95.0
                        && (kp[2] - pos[2]).abs() < 4.0
                        && dist2d(k2, safe_c) < safe_r
                        && !self.is_blacklisted(k2, obs.t)
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.0[0], a.0[1]])
                        .partial_cmp(&dist2d(pos2, [b.0[0], b.0[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some((kp, _)) = nearest_kill_drop {
                return (*kp, "KILL_LOOT", vis_enemy_count > 0, false);
            }
        }

        // Priority 7: Supply Drops (arrive right as it lands: lands_in <= 12s -> 4s capture!)
        let best_drop = obs
            .drops
            .iter()
            .filter(|d| {
                let d2 = [d.pos[0], d.pos[1]];
                let dd = dist2d(pos2, d2);
                let in_safe = dist2d(d2, z.next_center) <= z.next_radius + 8.0;
                in_safe
                    && d.lands_in <= 12.0
                    && !self.is_blacklisted(d2, obs.t)
                    && (dd < 135.0 || (hp_frac < 0.70 && dd < 165.0))
            })
            .min_by(|a, b| {
                dist2d(pos2, [a.pos[0], a.pos[1]])
                    .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(drop) = best_drop {
            let d_drop = dist2d(pos2, [drop.pos[0], drop.pos[1]]);
            return (drop.pos, "DROP", d_drop > drop.radius, false);
        }

        // Priority 8: Farm nearby NPCs / crates when < 10 cubes, empty slot, OR still hunting for a Laser!
        let need_crate_wpn = !me.has_weapon(WeaponKind::Laser);
        let farm_ok = !me.in_zone
            || hp_frac
                >= if me.upgrades.iter().any(|u| u == "nanites") {
                    0.86
                } else if need_crate_wpn {
                    0.74
                } else {
                    0.78
                };
        if (want_cubes || me.empty_slot().is_some() || need_crate_wpn) && farm_ok {
            if let Some(t) = target {
                if !t.is_mech {
                    let dx = pos[0] - t.pos[0];
                    let dy = pos[1] - t.pos[1];
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    let strafe = if (obs.tick / 12) % 2 == 0 { 1.0 } else { -1.0 };
                    let gx = t.pos[0] + (dx / len) * 24.0 + (-dy / len) * strafe * 14.0;
                    let gy = t.pos[1] + (dy / len) * 24.0 + (dx / len) * strafe * 14.0;
                    return ([gx, gy, pos[2]], "FARM_FIGHT", false, on_roof);
                }
            }
            let crate_r = if need_crate_wpn { 130.0 } else { 95.0 };
            let best_crate = self
                .world
                .crates
                .values()
                .filter(|c| {
                    let c2 = [c.pos[0], c.pos[1]];
                    dist2d(pos2, c2) < crate_r
                        && (c.pos[2] - pos[2]).abs() < 3.5
                        && dist2d(c2, safe_c) < safe_r
                        && !self.is_blacklisted(c2, obs.t)
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.pos[0], a.pos[1]])
                        .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(cr) = best_crate {
                return (cr.pos, "SEEK_CRATE", false, false);
            }
        }

        // Priority 9: Seize Rooftop High Ground when armed with Laser (huge 1v15 multiplier!) or peaceful!
        let finishing_climb = !on_roof && !me.grounded && self.climb_target.is_some() && me.energy >= 8.0;
        if !on_roof
            && vis_enemy_count == 0
            && obs.t - self.world.last_damaged_t > 3.5
            && me.has_weapon(WeaponKind::Laser)
            && (me.energy >= 68.0 || finishing_climb)
            && z.next_radius >= 35.0
        {
            let active_climb = self.climb_target.and_then(|(ctick, anchor)| {
                let r2 = [anchor.roof_pos[0], anchor.roof_pos[1]];
                if obs.tick.saturating_sub(ctick) < 70
                    && !self.is_blacklisted(r2, obs.t)
                    && dist2d(r2, z.next_center) <= (z.next_radius - 10.0).max(8.0)
                {
                    Some(anchor)
                } else {
                    None
                }
            });
            let anchor = active_climb.or_else(|| {
                let found = self
                    .map
                    .find_nearby_rooftop(pos, z.next_center, z.next_radius, 55.0, 22.0)
                    .filter(|a| !self.is_blacklisted([a.roof_pos[0], a.roof_pos[1]], obs.t));
                if let Some(a) = found {
                    self.climb_target = Some((obs.tick, a));
                }
                found
            });
            if let Some(a) = anchor {
                let d_launch = dist2d(pos2, [a.launch_pos[0], a.launch_pos[1]]);
                let need_e = (a.climb_dz * 2.1 + 18.0).min(85.0);
                if d_launch > 6.5 && me.grounded {
                    return (a.launch_pos, "ROOF_APPROACH", false, false);
                } else if me.energy >= need_e || finishing_climb {
                    return (a.roof_pos, "ROOF_CLIMB", false, false);
                } else if me.grounded && me.energy < 20.0 {
                    // Failed climb -> blacklist this anchor for 20s instead of pogo-sticking!
                    self.blacklist
                        .push(([a.roof_pos[0], a.roof_pos[1]], obs.t + 20.0));
                    self.climb_target = None;
                }
            }
        } else if on_roof
            && dist2d(pos2, z.next_center) < (z.next_radius - 10.0).max(8.0)
            && (me.has_weapon(WeaponKind::Laser) || !self.world.looks_like_practice_squad(obs.t))
        {
            self.climb_target = None;
            return (pos, "ROOF_HOLD", false, true);
        }

        // Priority 10: Safe Perimeter / Edge Patrol (avoid walking into center of 15-bot hive!)
        let need_new_anchor = self.roam_anchor.is_none_or(|a| {
            dist2d(pos2, a) < 16.0
                || dist2d(a, z.next_center) > (z.next_radius * 0.80).max(10.0)
                || obs.tick.saturating_sub(self.roam_tick) > 100
        });
        if need_new_anchor {
            // Stay near our side of the safe perimeter (~62% radius) so nobody flanks behind us!
            let cur_ang = (pos[1] - z.next_center[1]).atan2(pos[0] - z.next_center[0]);
            let step_ang = cur_ang + 0.55;
            let r = (z.next_radius * 0.62).clamp(10.0, 180.0);
            let ax = z.next_center[0] + step_ang.cos() * r;
            let ay = z.next_center[1] + step_ang.sin() * r;
            let anchor = if let Some((gx, gy)) = self.map.nearest_walkable_cell(ax, ay, 18) {
                self.map.nav_to_world(gx, gy)
            } else if let Some((gx, gy)) =
                self.map
                    .nearest_walkable_cell(z.next_center[0], z.next_center[1], 20)
            {
                self.map.nav_to_world(gx, gy)
            } else {
                z.next_center
            };
            self.roam_anchor = Some(anchor);
            self.roam_tick = obs.tick;
        }

        let anchor = self.roam_anchor.unwrap_or(z.next_center);
        let az = self.map.terrain_z(anchor[0] as f32, anchor[1] as f32) as f64;
        (
            [anchor[0], anchor[1], az],
            "PATROL",
            dist2d(pos2, z.next_center) > z.next_radius,
            false,
        )
    }

    fn get_or_update_cover(
        &mut self,
        obs: &Observation,
        threats: &[Vec3],
        safe_c: Vec2,
        safe_r: f64,
        max_r: f64,
    ) -> Option<Vec3> {
        if threats.is_empty() {
            return None;
        }
        let pos = obs.self_obs.pos;
        if let Some((ctick, cpos)) = self.cached_cover {
            let d_to_c = dist2d([cpos[0], cpos[1]], [pos[0], pos[1]]);
            if obs.tick.saturating_sub(ctick) < 6
                && dist2d([cpos[0], cpos[1]], safe_c) < safe_r
                && d_to_c < max_r
            {
                let ceye = [cpos[0], cpos[1], cpos[2] + 7.5];
                if threats.iter().take(4).all(|&th| !self.map.has_los(ceye, th)) {
                    return Some(cpos);
                }
            }
        }
        let found = self
            .map
            .find_cover_pos(pos, threats, safe_c, safe_r, max_r, true);
        if let Some(c) = found {
            self.cached_cover = Some((obs.tick, c));
        }
        found
    }

    #[allow(clippy::too_many_arguments)]
    fn compute_micro_move(
        &self,
        obs: &Observation,
        waypoint: Vec2,
        goal_pos3: Vec3,
        target: Option<&CombatTarget>,
        desired_range: f64,
        urgent_zone: bool,
        defensive_cover_move: bool,
        stay_on_roof: bool,
        stuck: bool,
        stuck_n: u32,
    ) -> (Vec2, bool, bool) {
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let scale = 1.0 + self.cfg.cubes.size_per_cube * (me.cubes as f64);
        let my_r = me.stat("radius", 2.5) * scale;
        let walk_spd = me.stat("walk_speed", 11.7);
        let z = &obs.zone;

        let climbing_up = !urgent_zone
            && goal_pos3[2] - pos[2] > 1.3
            && dist2d(pos2, [goal_pos3[0], goal_pos3[1]]) < 14.0;
        if climbing_up {
            let dx = goal_pos3[0] - pos[0];
            let dy = goal_pos3[1] - pos[1];
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            // Rise cleanly beside the wall first, then push full speed onto the roof lip!
            let push = if pos[2] >= goal_pos3[2] - 0.3 { 1.0 } else { 0.25 };
            return ([dx / d * push, dy / d * push], false, true);
        }

        if stuck {
            // Escalating jetpack escape: quick hop at e>=28 for low walls; if still stuck >= 14 ticks
            // (trapped in a narrow courtyard at high cubes!), recharge to e>=68 and jet cleanly over 25m roofs!
            let need_e = if stuck_n >= 14 { 68.0 } else { 28.0 };
            let jet_escape = (stuck_n >= 6 && me.energy >= need_e) || (!me.grounded && me.energy >= 8.0);
            let dir = if jet_escape && !me.grounded && stuck_n >= 14 {
                // While high-jetting out of a courtyard, push directly toward the waypoint!
                let wdx = waypoint[0] - pos[0];
                let wdy = waypoint[1] - pos[1];
                let wd = (wdx * wdx + wdy * wdy).sqrt().max(1e-6);
                [wdx / wd, wdy / wd]
            } else {
                self.nav.unstuck_direction()
            };
            return (dir, false, jet_escape);
        }

        let wp_dx = waypoint[0] - pos[0];
        let wp_dy = waypoint[1] - pos[1];
        let wp_dist = (wp_dx * wp_dx + wp_dy * wp_dy).sqrt();
        let wp_dir = if wp_dist > 0.5 {
            [wp_dx / wp_dist, wp_dy / wp_dist]
        } else {
            [0.0, 0.0]
        };

        let mut laser_threats: Vec<(Vec2, Vec2)> = Vec::new();
        for e in self.world.visible_enemies() {
            if e.charging {
                let dir = [e.aim[0].cos(), e.aim[0].sin()];
                laser_threats.push(([e.pos[0], e.pos[1]], dir));
            }
        }

        let mortar_g = self.cfg.weapons.mortar.get("gravity").copied().unwrap_or(22.0);
        let mortar_splash = self
            .cfg
            .weapons
            .mortar
            .get("splash_radius")
            .copied()
            .unwrap_or(7.0);
        let mut urgent_dodge = false;

        for &(lo, ldir) in &laser_threats {
            let rx = pos[0] - lo[0];
            let ry = pos[1] - lo[1];
            let along = rx * ldir[0] + ry * ldir[1];
            if along > 0.0 && along < 280.0 {
                let perp = (rx * ldir[1] - ry * ldir[0]).abs();
                if perp < my_r + 4.5 {
                    urgent_dodge = true;
                }
            }
        }

        let mut best_score = f64::NEG_INFINITY;
        let mut best_dir = wp_dir;
        let step_dt = 0.35;
        let g_accel = me.stat("ground_accel", 58.5);
        let max_dv = g_accel * step_dt;
        let cur_spd = (me.vel[0] * me.vel[0] + me.vel[1] * me.vel[1]).sqrt();

        let mut lightning_close = false;
        for e in self.world.visible_enemies() {
            if e.has_weapon(WeaponKind::Lightning) {
                let ed = dist2d(pos2, [e.pos[0], e.pos[1]]);
                let win_duel = me.has_weapon(WeaponKind::Lightning) && me.hp > e.hp + 150.0;
                if !win_duel && ed < 48.0 {
                    lightning_close = true;
                    if ed < 45.0 {
                        urgent_dodge = true;
                    }
                }
            }
        }

        // Never stand still (ci == 0) when traveling or inside enemy Lightning danger!
        let must_move = wp_dist > 2.5 || lightning_close;
        let start_ci = if must_move { 1usize } else { 0usize };
        for ci in start_ci..=16usize {
            let dir = if ci == 0 {
                [0.0, 0.0]
            } else {
                let ang = ((ci - 1) as f64) * (std::f64::consts::TAU / 16.0);
                [ang.cos(), ang.sin()]
            };

            // Kinematic acceleration-aware prediction (respects inertia so we maintain high lateral speed!)
            let des_vx = dir[0] * walk_spd;
            let des_vy = dir[1] * walk_spd;
            let dvx = des_vx - me.vel[0];
            let dvy = des_vy - me.vel[1];
            let dv = (dvx * dvx + dvy * dvy).sqrt();
            let (evx, evy) = if dv > max_dv && dv > 1e-6 {
                (me.vel[0] + dvx / dv * max_dv, me.vel[1] + dvy / dv * max_dv)
            } else {
                (des_vx, des_vy)
            };
            let nx = pos[0] + 0.5 * (me.vel[0] + evx) * step_dt;
            let ny = pos[1] + 0.5 * (me.vel[1] + evy) * step_dt;
            let mut score = 0.0;

            // Light momentum smoothing (keeps bullet dodging super crisp without braking oscillation)
            if cur_spd > 2.5 && ci != 0 {
                let vdot = (dir[0] * me.vel[0] + dir[1] * me.vel[1]) / cur_spd;
                score += vdot * 1.5;
            }

            if wp_dist > 2.2 {
                let dot = dir[0] * wp_dir[0] + dir[1] * wp_dir[1];
                let w = if urgent_zone || defensive_cover_move {
                    32.0
                } else {
                    20.0
                };
                score += dot * w;
            } else if ci == 0 {
                score += if defensive_cover_move { 18.0 } else { 7.0 };
            }

            if self.map.loaded {
                if stay_on_roof {
                    let next_top = self.map.solid_top_at(nx as f32, ny as f32) as f64;
                    if next_top < pos[2] - 2.5 {
                        score -= 90.0;
                    }
                }
                // Tight 1.8 m collision lookahead so narrow alleys & doorways stay cleanly navigable!
                let check_x = pos[0] + dir[0] * 1.8;
                let check_y = pos[1] + dir[1] * 1.8;
                if self.map.is_blocked_at(check_x, check_y, pos[2], my_r * 0.72) {
                    score -= 22.0;
                }
                if !stay_on_roof {
                    let (gx, gy) = self.map.world_to_nav(nx, ny);
                    let wd = self.map.wall_dist_cell(gx, gy);
                    if wd == 0
                        && pos[2] <= self.map.terrain_z_fast(pos[0] as f32, pos[1] as f32) as f64 + 1.5
                    {
                        score -= 14.0;
                    }
                }
            }

            let nd_cur = dist2d([nx, ny], z.center);
            if nd_cur > z.radius - 4.0 {
                score -= (nd_cur - (z.radius - 4.0)) * 14.0;
            }
            if !me.in_zone {
                let d_before = dist2d(pos2, z.center);
                score += (d_before - nd_cur) * 8.0;
            }

            // Projectile dodging
            for p in &obs.projectiles {
                if p.kind == "mortar" {
                    let vz = p.vel[2];
                    let dz_ground = (p.pos[2] - pos[2]).max(0.0);
                    let disc = vz * vz + 2.0 * mortar_g * dz_ground;
                    if disc >= 0.0 {
                        let t_land = ((vz + disc.sqrt()) / mortar_g).clamp(0.05, 4.0);
                        if t_land < 1.8 {
                            let lx = p.pos[0] + p.vel[0] * t_land;
                            let ly = p.pos[1] + p.vel[1] * t_land;
                            let d_land = dist2d([nx, ny], [lx, ly]);
                            let danger_r = mortar_splash + my_r + 2.5;
                            if d_land < danger_r {
                                score -= (danger_r - d_land) * 30.0;
                                if dist2d(pos2, [lx, ly]) < danger_r {
                                    urgent_dodge = true;
                                }
                            }
                        }
                    }
                } else {
                    let rx = p.pos[0] - nx;
                    let ry = p.pos[1] - ny;
                    let vx = p.vel[0];
                    let vy = p.vel[1];
                    let v2 = vx * vx + vy * vy;
                    if v2 > 10.0 {
                        let t_closest = (-(rx * vx + ry * vy) / v2).clamp(0.0, 0.75);
                        if t_closest > 0.01 {
                            let cx = p.pos[0] + vx * t_closest - nx;
                            let cy = p.pos[1] + vy * t_closest - ny;
                            let cz =
                                (p.pos[2] + p.vel[2] * t_closest) - (pos[2] + 4.5 * scale);
                            if cz.abs() < 5.5 * scale {
                                let d_perp = (cx * cx + cy * cy).sqrt();
                                let safe_r = my_r + 2.4;
                                if d_perp < safe_r {
                                    score -= (safe_r - d_perp) * 28.0;
                                    if t_closest < 0.35 && d_perp < my_r + 1.2 {
                                        urgent_dodge = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for &(lo, ldir) in &laser_threats {
                let rx = nx - lo[0];
                let ry = ny - lo[1];
                let along = rx * ldir[0] + ry * ldir[1];
                if along > 0.0 && along < 280.0 {
                    let perp = (rx * ldir[1] - ry * ldir[0]).abs();
                    let danger = my_r + 6.5;
                    if perp < danger {
                        score -= (danger - perp) * 26.0;
                    }
                }
            }

            if !urgent_zone && !defensive_cover_move {
                if let Some(t) = target {
                    if t.is_mech {
                        let nd = dist2d([nx, ny], [t.pos[0], t.pos[1]]);
                        let cur_d = dist2d(pos2, [t.pos[0], t.pos[1]]);
                        score -= ((nd - desired_range).abs() - (cur_d - desired_range).abs()) * 0.9;
                        if ci == 0 {
                            score -= 14.0;
                        }
                        // Soft bonus for staying within 1-3 cells (4-12 m) of hard cover during combat
                        // so we can duck enemy lasers in < 0.5s WITHOUT breaking LoS early!
                        if self.map.loaded && !stay_on_roof {
                            let (gx, gy) = self.map.world_to_nav(nx, ny);
                            let wd = self.map.wall_dist_cell(gx, gy);
                            if (1..=3).contains(&wd) {
                                score += 5.0;
                            }
                        }
                    }
                }
            }

            // Explicit ANTI-LIGHTNING repulsion from EVERY visible enemy carrying Lightning!
            if !urgent_zone {
                for e in self.world.visible_enemies() {
                    if e.has_weapon(WeaponKind::Lightning) {
                        let ed = dist2d([nx, ny], [e.pos[0], e.pos[1]]);
                        let safe_lr = if me.has_weapon(WeaponKind::Lightning) && me.hp > e.hp + 150.0 {
                            0.0
                        } else {
                            52.0
                        };
                        if ed < safe_lr {
                            score -= (safe_lr - ed) * 4.2;
                        }
                    }
                }
            }

            if score > best_score {
                best_score = score;
                best_dir = dir;
            }
        }

        (best_dir, urgent_dodge, false)
    }

    fn choose_torso_aim(
        &mut self,
        obs: &Observation,
        combat: &crate::combat::CombatDecision,
        move_dir: Vec2,
    ) -> Vec2 {
        let me = &obs.self_obs;
        let firing_now = combat.fire[0] || combat.fire[1];
        let laser_releasing = me
            .weapons
            .iter()
            .flatten()
            .any(|w| w.kind == WeaponKind::Laser && w.charge.unwrap_or(0.0) > 0.0);

        // NEVER disturb active firing, laser charging, laser release, or direct-LoS combat aiming!
        if let Some(a) = combat.aim {
            if firing_now || combat.charging_laser || laser_releasing || combat.has_los_to_target {
                return a;
            }
        }

        // Strict Rear-Armor Guard when NOT engaging a direct-LoS target (e.g., running to cover or staring at an occluded target):
        if self.world.last_damaged_rear && obs.t - self.world.last_damaged_t < 1.2 {
            if let Some(b) = self.world.last_damaged_bearing {
                return [b, 0.0];
            }
        }
        let my_eye = [me.pos[0], me.pos[1], me.pos[2] + me.stat("eye_height", 8.0)];
        if let Some(nearest_los) = self
            .world
            .visible_enemies()
            .into_iter()
            .filter(|e| {
                self.map
                    .has_los(my_eye, [e.pos[0], e.pos[1], e.pos[2] + 5.0])
            })
            .min_by(|a, b| {
                dist2d([me.pos[0], me.pos[1]], [a.pos[0], a.pos[1]])
                    .partial_cmp(&dist2d([me.pos[0], me.pos[1]], [b.pos[0], b.pos[1]]))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            let yaw = (nearest_los.pos[1] - me.pos[1]).atan2(nearest_los.pos[0] - me.pos[0]);
            return [yaw, 0.0];
        }
        if let Some(a) = combat.aim {
            return a;
        }
        // Keep front 180-deg armor locked onto nearest visible enemy (even while retreating/looting!)
        if let Some(nearest) = self.world.visible_enemies().into_iter().min_by(|a, b| {
            dist2d([me.pos[0], me.pos[1]], [a.pos[0], a.pos[1]])
                .partial_cmp(&dist2d([me.pos[0], me.pos[1]], [b.pos[0], b.pos[1]]))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            let yaw = (nearest.pos[1] - me.pos[1]).atan2(nearest.pos[0] - me.pos[0]);
            return [yaw, 0.0];
        }
        if obs.t - self.world.last_damaged_t < 2.2 {
            if let Some(b) = self.world.last_damaged_bearing {
                return [b, 0.0];
            }
        }
        if let Some(s) = self.world.most_urgent_sound(obs.t) {
            if obs.t - s.at_t < 1.4 {
                return [s.bearing, 0.0];
            }
        }
        // Quick 360-degree spawn recon ONLY in first 1.6s when zero threats exist
        if obs.t < 1.6 && obs.t - self.world.last_damaged_t > 2.0 {
            return [wrap_angle(me.aim[0] + 1.15), -0.05];
        }
        let base_yaw = if move_dir[0].abs() + move_dir[1].abs() > 0.2 {
            move_dir[1].atan2(move_dir[0])
        } else {
            me.aim[0]
        };
        let sweep = (obs.t * 1.65).sin() * 1.45;
        [wrap_angle(base_yaw + sweep), -0.04]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{HelloMap, RawObservation};
    use serde_json::json;

    fn fixture() -> (BotBrain, Observation) {
        let cfg: Config = serde_json::from_value(json!({
            "vision":{"range":200,"fov_deg":120,"near_range":45},
            "sound":{"bearing_noise_deg":7,"near":60,"mid":160,
                "step_speed":5,"step_every":0.6},
            "weapons":{},"loot":{"pickup_radius":4,"equip_s":1},
            "zone":{"start_center":[0,0],"start_radius":500,"phases":[]}
        })).unwrap();
        let map = ArenaMap::empty();
        let nav = Navigator::new(&map);
        let brain = BotBrain {
            cfg,map,nav,world:WorldState::new(16),prev_wanted_move:false,
            regen_mode:false,cached_cover:None,climb_target:None,
            roam_anchor:None,roam_tick:0,goal_track:None,drop_hold:None,
            blacklist:Vec::new(),
        };
        let raw: RawObservation = serde_json::from_value(json!({
            "tick":1740,"t":87,"alive":16,
            "self":{"id":0,"pos":[0,0,0],"vel":[0,0,0],"aim":[0,0],
                "hp":1400,"max_hp":1400,"energy":100,"max_energy":100,
                "grounded":true,"cubes":0,"in_zone":true,
                "weapons":[{"kind":"gun","cooldown":0,"ammo":12,"reload":0},null]},
            "zone":{"center":[0,0],"radius":500,"next_center":[0,0],
                "next_radius":380,"shrink_start":200,"shrink_end":240,"dps":20},
            "drops":[{"id":9000,"pos":[0,0,0],"radius":8,
                "lands_in":8,"capture_s":4,"progress":0,"holder":null}]
        })).unwrap();
        (brain,raw.into())
    }

    fn label(action: &Action) -> &str {
        action.debug.as_ref().and_then(|d| d.label.as_deref()).unwrap_or("")
    }

    #[test]
    fn early_drop_arrival_waits_through_landing_and_capture() {
        let (mut brain,mut obs) = fixture();
        // Deliberately stop inside the ring, away from the center. The server
        // permits capture here; neither travel watchdog may force a jet escape.
        obs.self_obs.pos = [6.0,0.0,0.0];
        for step in 0..120 {
            obs.tick = 1740+step*2;
            obs.t = 87.0+step as f64*0.1;
            obs.drops[0].lands_in = (95.0-obs.t).max(0.0);
            if obs.t >= 95.0 {
                obs.drops[0].holder = Some(0);
                obs.drops[0].progress = obs.t-95.0;
            }
            let action = brain.act(&obs);
            assert!(label(&action).starts_with("DROP"),"t={}: {}",obs.t,label(&action));
            assert!(!brain.is_blacklisted([0.0,0.0],obs.t));
            assert_eq!(brain.nav.stuck_ticks(),0);
            assert_ne!(action.jump,Some(true));
        }
        obs.t = 99.0;
        obs.tick = 1980;
        obs.drops.clear();
        brain.act(&obs);
        assert!(brain.drop_hold.is_none());
        assert!(brain.blacklist.is_empty());
    }

    #[test]
    fn frozen_or_contested_drop_capture_is_bounded() {
        for holder in [None,Some(0),Some(1)] {
            let (mut brain,mut obs) = fixture();
            obs.drops[0].lands_in = 0.0;
            obs.drops[0].holder = holder;
            for step in 0..60 {
                obs.tick = 2000+step*2;
                obs.t = 100.0+step as f64*0.1;
                brain.act(&obs);
            }
            assert!(brain.is_blacklisted([0.0,0.0],obs.t),"holder={holder:?}");
        }
    }

    #[test]
    fn brief_drop_contest_does_not_cancel_a_resumed_capture() {
        let (mut brain,mut obs) = fixture();
        obs.drops[0].lands_in = 0.0;
        for step in 0..70 {
            obs.tick = 2000+step*2;
            obs.t = 100.0+step as f64*0.1;
            let elapsed = step as f64*0.1;
            let (holder,progress) = if elapsed < 1.0 {
                (Some(0),elapsed)
            } else if elapsed < 3.0 {
                (None,0.0)
            } else {
                (Some(0),elapsed-3.0)
            };
            obs.drops[0].holder = holder;
            obs.drops[0].progress = progress;
            let action = brain.act(&obs);
            assert!(label(&action).starts_with("DROP"),"t={}: {}",obs.t,label(&action));
            assert!(brain.blacklist.is_empty());
        }
    }

    #[test]
    fn authoritative_capture_progress_overrides_height_estimate() {
        let (mut brain,mut obs) = fixture();
        obs.self_obs.pos[2] = 10.0;
        obs.drops[0].lands_in = 0.0;
        obs.drops[0].holder = Some(0);
        for step in 0..40 {
            obs.tick = 2000+step*2;
            obs.t = 100.0+step as f64*0.1;
            obs.drops[0].progress = step as f64*0.1;
            assert!(brain.track_goal_progress("DROP",[0.0,0.0],&obs));
            assert!(brain.goal_track.is_none());
            assert!(brain.blacklist.is_empty());
        }
    }

    #[test]
    fn nearly_complete_capture_can_restart_after_a_brief_contest() {
        let (mut brain, mut obs) = fixture();
        obs.drops[0].lands_in = 0.0;
        for step in 0..100 {
            obs.tick = 2000 + step * 2;
            obs.t = 100.0 + step as f64 * 0.1;
            let elapsed = step as f64 * 0.1;
            let (holder, progress) = if elapsed < 4.0 {
                (Some(0), elapsed)
            } else if elapsed < 6.0 {
                (None, 0.0)
            } else {
                (Some(0), elapsed - 6.0)
            };
            obs.drops[0].holder = holder;
            obs.drops[0].progress = progress;
            let action = brain.act(&obs);
            assert!(label(&action).starts_with("DROP"), "t={}: {}", obs.t, label(&action));
            assert!(brain.blacklist.is_empty());
        }
    }

    #[test]
    fn repeated_capture_resets_have_an_absolute_deadline() {
        let (mut brain, mut obs) = fixture();
        obs.drops[0].lands_in = 0.0;
        obs.drops[0].holder = Some(0);
        for step in 0..160 {
            obs.tick = 2000 + step * 2;
            obs.t = 100.0 + step as f64 * 0.1;
            obs.drops[0].progress = (step % 30) as f64 * 0.1;
            brain.act(&obs);
        }
        assert!(brain.is_blacklisted([0.0, 0.0], obs.t));
    }

    #[test]
    fn unreachable_drop_still_uses_travel_watchdog() {
        for pos in [[30.0,0.0,0.0],[0.0,0.0,10.0]] {
            let (mut brain,mut obs) = fixture();
            obs.self_obs.pos = pos;
            for step in 0..46 {
                obs.tick = 1740+step*2;
                obs.t = 87.0+step as f64*0.1;
                obs.drops[0].lands_in = (95.0-obs.t).max(0.0);
                brain.act(&obs);
            }
            assert!(brain.is_blacklisted([0.0,0.0],obs.t),"pos={pos:?}");
        }
    }

    #[test]
    fn zone_escape_interrupts_drop_waiting() {
        let (mut brain,mut obs) = fixture();
        assert!(label(&brain.act(&obs)).starts_with("DROP"));
        obs.t += 0.1;
        obs.tick += 2;
        obs.self_obs.in_zone = false;
        obs.zone.center = [100.0,0.0];
        obs.zone.next_center = [100.0,0.0];
        obs.zone.radius = 20.0;
        obs.zone.next_radius = 10.0;
        assert!(label(&brain.act(&obs)).starts_with("ZONE"));
        assert!(brain.drop_hold.is_none());
        assert!(brain.blacklist.is_empty());
    }

    #[test]
    fn stale_landing_countdown_does_not_allow_unlimited_wait() {
        let (mut brain,mut obs) = fixture();
        for step in 0..160 {
            obs.tick = 1740+step*2;
            obs.t = 87.0+step as f64*0.1;
            // An unchanged countdown should not keep moving the landing deadline.
            brain.act(&obs);
        }
        assert!(brain.is_blacklisted([0.0,0.0],obs.t));
    }

    fn corner_map() -> ArenaMap {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("mechbot-brain-test-{}-{unique}",std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let world = json!({
            "bounds":{"minX":-64,"maxX":64,"minY":-64,"maxY":64},
            "terrain":{"minX":-64,"minY":-64,"step":128,"nx":2,"ny":2,"scale":0.01},
            "prisms":[{"base":0,"top":20,"ring":[8,-8,16,-8,16,8,8,8]}]
        });
        std::fs::write(dir.join("world.json"),world.to_string()).unwrap();
        std::fs::write(dir.join("terrain.i16"),[0u8;8]).unwrap();
        let map = ArenaMap::load(&HelloMap {
            dir:dir.to_string_lossy().into_owned(),
            files:vec!["world.json".into(),"terrain.i16".into()],
        });
        // Only this test's freshly-created directory is removed.
        std::fs::remove_file(dir.join("world.json")).unwrap();
        std::fs::remove_file(dir.join("terrain.i16")).unwrap();
        std::fs::remove_dir(dir).unwrap();
        assert!(map.loaded);
        map
    }

    #[test]
    fn nonlethal_own_laser_does_not_disable_emergency_duck() {
        let (mut brain,mut obs) = fixture();
        brain.map = corner_map();
        brain.nav = Navigator::new(&brain.map);
        obs.drops.clear();
        obs.self_obs.pos = [0.0,12.0,0.0];
        obs.self_obs.weapons[1] = Some(serde_json::from_value(json!({
            "kind":"laser","cooldown":0,"charge":0.5,
            "stats":{"damage":315,"charge":0.75,"min_frac":0.35}
        })).unwrap());
        obs.mechs = serde_json::from_value(json!([{
            "id":1,"kind":"mech","pos":[50,12,0],"vel":[0,0,0],
            "aim":[3.141592653589793,0],"hp":350,"max_hp":1400,
            "cubes":0,"weapons":["laser"],"charging":true
        }])).unwrap();
        let action = brain.act(&obs);
        assert!(label(&action).starts_with("LASER_DUCK"),"{}",label(&action));
    }

    #[test]
    fn exposed_regen_retreat_preserves_defensive_gun_and_laser_fire() {
        for charge in [0.0,0.3] {
            let (mut brain,mut obs) = fixture();
            obs.drops.clear();
            obs.self_obs.hp = 600.0;
            obs.self_obs.aim = [0.0,-0.05];
            obs.self_obs.weapons[1] = Some(serde_json::from_value(json!({
                "kind":"laser","cooldown":0,"charge":charge
            })).unwrap());
            obs.mechs = serde_json::from_value(json!([{
                "id":1,"kind":"mech","pos":[50,0,0],"vel":[0,0,0],
                "aim":[3.141592653589793,0],"hp":1400,"max_hp":1400,
                "cubes":0,"weapons":["gun"],"charging":false
            }])).unwrap();
            let action = brain.act(&obs);
            assert!(label(&action).starts_with("REGEN"),"{}",label(&action));
            assert_eq!(action.fire,Some([true,true]));
        }
    }
}
