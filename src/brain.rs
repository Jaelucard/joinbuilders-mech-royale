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
}

impl BotBrain {
    pub fn new(hello: Hello) -> Self {
        let t0 = std::time::Instant::now();
        let map = ArenaMap::load(&hello.map);
        let nav = Navigator::new(&map);
        let initial_mechs = hello.config.r#match.get("mechs").copied().unwrap_or(16.0) as u64;
        let world = WorldState::new(initial_mechs);
        eprintln!(
            "vanguard-v2 initialized (slot={}, map_loaded={}, prisms={}, roofs={}, nav={}x{}) in {:.1}ms",
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
        }
    }

    pub fn act(&mut self, obs: &Observation) -> Action {
        self.world.update(obs, &self.cfg);
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let hp_frac = me.hp / me.max_hp.max(1.0);
        let walk_speed = me.stat("walk_speed", 11.7);
        let on_roof = self.map.is_on_roof(pos);

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
        let safe_next_r = (z.next_radius - 10.0).max(4.0);
        let dist_to_next_safe = (d_next - safe_next_r).max(0.0);
        let time_to_shrink_end = (z.shrink_end - obs.t).max(0.0);
        let est_travel_s = dist_to_next_safe / (walk_speed * 0.72).max(4.0);

        let urgent_zone = !me.in_zone
            || (obs.t >= z.shrink_start - 3.0 && d_cur > (z.radius - 22.0).max(6.0))
            || (dist_to_next_safe > 0.0 && est_travel_s + 9.0 >= time_to_shrink_end);

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
        let mut charging_los_snipers: Vec<Vec3> = Vec::new();
        let mut all_vis_threat_eyes: Vec<Vec3> = Vec::new();

        for e in self.world.visible_enemies() {
            vis_enemy_count += 1;
            let ed = dist2d(pos2, [e.pos[0], e.pos[1]]);
            let e_eye = [e.pos[0], e.pos[1], e.pos[2] + 7.5];
            all_vis_threat_eyes.push(e_eye);
            let has_los_to_e = self.map.has_los(my_eye, e_eye);
            if has_los_to_e && ed < 125.0 {
                los_enemies_close += 1;
            }
            if has_los_to_e && (e.aiming_at_me || obs.t - e.last_damaged_me_t < 3.0) {
                any_los_aiming = true;
            }
            if e.charging && has_los_to_e && ed < 275.0 {
                let can_short =
                    lightning_range > 0.0 && ed <= lightning_range + 2.0 && me.energy >= 6.0;
                if !can_short {
                    charging_los_snipers.push(e_eye);
                }
            }
        }

        // If nobody is visible right now, synthesize a threat from recent damage bearing for cover-hiding
        if all_vis_threat_eyes.is_empty() && obs.t - self.world.last_damaged_t < 4.5 {
            if let Some(b) = self.world.last_damaged_bearing {
                all_vis_threat_eyes.push([
                    pos[0] + b.cos() * 65.0,
                    pos[1] + b.sin() * 65.0,
                    pos[2] + 7.5,
                ]);
            }
        }

        // EMERGENCY LASER-DUCK: enemy charging a 315-dmg hitscan laser at us!
        let laser_duck_cover = if !urgent_zone && !charging_los_snipers.is_empty() {
            self.map
                .find_cover_pos(pos, &charging_los_snipers, safe_c, safe_r, 28.0)
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

        // Hysteresis on cover-regen mode: ONLY retreat to cover while threats exist (never run aimlessly in open!)
        let enter_regen = !urgent_zone
            && me.in_zone
            && !all_vis_threat_eyes.is_empty()
            && ((hp_frac < 0.48 && active_threats >= 1)
                || (hp_frac < 0.60 && active_threats >= 2)
                || (hp_frac < 0.68 && has_nanites && regen_eta <= 1.5 && active_threats >= 1)
                || (hp_frac < 0.50 && obs.t - self.world.last_damaged_t < 2.5));
        let exit_hp = if has_nanites { 0.88 } else { 0.80 };
        if enter_regen {
            self.regen_mode = true;
        } else if urgent_zone
            || !me.in_zone
            || hp_frac >= exit_hp
            || (all_vis_threat_eyes.is_empty() && obs.t - self.world.last_damaged_t >= 4.5)
        {
            self.regen_mode = false;
        }
        let want_regen_retreat = self.regen_mode && !all_vis_threat_eyes.is_empty();
        // Hold fire ONLY against crates/NPCs or unprovoked long shots when we aren't actively being beamed!
        let hold_fire_for_regen = (self.regen_mode || (hp_frac < 0.72 && regen_eta <= 1.8)) && !any_los_aiming;

        // 5. Should we farm cubes?
        let want_cubes = me.cubes < 10
            || self
                .cfg
                .cubes
                .upgrade_thresholds
                .iter()
                .any(|&th| th as u64 > me.cubes && (th as u64) - me.cubes <= 2);

        // 6. Combat target & laser peek pair check
        let allow_farm =
            !urgent_zone && !want_regen_retreat && !doing_laser_duck && (want_cubes || me.empty_slot().is_some());
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

        // Allow pre-charging our laser behind cover if we are standing near the hide spot of a peek pair!
        let allow_precharge_behind_cover = peek_pair
            .is_some_and(|(hide, _)| dist2d(pos2, [hide[0], hide[1]]) < 8.5);
        let avoid_open_laser_slowdown = doing_laser_duck || (los_enemies_close >= 2 && hp_frac < 0.78);

        let combat = evaluate_combat(
            obs,
            &self.cfg,
            &self.map,
            &self.world,
            target.as_ref(),
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
            target.as_ref(),
            &combat,
            peek_pair,
            &all_vis_threat_eyes,
            safe_c,
            safe_r,
        );

        let goal2 = [goal_pos3[0], goal_pos3[1]];
        let dist_to_goal = dist2d(pos2, goal2);

        // Update stuck detector before pathing
        let stuck = self.nav.update_stuck(obs.tick, pos, self.prev_wanted_move);
        let waypoint = if on_roof && stay_on_roof {
            // Moving directly on rooftop surface (don't route through ground nav grid!)
            goal2
        } else if dist_to_goal > 6.5 {
            self.nav.next_waypoint(&self.map, obs.tick, pos, goal2)
        } else {
            goal2
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
        );

        let move_mag = (move_dir[0] * move_dir[0] + move_dir[1] * move_dir[1]).sqrt();
        self.prev_wanted_move = move_mag > 0.25 && dist_to_goal > 2.5;
        act.r#move = Some(move_dir);

        // 9. Jump & Boost / Jetpack decision (supports full rooftop climbing!)
        let target_is_higher = goal_pos3[2] - pos[2] > 1.3 && dist_to_goal < 14.0;
        let climb_jet = target_is_higher && me.energy > 14.0;
        let mut jump = false;
        let mut boost = false;

        if climb_jet || need_jump_or_jet {
            if me.grounded {
                jump = true;
            } else if me.energy > 10.0 {
                boost = true; // jet upward onto roof/ledge!
            }
        } else if me.grounded {
            let reserve = if me.has_weapon(WeaponKind::Lightning) {
                24.0
            } else {
                16.0
            };
            if (dodge_boost || doing_laser_duck) && me.energy > 6.0 {
                boost = true;
            } else if want_boost_macro && me.energy > reserve && move_mag > 0.5 {
                boost = true;
            } else if !combat.charging_laser
                && target.as_ref().is_some_and(|t| t.is_mech)
                && me.energy > reserve + 20.0
                && move_mag > 0.6
            {
                boost = true;
            }
        }
        act.jump = Some(jump);
        act.boost = Some(boost);

        // 10. Torso Aim Discipline (Combat > Rear-Armor Facing > Sound Threat > Active Scan)
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
        target: Option<&CombatTarget>,
        combat: &crate::combat::CombatDecision,
        peek_pair: Option<(Vec3, Vec3)>,
        all_vis_threat_eyes: &[Vec3],
        safe_c: Vec2,
        safe_r: f64,
    ) -> (Vec3, &'static str, bool, bool) {
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let z = &obs.zone;
        let hp_frac = me.hp / me.max_hp.max(1.0);
        let on_roof = self.map.is_on_roof(pos);

        // Priority 1: Zone urgency
        if urgent_zone {
            self.climb_target = None;
            let gz = self
                .map
                .terrain_z(z.next_center[0] as f32, z.next_center[1] as f32) as f64;
            return ([z.next_center[0], z.next_center[1], gz], "ZONE", true, false);
        }

        // Priority 2: EMERGENCY LASER-DUCK behind cover before 315-dmg beam fires!
        if let Some(cov) = laser_duck_cover {
            let stay = on_roof && (cov[2] - pos[2]).abs() < 1.8;
            return (cov, "LASER_DUCK", true, stay);
        }

        // Priority 3: Retreat to hard cover & hold until healed!
        if want_regen_retreat {
            self.climb_target = None;
            let cover = self.get_or_update_cover(obs, all_vis_threat_eyes, safe_c, safe_r, 48.0);
            if let Some(cov) = cover {
                let stay = on_roof && (cov[2] - pos[2]).abs() < 1.8;
                let need_sprint = dist2d(pos2, [cov[0], cov[1]]) > 5.0;
                return (cov, "REGEN_COVER", need_sprint, stay);
            }
            if let Some(th) = all_vis_threat_eyes.first() {
                let away_x = pos[0] - th[0];
                let away_y = pos[1] - th[1];
                let len = (away_x * away_x + away_y * away_y).sqrt().max(1.0);
                let gx = pos[0] + (away_x / len) * 35.0;
                let gy = pos[1] + (away_y / len) * 35.0;
                if dist2d([gx, gy], safe_c) < safe_r {
                    return ([gx, gy, pos[2]], "REGEN_KITE", true, false);
                }
            }
            return ([z.next_center[0], z.next_center[1], pos[2]], "REGEN_ZONE", true, false);
        }

        // Priority 4: Grab kill-drop cubes & visible cubes (< 78 m when safe!)
        if want_cubes {
            let safe_for_loot = self.world.active_threat_count(pos, obs.t) <= 1 && hp_frac > 0.52;
            let nearest_cube = self
                .world
                .cubes
                .values()
                .filter(|c| {
                    let d = dist2d(pos2, [c.pos[0], c.pos[1]]);
                    d < if safe_for_loot { 78.0 } else { 26.0 }
                        && (c.pos[2] - pos[2]) < 16.0
                        && dist2d([c.pos[0], c.pos[1]], z.center) < z.radius - 5.0
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.pos[0], a.pos[1]])
                        .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(c) = nearest_cube {
                let d = dist2d(pos2, [c.pos[0], c.pos[1]]);
                return (c.pos, "CUBE", d > 12.0, on_roof && (c.pos[2] - pos[2]).abs() < 2.0);
            }

            if safe_for_loot {
                let nearest_kill_drop = self
                    .world
                    .kill_drops
                    .iter()
                    .filter(|(kp, _)| {
                        dist2d(pos2, [kp[0], kp[1]]) < 95.0
                            && (kp[2] - pos[2]) < 18.0
                            && dist2d([kp[0], kp[1]], z.center) < z.radius - 8.0
                    })
                    .min_by(|a, b| {
                        dist2d(pos2, [a.0[0], a.0[1]])
                            .partial_cmp(&dist2d(pos2, [b.0[0], b.0[1]]))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                if let Some((kp, _)) = nearest_kill_drop {
                    return (*kp, "KILL_LOOT", true, false);
                }
            }
        }

        // Priority 5: Grab a desirable weapon pickup nearby
        let holding_mortar = me.has_weapon(WeaponKind::Mortar);
        let pickup_search_r = if me.empty_slot().is_some() || holding_mortar {
            125.0
        } else {
            65.0
        };
        if let Some((p_pos, _)) =
            best_nearby_pickup(obs, &self.world, pickup_search_r, z.center, z.radius)
        {
            let safe_enough = hp_frac > 0.50 || self.world.active_threat_count(pos, obs.t) == 0;
            if safe_enough {
                return (
                    p_pos,
                    "LOOT_WPN",
                    me.empty_slot().is_some() || holding_mortar,
                    on_roof && (p_pos[2] - pos[2]).abs() < 2.0,
                );
            }
        }

        // Priority 6: Supply Drops (3 cubes + FULL HP + FULL ENERGY!)
        let best_drop = obs
            .drops
            .iter()
            .filter(|d| {
                let dd = dist2d(pos2, [d.pos[0], d.pos[1]]);
                let in_safe = dist2d([d.pos[0], d.pos[1]], z.next_center) <= z.next_radius + 10.0;
                in_safe && d.lands_in <= 22.0 && (dd < 185.0 || hp_frac < 0.72 || me.cubes < 10)
            })
            .min_by(|a, b| {
                dist2d(pos2, [a.pos[0], a.pos[1]])
                    .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(drop) = best_drop {
            let threats_here = self.world.active_threat_count(pos, obs.t);
            let d_drop = dist2d(pos2, [drop.pos[0], drop.pos[1]]);
            if threats_here <= 1 || hp_frac > 0.65 || d_drop <= drop.radius + 5.0 {
                return (drop.pos, "DROP", d_drop > drop.radius, false);
            }
        }

        // Priority 7: Active Combat (Laser Peek-and-Duck + Slice-the-Pie 1vMany + High-Ground!)
        if let Some(t) = target {
            if t.is_mech {
                let d = dist2d(pos2, [t.pos[0], t.pos[1]]);
                let threats = self.world.active_threat_count(pos, obs.t);
                let practice = self.world.looks_like_practice_squad(obs.t);

                // 7a. Slice-the-pie when outnumbered (>= 2 enemies!): isolate ONLY 1 target behind buildings!
                if threats >= 2 {
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
                    // Fallback: break LoS to everyone
                    if let Some(cov) =
                        self.get_or_update_cover(obs, all_vis_threat_eyes, safe_c, safe_r, 40.0)
                    {
                        let stay = on_roof && (cov[2] - pos[2]).abs() < 1.8;
                        return (cov, "KITE_COVER", true, stay);
                    }
                }

                // 7b. Laser Corner-Peek & Pre-Charge Loop!
                if let Some((hide_pos, peek_pos)) = peek_pair {
                    let stay_h = on_roof && (hide_pos[2] - pos[2]).abs() < 1.8;
                    let stay_p = on_roof && (peek_pos[2] - pos[2]).abs() < 1.8;
                    // Start stepping out at 0.30s so we reach peek_pos right as charge hits 0.75s!
                    if combat.laser_charge_s >= 0.30 {
                        return (peek_pos, "LASER_PEEK", false, stay_p);
                    } else if combat.laser_cooldown_s > 0.35 && !me.has_weapon(WeaponKind::Gun) {
                        return (hide_pos, "LASER_HIDE", dist2d(pos2, [hide_pos[0], hide_pos[1]]) > 4.0, stay_h);
                    } else if combat.laser_charge_s > 0.0 || combat.laser_cooldown_s <= 0.25 {
                        return (hide_pos, "LASER_PRECHG", false, stay_h);
                    }
                }

                // 7c. Stay on roof high-ground if we already have LoS!
                if on_roof && combat.has_los_to_target && dist2d(pos2, z.next_center) < z.next_radius {
                    return (pos, "ROOF_FIGHT", false, true);
                }

                if !combat.has_los_to_target && (practice || t.hp < 450.0) && d < 120.0 {
                    return (t.pos, "FLANK", false, false);
                }

                let dx = pos[0] - t.pos[0];
                let dy = pos[1] - t.pos[1];
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let ux = dx / len;
                let uy = dy / len;
                let strafe_sign = if (obs.tick / 35) % 2 == 0 { 1.0 } else { -1.0 };
                let tx = -uy * strafe_sign;
                let ty = ux * strafe_sign;

                let goal_r = combat.desired_range.clamp(22.0, 115.0);
                let mut gx = t.pos[0] + ux * goal_r + tx * 16.0;
                let mut gy = t.pos[1] + uy * goal_r + ty * 16.0;
                if dist2d([gx, gy], z.center) > (z.radius - 14.0).max(6.0) {
                    gx = z.next_center[0];
                    gy = z.next_center[1];
                }
                return ([gx, gy, pos[2]], "COMBAT", d > goal_r + 25.0, on_roof);
            }
        }

        // Priority 8: Farm nearby NPCs / crates when < 10 cubes
        if want_cubes {
            if let Some(t) = target {
                if !t.is_mech {
                    let dx = pos[0] - t.pos[0];
                    let dy = pos[1] - t.pos[1];
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    let strafe = if (obs.tick / 25) % 2 == 0 { 1.0 } else { -1.0 };
                    let gx = t.pos[0] + (dx / len) * 24.0 + (-dy / len) * strafe * 14.0;
                    let gy = t.pos[1] + (dy / len) * 24.0 + (dx / len) * strafe * 14.0;
                    return ([gx, gy, pos[2]], "FARM_FIGHT", false, on_roof);
                }
            }
            let best_crate = self
                .world
                .crates
                .values()
                .filter(|c| {
                    dist2d(pos2, [c.pos[0], c.pos[1]]) < 100.0
                        && (c.pos[2] - pos[2]) < 14.0
                        && dist2d([c.pos[0], c.pos[1]], safe_c) < safe_r
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

        // Priority 9: Seize Rooftop High Ground inside next_center (when armed & ready!)
        if !on_roof && me.empty_slot().is_none() && me.energy >= 62.0 && z.next_radius >= 45.0 {
            let active_climb = self.climb_target.and_then(|(ctick, anchor)| {
                if obs.tick.saturating_sub(ctick) < 90
                    && dist2d([anchor.roof_pos[0], anchor.roof_pos[1]], z.next_center)
                        <= (z.next_radius - 10.0).max(8.0)
                {
                    Some(anchor)
                } else {
                    None
                }
            });
            let anchor = active_climb.or_else(|| {
                let found = self
                    .map
                    .find_nearby_rooftop(pos, z.next_center, z.next_radius, 48.0, 22.0);
                if let Some(a) = found {
                    self.climb_target = Some((obs.tick, a));
                }
                found
            });
            if let Some(a) = anchor {
                let d_launch = dist2d(pos2, [a.launch_pos[0], a.launch_pos[1]]);
                let need_e = (a.climb_dz * 2.1 + 18.0).min(85.0);
                if d_launch > 6.5 {
                    return (a.launch_pos, "ROOF_APPROACH", false, false);
                } else if me.energy >= need_e {
                    return (a.roof_pos, "ROOF_CLIMB", false, false);
                }
            }
        } else if on_roof
            && dist2d(pos2, z.next_center) < (z.next_radius - 10.0).max(8.0)
            && (!self.world.looks_like_practice_squad(obs.t) || z.next_radius <= 160.0 || !want_cubes)
        {
            self.climb_target = None;
            return (pos, "ROOF_HOLD", false, true);
        }

        // Priority 10: In Practice mode with nobody visible, hunt toward recent threat sounds (if healthy & armed)
        if self.world.looks_like_practice_squad(obs.t)
            && hp_frac > 0.72
            && me.empty_slot().is_none()
        {
            if let Some(s) = self.world.most_urgent_sound(obs.t) {
                let probe_d = match s.dist {
                    crate::protocol::DistBand::Near => 35.0,
                    crate::protocol::DistBand::Mid => 75.0,
                    crate::protocol::DistBand::Far => 110.0,
                };
                let gx = pos[0] + s.bearing.cos() * probe_d;
                let gy = pos[1] + s.bearing.sin() * probe_d;
                if dist2d([gx, gy], z.next_center) <= (z.next_radius - 10.0).max(12.0) {
                    return ([gx, gy, pos[2]], "HUNT_SND", false, false);
                }
            }
        }

        // Priority 11: Patrol strong positions inside next_center
        let need_new_anchor = self.roam_anchor.is_none_or(|a| {
            dist2d(pos2, a) < 16.0
                || dist2d(a, z.next_center) > (z.next_radius * 0.72).max(10.0)
                || obs.tick.saturating_sub(self.roam_tick) > 120
        });
        if need_new_anchor {
            let r = (z.next_radius * 0.42).min(95.0);
            let angle =
                ((obs.tick as f64 * 0.13) + (me.id as f64 * 1.1)).rem_euclid(std::f64::consts::TAU);
            let ax = z.next_center[0] + angle.cos() * r;
            let ay = z.next_center[1] + angle.sin() * r;
            let anchor = if let Some((gx, gy)) = self.map.nearest_walkable_cell(ax, ay, 15) {
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
        // Reuse cached cover for up to 5 ticks if it's still safe & hidden!
        if let Some((ctick, cpos)) = self.cached_cover {
            if obs.tick.saturating_sub(ctick) < 5
                && dist2d([cpos[0], cpos[1]], safe_c) < safe_r
                && dist2d([cpos[0], cpos[1]], [pos[0], pos[1]]) < max_r
            {
                let ceye = [cpos[0], cpos[1], cpos[2] + 7.5];
                if threats.iter().take(4).all(|&th| !self.map.has_los(ceye, th)) {
                    return Some(cpos);
                }
            }
        }
        let found = self.map.find_cover_pos(pos, threats, safe_c, safe_r, max_r);
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
    ) -> (Vec2, bool, bool) {
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let scale = 1.0 + self.cfg.cubes.size_per_cube * (me.cubes as f64);
        let my_r = me.stat("radius", 2.5) * scale;
        let walk_spd = me.stat("walk_speed", 11.7);
        let z = &obs.zone;

        // If climbing straight up onto a rooftop edge, push directly toward the roof while jetting!
        let climbing_up = goal_pos3[2] - pos[2] > 1.3 && dist2d(pos2, [goal_pos3[0], goal_pos3[1]]) < 14.0;
        if climbing_up {
            let dx = goal_pos3[0] - pos[0];
            let dy = goal_pos3[1] - pos[1];
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            // If feet haven't cleared the roof yet, hold back slightly until airborne clearance!
            let push = if pos[2] >= goal_pos3[2] - 0.8 { 1.0 } else { 0.45 };
            return ([dx / d * push, dy / d * push], false, true);
        }

        if stuck {
            return (self.nav.unstuck_direction(), true, true);
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

        for ci in 0..=16usize {
            let dir = if ci == 0 {
                [0.0, 0.0]
            } else {
                let ang = ((ci - 1) as f64) * (std::f64::consts::TAU / 16.0);
                [ang.cos(), ang.sin()]
            };

            let nx = pos[0] + dir[0] * walk_spd * step_dt;
            let ny = pos[1] + dir[1] * walk_spd * step_dt;
            let mut score = 0.0;

            // 1. Goal / Waypoint alignment
            if wp_dist > 2.2 {
                let dot = dir[0] * wp_dir[0] + dir[1] * wp_dir[1];
                let w = if urgent_zone || defensive_cover_move {
                    30.0
                } else {
                    18.0
                };
                score += dot * w;
            } else if ci == 0 {
                score += if defensive_cover_move { 18.0 } else { 7.0 };
            }

            // 2. Wall, edge & rooftop retention
            if self.map.loaded {
                if stay_on_roof {
                    // Don't accidentally fall off our roof while strafing!
                    let next_top = self.map.solid_top_at(nx as f32, ny as f32) as f64;
                    if next_top < pos[2] - 2.5 {
                        score -= 90.0;
                    }
                }
                let check_x = pos[0] + dir[0] * (my_r + 2.6);
                let check_y = pos[1] + dir[1] * (my_r + 2.6);
                if self.map.is_blocked_at(check_x, check_y, pos[2], my_r * 0.85) {
                    score -= 70.0;
                }
                if !stay_on_roof {
                    let (gx, gy) = self.map.world_to_nav(nx, ny);
                    let wd = self.map.wall_dist_cell(gx, gy);
                    if wd == 0
                        && pos[2] <= self.map.terrain_z(pos[0] as f32, pos[1] as f32) as f64 + 1.5
                    {
                        score -= 35.0;
                    }
                }
            }

            // 3. Zone safety boundary
            let nd_cur = dist2d([nx, ny], z.center);
            if nd_cur > z.radius - 4.0 {
                score -= (nd_cur - (z.radius - 4.0)) * 14.0;
            }
            if !me.in_zone {
                let d_before = dist2d(pos2, z.center);
                score += (d_before - nd_cur) * 8.0;
            }

            // 4. Incoming projectile dodging
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
                                let safe_r = my_r + 2.2;
                                if d_perp < safe_r {
                                    score -= (safe_r - d_perp) * 24.0;
                                    if t_closest < 0.35 && d_perp < my_r + 1.0 {
                                        urgent_dodge = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 5. Charging enemy laser ray dodging (supplementary to cover duck!)
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

            // 6. Combat micro-strafe & range discipline
            if !urgent_zone && !defensive_cover_move {
                if let Some(t) = target {
                    if t.is_mech {
                        let nd = dist2d([nx, ny], [t.pos[0], t.pos[1]]);
                        let cur_d = dist2d(pos2, [t.pos[0], t.pos[1]]);
                        score -= ((nd - desired_range).abs() - (cur_d - desired_range).abs()) * 0.9;
                        if ci == 0 && !stay_on_roof {
                            score -= 10.0;
                        }
                        if !me.has_weapon(WeaponKind::Lightning) && nd < 46.0 {
                            score -= (46.0 - nd) * 1.4;
                        }
                    }
                }
            }

            let cur_spd = (me.vel[0] * me.vel[0] + me.vel[1] * me.vel[1]).sqrt();
            if cur_spd > 2.0 {
                let vdot = (dir[0] * me.vel[0] + dir[1] * me.vel[1]) / cur_spd;
                score += vdot * 2.2;
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
        if let Some(a) = combat.aim {
            return a;
        }
        let me = &obs.self_obs;
        if obs.t - self.world.last_damaged_t < 1.8 {
            if let Some(b) = self.world.last_damaged_bearing {
                return [b, 0.0];
            }
        }
        if let Some(s) = self.world.most_urgent_sound(obs.t) {
            if obs.t - s.at_t < 1.2 {
                return [s.bearing, 0.0];
            }
        }
        let base_yaw = if move_dir[0].abs() + move_dir[1].abs() > 0.2 {
            move_dir[1].atan2(move_dir[0])
        } else {
            me.aim[0]
        };
        let sweep = (obs.t * 1.55).sin() * 1.40;
        [wrap_angle(base_yaw + sweep), -0.04]
    }
}
