use crate::combat::{
    best_nearby_pickup, choose_immediate_pickup, choose_upgrade, evaluate_combat, muzzle_pos,
    select_target, CombatTarget,
};
use crate::map::ArenaMap;
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
            "vanguard-v1 initialized (slot={}, map_loaded={}, prisms={}, nav={}x{}) in {:.1}ms",
            hello.slot,
            map.loaded,
            map.prisms.len(),
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

        // 3. Compute zone safety & urgency
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

        // 4. Assess threat pressure & regen state
        let (vis_enemy_count, min_enemy_dist) = {
            let vis = self.world.visible_enemies();
            let md = vis
                .iter()
                .map(|e| dist2d(pos2, [e.pos[0], e.pos[1]]))
                .fold(f64::INFINITY, f64::min);
            (vis.len(), md)
        };
        let active_threats = self.world.active_threat_count(pos, obs.t);
        let has_nanites = me.upgrades.iter().any(|u| u == "nanites");
        let regen_eta = self.world.time_until_regen(obs);

        // Hysteresis on cover-regen mode: enter when hurt, stay behind cover until healed back up!
        let enter_regen = !urgent_zone
            && me.in_zone
            && ((hp_frac < 0.48 && active_threats >= 1)
                || (hp_frac < 0.60 && active_threats >= 2)
                || (hp_frac < 0.68 && has_nanites && regen_eta <= 1.2 && active_threats >= 1)
                || (hp_frac < 0.52 && obs.t - self.world.last_damaged_t < 2.5));
        let exit_hp = if has_nanites { 0.88 } else { 0.82 };
        if enter_regen {
            self.regen_mode = true;
        } else if urgent_zone || !me.in_zone || hp_frac >= exit_hp {
            self.regen_mode = false;
        }
        let want_regen_retreat = self.regen_mode;

        // Should we hold fire to let regen tick (only if not being point-blank rushed)?
        let hold_fire_for_regen = want_regen_retreat && min_enemy_dist > 34.0;

        // 5. Should we farm cubes?
        let want_cubes = me.cubes < 10
            || self
                .cfg
                .cubes
                .upgrade_thresholds
                .iter()
                .any(|&th| th as u64 > me.cubes && (th as u64) - me.cubes <= 2);

        // 6. Combat target & weapon evaluation
        let allow_farm = !urgent_zone && !want_regen_retreat && (want_cubes || me.empty_slot().is_some());
        let target = select_target(
            obs,
            &self.cfg,
            &self.map,
            &self.world,
            allow_farm,
            hold_fire_for_regen,
        );
        let combat = evaluate_combat(
            obs,
            &self.cfg,
            &self.map,
            &self.world,
            target.as_ref(),
            urgent_zone,
            hold_fire_for_regen,
        );

        act.fire = Some(combat.fire);
        act.reload = Some(combat.reload);

        // 7. Determine Macro Movement Goal
        let (goal_pos3, state_label, want_boost_macro) = self.choose_macro_goal(
            obs,
            urgent_zone,
            want_regen_retreat,
            want_cubes,
            target.as_ref(),
            combat.desired_range,
            combat.has_los_to_target,
        );

        let goal2 = [goal_pos3[0], goal_pos3[1]];
        let dist_to_goal = dist2d(pos2, goal2);

        // Update stuck detector before pathing
        let stuck = self.nav.update_stuck(obs.tick, pos, self.prev_wanted_move);
        let waypoint = if dist_to_goal > 6.5 {
            self.nav.next_waypoint(&self.map, obs.tick, pos, goal2)
        } else {
            goal2
        };

        // 8. 16-Direction Micro-Steering (Projectiles + Charging Lasers + Cover + Walls + Zone)
        let (move_dir, dodge_boost, need_jump_or_jet) = self.compute_micro_move(
            obs,
            waypoint,
            goal_pos3,
            target.as_ref(),
            combat.desired_range,
            urgent_zone,
            want_regen_retreat,
            stuck,
        );

        let move_mag = (move_dir[0] * move_dir[0] + move_dir[1] * move_dir[1]).sqrt();
        self.prev_wanted_move = move_mag > 0.25 && dist_to_goal > 2.5;
        act.r#move = Some(move_dir);

        // 9. Jump & Boost / Jetpack decision
        let target_is_higher = goal_pos3[2] - pos[2] > 1.3 && dist_to_goal < 18.0;
        let climb_jet = target_is_higher && me.energy > 15.0;
        let mut jump = false;
        let mut boost = false;

        if climb_jet || need_jump_or_jet {
            if me.grounded {
                jump = true;
            } else if me.energy > 12.0 {
                boost = true; // jet upward!
            }
        } else if me.grounded {
            let reserve = if me.has_weapon(WeaponKind::Lightning) {
                26.0
            } else {
                18.0
            };
            if dodge_boost && me.energy > 8.0 {
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

        // 10. Torso Aim Discipline (Combat > Rear-Armor Facing > Sound Threat > Active 360 Scan)
        let desired_aim = self.choose_torso_aim(obs, &combat, move_dir);
        self.world.last_aim = desired_aim;
        act.aim = Some(desired_aim);

        // 11. Compact Debug Drawing (strictly inside caps)
        dbg.set_label(format!(
            "{} | hp:{:.0} c:{} e:{}",
            state_label,
            me.hp,
            me.cubes,
            vis_enemy_count
        ));
        dbg.point([goal2[0], goal2[1], pos[2] + 1.0], "#40c0ff", 5.0);
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

    fn choose_macro_goal(
        &mut self,
        obs: &Observation,
        urgent_zone: bool,
        want_regen_retreat: bool,
        want_cubes: bool,
        target: Option<&CombatTarget>,
        desired_range: f64,
        has_los_to_target: bool,
    ) -> (Vec3, &'static str, bool) {
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let z = &obs.zone;
        let hp_frac = me.hp / me.max_hp.max(1.0);

        // Priority 1: Zone urgency
        if urgent_zone {
            let gz = self.map.terrain_z(z.next_center[0] as f32, z.next_center[1] as f32) as f64;
            return ([z.next_center[0], z.next_center[1], gz], "ZONE", true);
        }

        // Priority 2: Retreat to hard cover & heal
        if want_regen_retreat {
            let threats: Vec<Vec3> = self
                .world
                .visible_enemies()
                .iter()
                .map(|e| [e.pos[0], e.pos[1], e.pos[2] + 7.5])
                .collect();
            if let Some(cover) = self.map.find_cover_pos(
                pos,
                &threats,
                z.next_center,
                z.next_radius.max(z.radius * 0.7),
                44.0,
            ) {
                return (cover, "REGEN_COVER", true);
            }
            // Fallback: kite away from nearest threat toward zone center
            if let Some(th) = threats.first() {
                let away_x = pos[0] - th[0];
                let away_y = pos[1] - th[1];
                let len = (away_x * away_x + away_y * away_y).sqrt().max(1.0);
                let gx = pos[0] + (away_x / len) * 35.0;
                let gy = pos[1] + (away_y / len) * 35.0;
                // Pull toward next_center if leaving safe zone
                if dist2d([gx, gy], z.next_center) < z.next_radius {
                    return ([gx, gy, pos[2]], "REGEN_KITE", true);
                }
            }
            return ([z.next_center[0], z.next_center[1], pos[2]], "REGEN_ZONE", true);
        }

        // Priority 3: Grab immediate visible cube right nearby (< 28 m) if safe
        if want_cubes {
            let nearest_cube = self
                .world
                .cubes
                .values()
                .filter(|c| {
                    let d = dist2d(pos2, [c.pos[0], c.pos[1]]);
                    d < 32.0
                        && (c.pos[2] - pos[2]) < 15.0
                        && dist2d([c.pos[0], c.pos[1]], z.center) < z.radius - 5.0
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.pos[0], a.pos[1]])
                        .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(c) = nearest_cube {
                let near_danger = self.world.active_threat_count(pos, obs.t) >= 2 && hp_frac < 0.7;
                if !near_danger {
                    return (c.pos, "CUBE", dist2d(pos2, [c.pos[0], c.pos[1]]) > 10.0);
                }
            }
        }

        // Priority 4: Grab a desirable weapon pickup nearby (huge power spike if slot 1 empty!)
        let pickup_search_r = if me.empty_slot().is_some() { 115.0 } else { 55.0 };
        if let Some((p_pos, _)) = best_nearby_pickup(obs, &self.world, pickup_search_r, z.center, z.radius) {
            let safe_enough = hp_frac > 0.50 || self.world.active_threat_count(pos, obs.t) == 0;
            if safe_enough {
                return (p_pos, "LOOT_WPN", me.empty_slot().is_some());
            }
        }

        // Priority 5: Supply Drops (3 cubes + FULL HP + FULL ENERGY!)
        let best_drop = obs
            .drops
            .iter()
            .filter(|d| {
                let dd = dist2d(pos2, [d.pos[0], d.pos[1]]);
                let in_safe = dist2d([d.pos[0], d.pos[1]], z.next_center) <= z.next_radius + 10.0;
                in_safe && d.lands_in <= 20.0 && (dd < 170.0 || hp_frac < 0.70 || me.cubes < 8)
            })
            .min_by(|a, b| {
                dist2d(pos2, [a.pos[0], a.pos[1]])
                    .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(drop) = best_drop {
            let threats_here = self.world.active_threat_count(pos, obs.t);
            if threats_here <= 1 || hp_frac > 0.65 || dist2d(pos2, [drop.pos[0], drop.pos[1]]) <= drop.radius + 4.0 {
                let d_drop = dist2d(pos2, [drop.pos[0], drop.pos[1]]);
                return (drop.pos, "DROP", d_drop > drop.radius);
            }
        }

        // Priority 6: Active Combat Positioning against Mech
        if let Some(t) = target {
            if t.is_mech {
                let d = dist2d(pos2, [t.pos[0], t.pos[1]]);
                let threats = self.world.active_threat_count(pos, obs.t);
                let practice = self.world.looks_like_practice_squad(obs.t);

                // If outnumbered (>= 2 threats) or laser on cooldown while hurt -> duck near cover!
                let laser_on_cd = me
                    .weapons
                    .iter()
                    .flatten()
                    .any(|w| w.kind == WeaponKind::Laser && w.cooldown > 0.55)
                    && !me.has_weapon(WeaponKind::Gun)
                    && d > 48.0;

                if threats >= 2 || laser_on_cd {
                    let th_list: Vec<Vec3> = self
                        .world
                        .visible_enemies()
                        .iter()
                        .map(|e| [e.pos[0], e.pos[1], e.pos[2] + 7.5])
                        .collect();
                    if let Some(cov) = self.map.find_cover_pos(pos, &th_list, z.next_center, z.next_radius, 34.0) {
                        return (cov, "KITE_COVER", threats >= 2);
                    }
                }

                // In Practice or with HP advantage: engage at optimal weapon range; in FFA stay disciplined
                if !has_los_to_target && (practice || t.hp < 450.0) && d < 120.0 {
                    return (t.pos, "FLANK", false);
                }

                // Compute anchor point at ideal range + slight orbital strafe offset
                let dx = pos[0] - t.pos[0];
                let dy = pos[1] - t.pos[1];
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let ux = dx / len;
                let uy = dy / len;
                let strafe_sign = if (obs.tick / 35) % 2 == 0 { 1.0 } else { -1.0 };
                let tx = -uy * strafe_sign;
                let ty = ux * strafe_sign;

                let goal_r = desired_range.clamp(22.0, 120.0);
                let mut gx = t.pos[0] + ux * goal_r + tx * 16.0;
                let mut gy = t.pos[1] + uy * goal_r + ty * 16.0;

                // Keep combat anchor inside safe circle
                if dist2d([gx, gy], z.center) > (z.radius - 14.0).max(6.0) {
                    gx = z.next_center[0];
                    gy = z.next_center[1];
                }
                return ([gx, gy, pos[2]], "COMBAT", d > goal_r + 25.0);
            }
        }

        // Priority 7: Farm nearby cubes/crates/NPCs when safe and cubes < 10
        if want_cubes {
            if let Some(t) = target {
                if !t.is_mech {
                    // Strafe around NPC or crate at ~26 m
                    let dx = pos[0] - t.pos[0];
                    let dy = pos[1] - t.pos[1];
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    let strafe = if (obs.tick / 25) % 2 == 0 { 1.0 } else { -1.0 };
                    let gx = t.pos[0] + (dx / len) * 24.0 + (-dy / len) * strafe * 14.0;
                    let gy = t.pos[1] + (dy / len) * 24.0 + (dx / len) * strafe * 14.0;
                    return ([gx, gy, pos[2]], "FARM_FIGHT", false);
                }
            }
            // Move toward a remembered crate inside the zone
            let best_crate = self
                .world
                .crates
                .values()
                .filter(|c| {
                    dist2d(pos2, [c.pos[0], c.pos[1]]) < 95.0
                        && (c.pos[2] - pos[2]) < 12.0
                        && dist2d([c.pos[0], c.pos[1]], z.next_center) < z.next_radius
                })
                .min_by(|a, b| {
                    dist2d(pos2, [a.pos[0], a.pos[1]])
                        .partial_cmp(&dist2d(pos2, [b.pos[0], b.pos[1]]))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(cr) = best_crate {
                return (cr.pos, "SEEK_CRATE", false);
            }
        }

        // Priority 8: In Practice mode with nobody visible, hunt toward recent threat sounds (if healthy)
        if self.world.looks_like_practice_squad(obs.t) && hp_frac > 0.72 && me.empty_slot().is_none() {
            if let Some(s) = self.world.most_urgent_sound(obs.t) {
                let probe_d = match s.dist {
                    crate::protocol::DistBand::Near => 35.0,
                    crate::protocol::DistBand::Mid => 75.0,
                    crate::protocol::DistBand::Far => 110.0,
                };
                let gx = pos[0] + s.bearing.cos() * probe_d;
                let gy = pos[1] + s.bearing.sin() * probe_d;
                if dist2d([gx, gy], z.next_center) <= (z.next_radius - 10.0).max(12.0) {
                    return ([gx, gy, pos[2]], "HUNT_SND", false);
                }
            }
        }

        // Priority 9: Smart Roam inside next_center (patrol strong positions rather than standing still)
        let need_new_anchor = self.roam_anchor.is_none_or(|a| {
            dist2d(pos2, a) < 16.0
                || dist2d(a, z.next_center) > (z.next_radius * 0.72).max(10.0)
                || obs.tick.saturating_sub(self.roam_tick) > 120
        });
        if need_new_anchor {
            let r = (z.next_radius * 0.42).min(95.0);
            let angle = ((obs.tick as f64 * 0.13) + (me.id as f64 * 1.1)).rem_euclid(std::f64::consts::TAU);
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
        ([anchor[0], anchor[1], az], "PATROL", dist2d(pos2, z.next_center) > z.next_radius)
    }

    /// Evaluate 16 candidate movement directions + stay still for bullet/laser dodging, wall clearance, and goal progress.
    #[allow(clippy::too_many_arguments)]
    fn compute_micro_move(
        &self,
        obs: &Observation,
        waypoint: Vec2,
        goal_pos3: Vec3,
        target: Option<&CombatTarget>,
        desired_range: f64,
        urgent_zone: bool,
        want_regen_retreat: bool,
        stuck: bool,
    ) -> (Vec2, bool, bool) {
        let me = &obs.self_obs;
        let pos = me.pos;
        let pos2 = [pos[0], pos[1]];
        let scale = 1.0 + self.cfg.cubes.size_per_cube * (me.cubes as f64);
        let my_r = me.stat("radius", 2.5) * scale;
        let walk_spd = me.stat("walk_speed", 11.7);
        let z = &obs.zone;

        // If stuck, try jumping/jetting AND sliding orthogonally
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

        // Precompute charging enemy laser rays!
        let mut laser_threats: Vec<(Vec2, Vec2)> = Vec::new();
        for e in self.world.visible_enemies() {
            if e.charging {
                let dir = [e.aim[0].cos(), e.aim[0].sin()];
                laser_threats.push(([e.pos[0], e.pos[1]], dir));
            }
        }

        // Precompute incoming projectile hazards
        let mortar_g = self.cfg.weapons.mortar.get("gravity").copied().unwrap_or(22.0);
        let mortar_splash = self.cfg.weapons.mortar.get("splash_radius").copied().unwrap_or(7.0);
        let mut urgent_dodge = false;

        // Check if current position is directly in danger from a laser or projectile
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
        let step_dt = 0.35; // evaluate position ~0.35s ahead

        // Candidate 0 = stand still (useful when capturing drop or inside good cover), 1..=16 = 16 compass angles
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
            if wp_dist > 2.0 {
                let dot = dir[0] * wp_dir[0] + dir[1] * wp_dir[1];
                let w = if urgent_zone || want_regen_retreat {
                    28.0
                } else {
                    18.0
                };
                score += dot * w;
            } else if ci == 0 {
                score += 6.0;
            }

            // 2. Hard wall & map edge avoidance (at our current z height!)
            if self.map.loaded {
                let check_x = pos[0] + dir[0] * (my_r + 2.8);
                let check_y = pos[1] + dir[1] * (my_r + 2.8);
                if self.map.is_blocked_at(check_x, check_y, pos[2], my_r * 0.85) {
                    // Only allow pushing into wall if we're intentionally climbing onto a higher target
                    if goal_pos3[2] - pos[2] <= 1.3 {
                        score -= 70.0;
                    }
                }
                let (gx, gy) = self.map.world_to_nav(nx, ny);
                let wd = self.map.wall_dist_cell(gx, gy);
                if wd == 0 && pos[2] <= self.map.terrain_z(pos[0] as f32, pos[1] as f32) as f64 + 1.5 {
                    score -= 35.0;
                }
            }

            // 3. Zone safety boundary
            let nd_cur = dist2d([nx, ny], z.center);
            if nd_cur > z.radius - 4.0 {
                score -= (nd_cur - (z.radius - 4.0)) * 12.0;
            }
            if !me.in_zone {
                let d_before = dist2d(pos2, z.center);
                score += (d_before - nd_cur) * 8.0;
            }

            // 4. Incoming projectile dodging!
            for p in &obs.projectiles {
                if p.kind == "mortar" {
                    // Estimate mortar landing point
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
                                score -= (danger_r - d_land) * 28.0;
                                if dist2d(pos2, [lx, ly]) < danger_r {
                                    urgent_dodge = true;
                                }
                            }
                        }
                    }
                } else {
                    // Gun or NPC bullet: check closest approach in next 0.75s
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
                            let cz = (p.pos[2] + p.vel[2] * t_closest) - (pos[2] + 4.5 * scale);
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

            // 5. Charging enemy laser ray dodging! (HUGE survivability win!)
            for &(lo, ldir) in &laser_threats {
                let rx = nx - lo[0];
                let ry = ny - lo[1];
                let along = rx * ldir[0] + ry * ldir[1];
                if along > 0.0 && along < 280.0 {
                    let perp = (rx * ldir[1] - ry * ldir[0]).abs();
                    let danger = my_r + 6.5;
                    if perp < danger {
                        score -= (danger - perp) * 32.0;
                    }
                }
            }

            // 6. Combat micro-strafe & range discipline (when fighting a mech)
            if !urgent_zone && !want_regen_retreat {
                if let Some(t) = target {
                    if t.is_mech {
                        let nd = dist2d([nx, ny], [t.pos[0], t.pos[1]]);
                        let cur_d = dist2d(pos2, [t.pos[0], t.pos[1]]);
                        // Stay near desired range
                        score -= ((nd - desired_range).abs() - (cur_d - desired_range).abs()) * 0.9;
                        // Avoid standing still in front of an active mech
                        if ci == 0 {
                            score -= 10.0;
                        }
                        // Don't walk into enemy lightning range if we don't have lightning!
                        if !me.has_weapon(WeaponKind::Lightning) && nd < 46.0 {
                            score -= (46.0 - nd) * 1.4;
                        }
                    }
                }
            }

            // Smooth velocity continuity (reduces jittery zig-zag acceleration loss)
            let cur_spd = (me.vel[0] * me.vel[0] + me.vel[1] * me.vel[1]).sqrt();
            if cur_spd > 2.0 {
                let vdot = (dir[0] * me.vel[0] + dir[1] * me.vel[1]) / cur_spd;
                score += vdot * 2.5;
            }

            if score > best_score {
                best_score = score;
                best_dir = dir;
            }
        }

        // Check if a low ledge is directly ahead along our chosen direction
        let need_jump = self.map.loaded
            && goal_pos3[2] - pos[2] > 1.3
            && dist2d(pos2, [goal_pos3[0], goal_pos3[1]]) < 16.0;

        (best_dir, urgent_dodge, need_jump)
    }

    fn choose_torso_aim(
        &mut self,
        obs: &Observation,
        combat: &crate::combat::CombatDecision,
        move_dir: Vec2,
    ) -> Vec2 {
        // 1. Active combat target (including backwards retreat facing the attacker -> prevents 1.5x rear dmg!)
        if let Some(a) = combat.aim {
            return a;
        }

        let me = &obs.self_obs;
        // 2. If recently damaged from a known bearing, snap torso toward attacker immediately (front armor + spot!)
        if obs.t - self.world.last_damaged_t < 1.8 {
            if let Some(b) = self.world.last_damaged_bearing {
                return [b, 0.0];
            }
        }

        // 3. Look toward urgent nearby threat sound (laser_charge, lightning, gun, steps)
        if let Some(s) = self.world.most_urgent_sound(obs.t) {
            if obs.t - s.at_t < 1.2 {
                return [s.bearing, 0.0];
            }
        }

        // 4. Otherwise do an active sweeping scan around our travel direction (120 deg FOV -> covers 360 cleanly!)
        let base_yaw = if move_dir[0].abs() + move_dir[1].abs() > 0.2 {
            move_dir[1].atan2(move_dir[0])
        } else {
            me.aim[0]
        };
        let sweep = (obs.t * 1.45).sin() * 1.35; // +-77 deg sweep + 120 deg FOV = ~275 deg coverage
        [wrap_angle(base_yaw + sweep), -0.04]
    }
}
