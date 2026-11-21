use crate::map::{ArenaMap, NAV_STEP};
use crate::protocol::{dist2d, Vec2, Vec3};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, PartialEq)]
struct HeapNode {
    f: f32,
    idx: usize,
}

impl Eq for HeapNode {}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.idx.cmp(&other.idx))
    }
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct Navigator {
    g_score: Vec<f32>,
    came_from: Vec<u32>,
    stamp: Vec<u32>,
    cur_stamp: u32,
    path: Vec<Vec2>,
    path_idx: usize,
    cached_goal: Option<Vec2>,
    last_plan_tick: u64,
    // Stuck detection
    pos_history: Vec<(u64, Vec3)>,
    stuck_ticks: u32,
    unstuck_dir: Vec2,
}

impl Navigator {
    pub fn new(map: &ArenaMap) -> Self {
        let n = map.nav_nx * map.nav_ny;
        Self {
            g_score: vec![f32::INFINITY; n],
            came_from: vec![u32::MAX; n],
            stamp: vec![0; n],
            cur_stamp: 0,
            path: Vec::new(),
            path_idx: 0,
            cached_goal: None,
            last_plan_tick: 0,
            pos_history: Vec::with_capacity(30),
            stuck_ticks: 0,
            unstuck_dir: [1.0, 0.0],
        }
    }

    pub fn update_stuck(
        &mut self,
        map: &ArenaMap,
        tick: u64,
        pos: Vec3,
        wanted_move: bool,
        allow_loop_detect: bool,
    ) -> bool {
        self.pos_history.push((tick, pos));
        if self.pos_history.len() > 26 {
            self.pos_history.remove(0);
        }
        if !wanted_move {
            self.stuck_ticks = 0;
            return false;
        }
        let n = self.pos_history.len();
        let short_stall = n >= 8
            && dist2d(
                [pos[0], pos[1]],
                [self.pos_history[n - 8].1[0], self.pos_history[n - 8].1[1]],
            ) < 1.6;
        // ONLY check 2.4s oscillation loops during travel states (ZONE/PATROL/LOOT), NEVER during combat strafing or corner-peeking!
        let loop_stall = allow_loop_detect
            && n >= 24
            && dist2d([pos[0], pos[1]], [self.pos_history[0].1[0], self.pos_history[0].1[1]])
                < 8.5;
        if short_stall || loop_stall {
            self.stuck_ticks += 1;
            // Strictly rotate escape direction every 4 stuck ticks so we NEVER pin against a thin wall or cliff!
            if self.stuck_ticks == 1 || self.stuck_ticks % 4 == 0 {
                let base = if let Some(wp) = self.current_waypoint() {
                    (wp[1] - pos[1]).atan2(wp[0] - pos[0])
                } else {
                    0.0
                };
                let step_i = (self.stuck_ticks / 4) as usize;
                let mut chosen = None;
                for k in 0..8usize {
                    let ang = base
                        + ((step_i + k) as f64) * std::f64::consts::FRAC_PI_4
                        + std::f64::consts::FRAC_PI_2;
                    let dx = ang.cos();
                    let dy = ang.sin();
                    // Check BOTH near (2.0 m) and mid (4.2 m) so thin stadium walls & cliffs don't fool us!
                    if !map.is_blocked_at(pos[0] + dx * 2.0, pos[1] + dy * 2.0, pos[2], 1.8)
                        && !map.is_blocked_at(pos[0] + dx * 4.2, pos[1] + dy * 4.2, pos[2], 2.0)
                    {
                        chosen = Some([dx, dy]);
                        break;
                    }
                }
                let fallback_ang = base + (step_i as f64 + 1.0) * std::f64::consts::FRAC_PI_4;
                self.unstuck_dir =
                    chosen.unwrap_or([fallback_ang.cos(), fallback_ang.sin()]);
                self.cached_goal = None;
            }
            return true;
        }
        self.stuck_ticks = 0;
        false
    }

    pub fn stuck_ticks(&self) -> u32 {
        self.stuck_ticks
    }

    #[allow(dead_code)]
    pub fn is_stuck(&self) -> bool {
        self.stuck_ticks >= 1
    }

    pub fn unstuck_direction(&self) -> Vec2 {
        self.unstuck_dir
    }

    pub fn current_waypoint(&self) -> Option<Vec2> {
        self.path.get(self.path_idx).copied()
    }

    pub fn remaining_path(&self) -> &[Vec2] {
        if self.path_idx < self.path.len() {
            &self.path[self.path_idx..]
        } else {
            &[]
        }
    }

    /// Compute the next 2D waypoint toward `goal`, using cached smoothed A* paths.
    pub fn next_waypoint(&mut self, map: &ArenaMap, tick: u64, pos: Vec3, goal: Vec2) -> Vec2 {
        let cur2 = [pos[0], pos[1]];
        if !map.loaded {
            self.path.clear();
            return goal;
        }

        let current_cell = map.world_to_nav(cur2[0], cur2[1]);
        if !map.can_traverse_nav_step(current_cell, current_cell) {
            self.path.clear();
            self.cached_goal = None;
            // Conservative rasterization may exclude the current body position
            // even though a short physically clear route reaches a valid cell.
            return map.nearby_nav_recovery(pos).unwrap_or(cur2);
        }

        // Fast direct-line shortcut if completely clear!
        if dist2d(cur2, goal) < 120.0 && map.line_walkable(cur2, goal) {
            self.path.clear();
            self.path.push(goal);
            self.path_idx = 0;
            self.cached_goal = Some(goal);
            return goal;
        }

        // Advance along existing path
        while self.path_idx + 1 < self.path.len()
            && dist2d(cur2, self.path[self.path_idx]) < 5.5
            && map.line_walkable(cur2, self.path[self.path_idx + 1])
        {
            self.path_idx += 1;
        }

        let goal_moved = self
            .cached_goal
            .is_none_or(|g| dist2d(g, goal) > 8.0);
        let need_replan = self.path_idx >= self.path.len()
            || goal_moved
            || tick.saturating_sub(self.last_plan_tick) >= 12
            || self.stuck_ticks >= 2
            || (self.path_idx < self.path.len()
                && (dist2d(cur2, self.path[self.path_idx]) > 35.0
                    || !map.line_walkable(cur2, self.path[self.path_idx])));

        if need_replan {
            self.replan(map, cur2, goal);
            self.cached_goal = Some(goal);
            self.last_plan_tick = tick;
        }

        // Lookahead smoothing from current position
        while self.path_idx + 1 < self.path.len()
            && map.line_walkable(cur2, self.path[self.path_idx + 1])
        {
            self.path_idx += 1;
        }

        self.path.get(self.path_idx).copied().unwrap_or(cur2)
    }

    fn replan(&mut self, map: &ArenaMap, start: Vec2, goal: Vec2) {
        self.path.clear();
        self.path_idx = 0;

        let Some((sx, sy)) = map.nearest_walkable_cell(start[0], start[1], 12) else {
            self.path.push(start);
            return;
        };
        let Some((gx, gy)) = map.nearest_walkable_cell(goal[0], goal[1], 12) else {
            self.path.push(start);
            return;
        };
        if sx == gx && sy == gy {
            self.path.push(if map.line_walkable(start, goal) { goal } else { start });
            return;
        }

        let nx = map.nav_nx;
        let ny = map.nav_ny;
        let total = nx * ny;
        if self.stamp.len() != total {
            self.g_score = vec![f32::INFINITY; total];
            self.came_from = vec![u32::MAX; total];
            self.stamp = vec![0; total];
            self.cur_stamp = 0;
        }

        self.cur_stamp = self.cur_stamp.wrapping_add(1);
        if self.cur_stamp == 0 {
            self.stamp.fill(0);
            self.cur_stamp = 1;
        }
        let st = self.cur_stamp;

        let start_idx = sy * nx + sx;
        let goal_idx = gy * nx + gx;

        let mut heap = BinaryHeap::with_capacity(512);
        self.stamp[start_idx] = st;
        self.g_score[start_idx] = 0.0;
        self.came_from[start_idx] = u32::MAX;
        heap.push(HeapNode {
            f: octile(sx, sy, gx, gy),
            idx: start_idx,
        });

        let dirs: [(isize, isize, f32); 8] = [
            (1, 0, 1.0),
            (-1, 0, 1.0),
            (0, 1, 1.0),
            (0, -1, 1.0),
            (1, 1, 1.414_213_5),
            (-1, 1, 1.414_213_5),
            (1, -1, 1.414_213_5),
            (-1, -1, 1.414_213_5),
        ];

        let mut expanded = 0usize;
        let max_expansions = 12_000usize;
        let mut best_fallback_idx = start_idx;
        let mut best_fallback_h = octile(sx, sy, gx, gy);
        let mut found = false;

        while let Some(HeapNode { idx: cur, .. }) = heap.pop() {
            if cur == goal_idx {
                found = true;
                break;
            }
            expanded += 1;
            if expanded >= max_expansions {
                break;
            }

            let cx = cur % nx;
            let cy = cur / nx;
            let cg = self.g_score[cur];

            let ch = octile(cx, cy, gx, gy);
            if ch < best_fallback_h {
                best_fallback_h = ch;
                best_fallback_idx = cur;
            }

            for (dx, dy, base_cost) in dirs {
                let nxi = cx as isize + dx;
                let nyi = cy as isize + dy;
                if nxi < 0 || nyi < 0 || nxi as usize >= nx || nyi as usize >= ny {
                    continue;
                }
                let nxu = nxi as usize;
                let nyu = nyi as usize;
                let nidx = nyu * nx + nxu;
                if !map.can_traverse_nav_step((cx, cy), (nxu, nyu)) {
                    continue;
                }
                let wd = map.wall_dist[nidx];
                let penalty = match wd {
                    0 => continue,
                    1 => 0.45,
                    2 => 0.12,
                    _ => 0.0,
                };
                let ng = cg + base_cost + penalty;
                let prev_g = if self.stamp[nidx] == st {
                    self.g_score[nidx]
                } else {
                    f32::INFINITY
                };
                if ng < prev_g {
                    self.stamp[nidx] = st;
                    self.g_score[nidx] = ng;
                    self.came_from[nidx] = cur as u32;
                    let h = octile(nxu, nyu, gx, gy);
                    heap.push(HeapNode {
                        f: ng + h * 1.05,
                        idx: nidx,
                    });
                }
            }
        }

        let end_idx = if found { goal_idx } else { best_fallback_idx };
        if end_idx == start_idx {
            // No reachable progress: let the caller's stuck/jet recovery act,
            // rather than steering directly into the edge A* just rejected.
            self.path.push(start);
            return;
        }

        let mut rev = Vec::new();
        let mut cur = end_idx;
        let mut guard = 0;
        while cur != start_idx && cur != u32::MAX as usize && guard < 2048 {
            rev.push(map.nav_to_world(cur % nx, cur / nx));
            let prev = self.came_from[cur];
            if prev == u32::MAX {
                break;
            }
            cur = prev as usize;
            guard += 1;
        }
        rev.reverse();

        // String-pull / thin out collinear points
        let mut smoothed = Vec::with_capacity(rev.len() + 1);
        let mut anchor = start;
        // A* starts at the cell center. From an off-center position, its first
        // diagonal can cut a different (forbidden) edge. Re-center safely before
        // following that edge rather than assuming its world-space segment fits.
        if !map.line_walkable(start, rev[0]) {
            let center = map.nav_to_world(sx, sy);
            if !map.line_walkable(start, center) {
                self.path.push(start);
                return;
            }
            smoothed.push(center);
            anchor = center;
        }
        let mut i = 0;
        while i < rev.len() {
            let mut furthest = i;
            let limit = (i + 10).min(rev.len() - 1);
            for j in ((i + 1)..=limit).rev() {
                if map.line_walkable(anchor, rev[j]) {
                    furthest = j;
                    break;
                }
            }
            smoothed.push(rev[furthest]);
            anchor = rev[furthest];
            i = furthest + 1;
        }
        if found
            && smoothed.last().is_none_or(|p| dist2d(*p, goal) > NAV_STEP as f64)
            && map.line_walkable(anchor, goal)
        {
            smoothed.push(goal);
        }
        self.path = smoothed;
        self.path_idx = 0;
    }
}

#[inline]
fn octile(ax: usize, ay: usize, bx: usize, by: usize) -> f32 {
    let dx = (ax as isize - bx as isize).unsigned_abs() as f32;
    let dy = (ay as isize - by as isize).unsigned_abs() as f32;
    let (mn, mx) = if dx < dy { (dx, dy) } else { (dy, dx) };
    mx + 0.414_213_5 * mn
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    fn grid(nx: usize, ny: usize, heights: Vec<f32>) -> ArenaMap {
        let mut map = ArenaMap::empty();
        map.loaded = true;
        map.min_x = 0.0;
        map.min_y = 0.0;
        map.nav_nx = nx;
        map.nav_ny = ny;
        map.walkable = vec![true; nx * ny];
        map.wall_dist = vec![10; nx * ny];
        map.nav_z = heights;
        map
    }

    #[test]
    fn close_unreachable_goal_does_not_restore_rejected_cliff_shortcut() {
        let map = grid(2, 1, vec![0.0, 6.0]);
        let mut nav = Navigator::new(&map);
        assert_eq!(nav.next_waypoint(&map, 1, [2.0, 2.0, 0.0], [6.0, 2.0]), [2.0, 2.0]);
        assert_eq!(nav.next_waypoint(&map, 2, [6.0, 2.0, 6.0], [2.0, 2.0]), [2.0, 2.0]);
    }

    #[test]
    fn conservative_raster_start_can_recover_to_a_physically_clear_cell() {
        let mut map = grid(4, 3, vec![0.0; 12]);
        map.walkable[4] = false;
        map.wall_dist[4] = 0;
        let mut nav = Navigator::new(&map);
        let start = [3.5, 6.0, 0.0];
        let waypoint = nav.next_waypoint(&map, 1, start, [14.0, 6.0]);
        assert_eq!(waypoint, [6.0, 6.0]);
        assert!(!map.line_walkable([start[0], start[1]], waypoint));
        assert!(map.is_walkable_cell(1, 1));
    }

    #[test]
    fn smoothing_and_waypoint_advancement_keep_a_valid_detour() {
        let map = grid(3, 3, vec![0.0, 3.0, 6.0, 0.0, 6.0, 6.0, 0.0, 3.0, 6.0]);
        let mut nav = Navigator::new(&map);
        let goal = [10.0, 6.0];
        let first = nav.next_waypoint(&map, 1, [2.0, 6.0, 0.0], goal);
        assert_ne!(first, goal);
        let mut previous = [2.0, 6.0];
        for &point in nav.remaining_path() {
            assert!(map.line_walkable(previous, point));
            previous = point;
        }
        assert_eq!(previous, goal);
        // Being within the old 5.5 m waypoint radius cannot skip this bend.
        let next = nav.next_waypoint(&map, 2, [3.0, 6.0, 0.0], goal);
        assert_ne!(next, goal);
        assert!(map.line_walkable([3.0, 6.0], next));
    }

    #[test]
    fn real_arena_cliff_witness_is_not_a_direct_route() {
        let path = "kit/arena/v2/world.json";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let map = ArenaMap::load(&crate::protocol::HelloMap {
            dir: "kit/arena".into(), files: vec![],
        });
        assert!(map.loaded);
        let a = [-242.38, 48.91];
        let b = [-238.38, 48.91];
        let ac = map.world_to_nav(a[0], a[1]);
        let bc = map.world_to_nav(b[0], b[1]);
        assert!(map.is_walkable_cell(ac.0, ac.1));
        assert!(map.is_walkable_cell(bc.0, bc.1));
        assert!(!map.can_traverse_nav_step(ac, bc));
        assert!(!map.line_walkable(a, b));
        assert!(map.line_walkable(b, a));
        let mut nav = Navigator::new(&map);
        let start = [-246.38, 48.91];
        let waypoint = nav.next_waypoint(&map, 1, [start[0], start[1], -6.7], b);
        assert_ne!(waypoint, b);
        assert!(map.line_walkable(start, waypoint));
    }
}
