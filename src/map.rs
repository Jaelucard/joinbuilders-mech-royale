use crate::protocol::{dist2d, HelloMap, Vec2, Vec3};
use serde::Deserialize;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

const PRISM_CELL_SIZE: f32 = 16.0;
pub const NAV_STEP: f32 = 4.0;
pub const LOS_STEP: f32 = 2.0;
const MECH_CLEAR_RADIUS: f32 = 2.65;
const MECH_HEIGHT: f32 = 9.0;
const MECH_STEP_UP: f32 = 1.2;
const MAX_CLIMB_HEIGHT: f32 = 45.0;

#[derive(Deserialize)]
struct RawWorld {
    bounds: RawBounds,
    terrain: RawTerrain,
    #[serde(default)]
    prisms: Vec<RawPrism>,
}

#[derive(Deserialize)]
struct RawBounds {
    #[serde(rename = "minX")]
    min_x: f32,
    #[serde(rename = "maxX")]
    max_x: f32,
    #[serde(rename = "minY")]
    min_y: f32,
    #[serde(rename = "maxY")]
    max_y: f32,
}

#[derive(Deserialize)]
struct RawTerrain {
    #[serde(rename = "minX")]
    min_x: f32,
    #[serde(rename = "minY")]
    min_y: f32,
    step: f32,
    nx: usize,
    ny: usize,
    #[serde(default = "default_scale")]
    scale: f32,
}

fn default_scale() -> f32 {
    0.01
}

#[derive(Deserialize)]
struct RawPrism {
    base: f32,
    top: f32,
    #[serde(default)]
    ring: Vec<f32>,
    #[serde(default)]
    holes: Vec<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct Prism {
    pub base: f32,
    pub top: f32,
    pub ring: Vec<[f32; 2]>,
    pub holes: Vec<Vec<[f32; 2]>>,
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

impl Prism {
    fn from_raw(raw: RawPrism) -> Option<Self> {
        if raw.ring.len() < 6 {
            return None;
        }
        let mut ring = Vec::with_capacity(raw.ring.len() / 2);
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for chunk in raw.ring.chunks_exact(2) {
            let (x, y) = (chunk[0], chunk[1]);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            ring.push([x, y]);
        }
        let holes = raw
            .holes
            .into_iter()
            .filter(|h| h.len() >= 6)
            .map(|h| h.chunks_exact(2).map(|c| [c[0], c[1]]).collect())
            .collect();
        Some(Self {
            base: raw.base,
            top: raw.top,
            ring,
            holes,
            min_x,
            max_x,
            min_y,
            max_y,
        })
    }

    #[inline]
    pub fn contains_2d(&self, x: f32, y: f32) -> bool {
        if x < self.min_x || x > self.max_x || y < self.min_y || y > self.max_y {
            return false;
        }
        if !point_in_poly(x, y, &self.ring) {
            return false;
        }
        for h in &self.holes {
            if point_in_poly(x, y, h) {
                return false;
            }
        }
        true
    }

    pub fn intersects_segment_3d(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let seg_min_x = a[0].min(b[0]);
        let seg_max_x = a[0].max(b[0]);
        if seg_max_x < self.min_x || seg_min_x > self.max_x {
            return false;
        }
        let seg_min_y = a[1].min(b[1]);
        let seg_max_y = a[1].max(b[1]);
        if seg_max_y < self.min_y || seg_min_y > self.max_y {
            return false;
        }
        let seg_min_z = a[2].min(b[2]);
        let seg_max_z = a[2].max(b[2]);
        if seg_max_z < self.base || seg_min_z > self.top {
            return false;
        }

        let dz = b[2] - a[2];
        let (t_lo, t_hi) = if dz.abs() < 1e-6 {
            (0.0_f32, 1.0_f32)
        } else {
            let t0 = (self.base - a[2]) / dz;
            let t1 = (self.top - a[2]) / dz;
            let lo = t0.min(t1).clamp(0.0, 1.0);
            let hi = t0.max(t1).clamp(0.0, 1.0);
            if lo > hi {
                return false;
            }
            (lo, hi)
        };

        let dx = b[0] - a[0];
        let dy = b[1] - a[1];

        if self.contains_2d(a[0] + dx * t_lo, a[1] + dy * t_lo)
            || self.contains_2d(a[0] + dx * t_hi, a[1] + dy * t_hi)
        {
            return true;
        }

        let mut ts = [0.0_f32; 64];
        let mut count = 0usize;
        ts[count] = t_lo;
        count += 1;
        ts[count] = t_hi;
        count += 1;

        let mut push_poly = |poly: &[[f32; 2]]| {
            let n = poly.len();
            for i in 0..n {
                let p0 = poly[i];
                let p1 = poly[(i + 1) % n];
                if let Some(t) = seg_intersect_t(a[0], a[1], dx, dy, p0[0], p0[1], p1[0], p1[1]) {
                    if t > t_lo && t < t_hi {
                        if count < ts.len() {
                            ts[count] = t;
                            count += 1;
                        } else {
                            return true;
                        }
                    }
                }
            }
            false
        };

        if push_poly(&self.ring) {
            return true;
        }
        for h in &self.holes {
            if push_poly(h) {
                return true;
            }
        }

        let slice = &mut ts[..count];
        slice.sort_by(|u, v| u.partial_cmp(v).unwrap_or(std::cmp::Ordering::Equal));
        for w in slice.windows(2) {
            if w[1] - w[0] > 1e-5 {
                let tm = 0.5 * (w[0] + w[1]);
                if self.contains_2d(a[0] + dx * tm, a[1] + dy * tm) {
                    return true;
                }
            }
        }
        false
    }
}

#[inline]
fn point_in_poly(x: f32, y: f32, poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let xi = poly[i][0];
        let yi = poly[i][1];
        let xj = poly[j][0];
        let yj = poly[j][1];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[inline]
fn seg_intersect_t(
    ax: f32,
    ay: f32,
    dx: f32,
    dy: f32,
    p0x: f32,
    p0y: f32,
    p1x: f32,
    p1y: f32,
) -> Option<f32> {
    let ex = p1x - p0x;
    let ey = p1y - p0y;
    let denom = dx * ey - dy * ex;
    if denom.abs() < 1e-8 {
        return None;
    }
    let rx = p0x - ax;
    let ry = p0y - ay;
    let t = (rx * ey - ry * ex) / denom;
    let u = (rx * dy - ry * dx) / denom;
    if (-1e-5..=1.0 + 1e-5).contains(&t) && (-1e-5..=1.0 + 1e-5).contains(&u) {
        Some(t.clamp(0.0, 1.0))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RoofAnchor {
    pub roof_pos: Vec3,
    pub launch_pos: Vec3,
    pub climb_dz: f64,
}

pub struct ArenaMap {
    pub loaded: bool,
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
    // Terrain heightmap
    t_min_x: f32,
    t_min_y: f32,
    t_step: f32,
    t_nx: usize,
    t_ny: usize,
    terrain: Vec<f32>,
    // Solid prisms + spatial hash
    pub prisms: Vec<Prism>,
    p_nx: usize,
    p_ny: usize,
    prism_cells: Vec<Vec<u16>>,
    // Fast 2 m height-field for O(1) per-step 3D LoS & wall checks
    los_nx: usize,
    los_ny: usize,
    solid_top: Vec<f32>,
    elevated_prisms: Vec<u16>,
    // Precomputed 2D ground navigation grid (4 m cells)
    pub nav_nx: usize,
    pub nav_ny: usize,
    pub walkable: Vec<bool>,
    pub wall_dist: Vec<u8>,
    pub roof_top: Vec<f32>,
    pub roof_anchors: Vec<RoofAnchor>,
}

impl ArenaMap {
    pub fn empty() -> Self {
        Self {
            loaded: false,
            min_x: -512.0,
            max_x: 555.0,
            min_y: -333.0,
            max_y: 389.0,
            t_min_x: -513.0,
            t_min_y: -334.0,
            t_step: 1.0,
            t_nx: 1,
            t_ny: 1,
            terrain: vec![0.0],
            prisms: Vec::new(),
            p_nx: 1,
            p_ny: 1,
            prism_cells: vec![Vec::new()],
            los_nx: 1,
            los_ny: 1,
            solid_top: vec![0.0],
            elevated_prisms: Vec::new(),
            nav_nx: 1,
            nav_ny: 1,
            walkable: vec![true],
            wall_dist: vec![10],
            roof_top: vec![f32::NAN],
            roof_anchors: Vec::new(),
        }
    }

    pub fn load(map_info: &HelloMap) -> Self {
        let mut base_dir = PathBuf::from(&map_info.dir);
        if !base_dir.exists() && Path::new("/arena").exists() {
            base_dir = PathBuf::from("/arena");
        }
        let world_path = find_map_file(&base_dir, &map_info.files, "world.json", "v2/world.json");
        let terrain_path =
            find_map_file(&base_dir, &map_info.files, "terrain.i16", "v2/terrain.i16");

        let world_text = match fs::read_to_string(&world_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warn: failed to read {:?}: {}", world_path, e);
                return Self::empty();
            }
        };
        let raw: RawWorld = match serde_json::from_str(&world_text) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("warn: failed to parse world.json: {}", e);
                return Self::empty();
            }
        };

        let expected_verts = raw.terrain.nx * raw.terrain.ny;
        let terrain = match fs::read(&terrain_path) {
            Ok(bytes) if bytes.len() >= expected_verts * 2 => {
                let mut h = Vec::with_capacity(expected_verts);
                for chunk in bytes[..expected_verts * 2].chunks_exact(2) {
                    let v = i16::from_le_bytes([chunk[0], chunk[1]]);
                    h.push(v as f32 * raw.terrain.scale);
                }
                h
            }
            Ok(bytes) => {
                eprintln!(
                    "warn: terrain.i16 size mismatch ({} bytes, expected {})",
                    bytes.len(),
                    expected_verts * 2
                );
                vec![0.0; expected_verts]
            }
            Err(e) => {
                eprintln!("warn: failed to read {:?}: {}", terrain_path, e);
                vec![0.0; expected_verts]
            }
        };

        let prisms: Vec<Prism> = raw.prisms.into_iter().filter_map(Prism::from_raw).collect();

        let min_x = raw.bounds.min_x;
        let max_x = raw.bounds.max_x;
        let min_y = raw.bounds.min_y;
        let max_y = raw.bounds.max_y;

        let p_nx = (((max_x - min_x) / PRISM_CELL_SIZE).ceil() as usize).max(1);
        let p_ny = (((max_y - min_y) / PRISM_CELL_SIZE).ceil() as usize).max(1);
        let mut prism_cells = vec![Vec::new(); p_nx * p_ny];
        for (idx, p) in prisms.iter().enumerate() {
            let ix0 = (((p.min_x - min_x) / PRISM_CELL_SIZE).floor() as isize)
                .clamp(0, p_nx as isize - 1) as usize;
            let ix1 = (((p.max_x - min_x) / PRISM_CELL_SIZE).floor() as isize)
                .clamp(0, p_nx as isize - 1) as usize;
            let iy0 = (((p.min_y - min_y) / PRISM_CELL_SIZE).floor() as isize)
                .clamp(0, p_ny as isize - 1) as usize;
            let iy1 = (((p.max_y - min_y) / PRISM_CELL_SIZE).floor() as isize)
                .clamp(0, p_ny as isize - 1) as usize;
            for iy in iy0..=iy1 {
                for ix in ix0..=ix1 {
                    prism_cells[iy * p_nx + ix].push(idx as u16);
                }
            }
        }

        let los_nx = (((max_x - min_x) / LOS_STEP).ceil() as usize).max(1);
        let los_ny = (((max_y - min_y) / LOS_STEP).ceil() as usize).max(1);
        let nav_nx = (((max_x - min_x) / NAV_STEP).ceil() as usize).max(1);
        let nav_ny = (((max_y - min_y) / NAV_STEP).ceil() as usize).max(1);

        let mut map = Self {
            loaded: true,
            min_x,
            max_x,
            min_y,
            max_y,
            t_min_x: raw.terrain.min_x,
            t_min_y: raw.terrain.min_y,
            t_step: raw.terrain.step,
            t_nx: raw.terrain.nx,
            t_ny: raw.terrain.ny,
            terrain,
            prisms,
            p_nx,
            p_ny,
            prism_cells,
            los_nx,
            los_ny,
            solid_top: vec![0.0; los_nx * los_ny],
            elevated_prisms: Vec::new(),
            nav_nx,
            nav_ny,
            walkable: vec![false; nav_nx * nav_ny],
            wall_dist: vec![0; nav_nx * nav_ny],
            roof_top: vec![f32::NAN; nav_nx * nav_ny],
            roof_anchors: Vec::new(),
        };
        map.build_los_grid();
        map.build_nav_grid();
        map.build_roof_anchors();
        map
    }

    fn build_los_grid(&mut self) {
        // 1. Seed solid_top with terrain elevation
        for iy in 0..self.los_ny {
            let wy = self.min_y + (iy as f32 + 0.5) * LOS_STEP;
            let row = iy * self.los_nx;
            for ix in 0..self.los_nx {
                let wx = self.min_x + (ix as f32 + 0.5) * LOS_STEP;
                self.solid_top[row + ix] = self.terrain_z(wx, wy);
            }
        }

        // 2. Rasterize every solid prism into its AABB; track elevated bridges separately
        for (pidx, p) in self.prisms.iter().enumerate() {
            let ix0 = (((p.min_x - self.min_x) / LOS_STEP).floor() as isize)
                .clamp(0, self.los_nx as isize - 1) as usize;
            let ix1 = (((p.max_x - self.min_x) / LOS_STEP).ceil() as isize)
                .clamp(0, self.los_nx as isize - 1) as usize;
            let iy0 = (((p.min_y - self.min_y) / LOS_STEP).floor() as isize)
                .clamp(0, self.los_ny as isize - 1) as usize;
            let iy1 = (((p.max_y - self.min_y) / LOS_STEP).ceil() as isize)
                .clamp(0, self.los_ny as isize - 1) as usize;

            let mut any_elevated = false;
            for iy in iy0..=iy1 {
                let wy = self.min_y + (iy as f32 + 0.5) * LOS_STEP;
                let row = iy * self.los_nx;
                for ix in ix0..=ix1 {
                    let wx = self.min_x + (ix as f32 + 0.5) * LOS_STEP;
                    if p.contains_2d(wx, wy) {
                        let tz = self.terrain_z(wx, wy);
                        if p.base <= tz + 2.5 {
                            if p.top > self.solid_top[row + ix] {
                                self.solid_top[row + ix] = p.top;
                            }
                        } else {
                            any_elevated = true;
                        }
                    }
                }
            }
            if any_elevated {
                self.elevated_prisms.push(pidx as u16);
            }
        }
    }

    fn build_nav_grid(&mut self) {
        let margin = 4.0_f32;
        let max_slope_tan = 47.0_f32.to_radians().tan();
        let offsets = [
            [0.0_f32, 0.0],
            [MECH_CLEAR_RADIUS, 0.0],
            [-MECH_CLEAR_RADIUS, 0.0],
            [0.0, MECH_CLEAR_RADIUS],
            [0.0, -MECH_CLEAR_RADIUS],
            [MECH_CLEAR_RADIUS * 0.7, MECH_CLEAR_RADIUS * 0.7],
            [-MECH_CLEAR_RADIUS * 0.7, MECH_CLEAR_RADIUS * 0.7],
            [MECH_CLEAR_RADIUS * 0.7, -MECH_CLEAR_RADIUS * 0.7],
            [-MECH_CLEAR_RADIUS * 0.7, -MECH_CLEAR_RADIUS * 0.7],
        ];

        for gy in 0..self.nav_ny {
            let wy = self.min_y + (gy as f32 + 0.5) * NAV_STEP;
            for gx in 0..self.nav_nx {
                let wx = self.min_x + (gx as f32 + 0.5) * NAV_STEP;
                let idx = gy * self.nav_nx + gx;

                if wx < self.min_x + margin
                    || wx > self.max_x - margin
                    || wy < self.min_y + margin
                    || wy > self.max_y - margin
                {
                    self.walkable[idx] = false;
                    continue;
                }

                let z0 = self.terrain_z(wx, wy);
                let dz_x =
                    (self.terrain_z(wx + 2.0, wy) - self.terrain_z(wx - 2.0, wy)).abs() * 0.25;
                let dz_y =
                    (self.terrain_z(wx, wy + 2.0) - self.terrain_z(wx, wy - 2.0)).abs() * 0.25;
                let slope = (dz_x * dz_x + dz_y * dz_y).sqrt();
                if slope > max_slope_tan {
                    self.walkable[idx] = false;
                    continue;
                }

                let mut blocked = false;
                let mut highest_roof = f32::NAN;
                for [ox, oy] in offsets {
                    let px = wx + ox;
                    let py = wy + oy;
                    if let Some(cell) = self.prism_cell_at(px, py) {
                        for &pidx in cell {
                            let p = &self.prisms[pidx as usize];
                            if p.contains_2d(px, py)
                                && p.base < z0 + MECH_HEIGHT + 0.2
                                && p.top > z0 + MECH_STEP_UP
                            {
                                blocked = true;
                                if ox == 0.0
                                    && oy == 0.0
                                    && p.top - z0 <= MAX_CLIMB_HEIGHT
                                    && (highest_roof.is_nan() || p.top > highest_roof)
                                {
                                    highest_roof = p.top;
                                }
                            }
                        }
                    }
                    if blocked && !highest_roof.is_nan() {
                        break;
                    }
                }
                self.walkable[idx] = !blocked;
                self.roof_top[idx] = highest_roof;
            }
        }

        let mut q = VecDeque::new();
        for i in 0..self.walkable.len() {
            if !self.walkable[i] {
                self.wall_dist[i] = 0;
                q.push_back(i);
            } else {
                self.wall_dist[i] = u8::MAX;
            }
        }
        let dirs: [(isize, isize); 8] = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, 1),
            (1, -1),
            (-1, -1),
        ];
        while let Some(cur) = q.pop_front() {
            let d = self.wall_dist[cur];
            if d >= 20 {
                continue;
            }
            let cx = (cur % self.nav_nx) as isize;
            let cy = (cur / self.nav_nx) as isize;
            for (dx, dy) in dirs {
                let nx = cx + dx;
                let ny = cy + dy;
                if nx >= 0 && ny >= 0 && (nx as usize) < self.nav_nx && (ny as usize) < self.nav_ny
                {
                    let nidx = ny as usize * self.nav_nx + nx as usize;
                    if self.wall_dist[nidx] > d + 1 {
                        self.wall_dist[nidx] = d + 1;
                        q.push_back(nidx);
                    }
                }
            }
        }
    }

    fn build_roof_anchors(&mut self) {
        // Identify climbable roof edge cells (6..26 m above ground) that have flat interior roof space
        // and an immediately adjacent walkable ground launch cell.
        let card = [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)];
        let mut last_anchors: Vec<Vec2> = Vec::new();

        for gy in 2..self.nav_ny.saturating_sub(2) {
            for gx in 2..self.nav_nx.saturating_sub(2) {
                let idx = gy * self.nav_nx + gx;
                let rt = self.roof_top[idx];
                if rt.is_nan() {
                    continue;
                }
                let rw = self.nav_to_world(gx, gy);
                let gz = self.terrain_z(rw[0] as f32, rw[1] as f32);
                let dz = (rt - gz) as f64;
                if !(6.0..=26.0).contains(&dz) {
                    continue;
                }

                // Require at least 4 nearby roof cells at same elevation (spacious roof, not a tiny pillar!)
                let mut flat_neighbors = 0;
                for dy in -1..=1isize {
                    for dx in -1..=1isize {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nidx = (gy as isize + dy) as usize * self.nav_nx
                            + (gx as isize + dx) as usize;
                        let nrt = self.roof_top[nidx];
                        if !nrt.is_nan() && (nrt - rt).abs() < 1.5 {
                            flat_neighbors += 1;
                        }
                    }
                }
                if flat_neighbors < 4 {
                    continue;
                }

                // Must have an adjacent ground launch cell (within 1-2 cells)
                let mut launch = None;
                for &(dx, dy) in &card {
                    let lx = (gx as isize + dx) as usize;
                    let ly = (gy as isize + dy) as usize;
                    if self.is_walkable_cell(lx, ly) {
                        let lw = self.nav_to_world(lx, ly);
                        let lz = self.terrain_z(lw[0] as f32, lw[1] as f32) as f64;
                        launch = Some([lw[0], lw[1], lz]);
                        break;
                    }
                }
                let Some(launch_pos) = launch else { continue };

                // Thin out anchors to at least 20 m apart
                if last_anchors.iter().any(|p| dist2d(*p, rw) < 20.0) {
                    continue;
                }
                last_anchors.push(rw);
                self.roof_anchors.push(RoofAnchor {
                    roof_pos: [rw[0], rw[1], rt as f64],
                    launch_pos,
                    climb_dz: dz,
                });
            }
        }
    }

    #[inline]
    pub fn terrain_z(&self, x: f32, y: f32) -> f32 {
        if !self.loaded || self.t_nx < 2 || self.t_ny < 2 {
            return 0.0;
        }
        let fx = ((x - self.t_min_x) / self.t_step).clamp(0.0, (self.t_nx - 1) as f32 - 1e-4);
        let fy = ((y - self.t_min_y) / self.t_step).clamp(0.0, (self.t_ny - 1) as f32 - 1e-4);
        let ix = fx.floor() as usize;
        let iy = fy.floor() as usize;
        let tx = fx - ix as f32;
        let ty = fy - iy as f32;
        let row0 = iy * self.t_nx;
        let row1 = (iy + 1) * self.t_nx;
        let h00 = self.terrain[row0 + ix];
        let h10 = self.terrain[row0 + ix + 1];
        let h01 = self.terrain[row1 + ix];
        let h11 = self.terrain[row1 + ix + 1];
        let h0 = h00 + (h10 - h00) * tx;
        let h1 = h01 + (h11 - h01) * tx;
        h0 + (h1 - h0) * ty
    }

    #[inline]
    pub fn solid_top_at(&self, x: f32, y: f32) -> f32 {
        if !self.loaded {
            return 0.0;
        }
        let ix = (((x - self.min_x) / LOS_STEP).floor() as isize)
            .clamp(0, self.los_nx as isize - 1) as usize;
        let iy = (((y - self.min_y) / LOS_STEP).floor() as isize)
            .clamp(0, self.los_ny as isize - 1) as usize;
        self.solid_top[iy * self.los_nx + ix]
    }

    #[inline]
    pub fn is_on_roof(&self, pos: Vec3) -> bool {
        if !self.loaded {
            return false;
        }
        pos[2] > self.terrain_z(pos[0] as f32, pos[1] as f32) as f64 + 3.0
    }

    #[inline]
    fn prism_cell_at(&self, x: f32, y: f32) -> Option<&[u16]> {
        if !self.loaded {
            return None;
        }
        let ix = ((x - self.min_x) / PRISM_CELL_SIZE).floor() as isize;
        let iy = ((y - self.min_y) / PRISM_CELL_SIZE).floor() as isize;
        if ix < 0 || iy < 0 || ix as usize >= self.p_nx || iy as usize >= self.p_ny {
            return None;
        }
        Some(&self.prism_cells[iy as usize * self.p_nx + ix as usize])
    }

    #[inline]
    pub fn world_to_nav(&self, x: f64, y: f64) -> (usize, usize) {
        let gx = (((x as f32 - self.min_x) / NAV_STEP).floor() as isize)
            .clamp(0, self.nav_nx as isize - 1) as usize;
        let gy = (((y as f32 - self.min_y) / NAV_STEP).floor() as isize)
            .clamp(0, self.nav_ny as isize - 1) as usize;
        (gx, gy)
    }

    #[inline]
    pub fn nav_to_world(&self, gx: usize, gy: usize) -> [f64; 2] {
        [
            (self.min_x + (gx as f32 + 0.5) * NAV_STEP) as f64,
            (self.min_y + (gy as f32 + 0.5) * NAV_STEP) as f64,
        ]
    }

    #[inline]
    pub fn is_walkable_cell(&self, gx: usize, gy: usize) -> bool {
        if gx >= self.nav_nx || gy >= self.nav_ny {
            return false;
        }
        self.walkable[gy * self.nav_nx + gx]
    }

    #[inline]
    pub fn wall_dist_cell(&self, gx: usize, gy: usize) -> u8 {
        if gx >= self.nav_nx || gy >= self.nav_ny {
            return 0;
        }
        self.wall_dist[gy * self.nav_nx + gx]
    }

    /// Fast wall check at `(x, y, feet_z)` using the 2 m height-field + elevated bridges.
    #[inline]
    pub fn is_blocked_at(&self, x: f64, y: f64, feet_z: f64, radius: f64) -> bool {
        if !self.loaded {
            return false;
        }
        let xf = x as f32;
        let yf = y as f32;
        let zf = feet_z as f32;
        let rf = radius as f32;
        if xf < self.min_x + rf
            || xf > self.max_x - rf
            || yf < self.min_y + rf
            || yf > self.max_y - rf
        {
            return true;
        }
        let offsets = [[0.0, 0.0], [rf, 0.0], [-rf, 0.0], [0.0, rf], [0.0, -rf]];
        for [ox, oy] in offsets {
            if self.solid_top_at(xf + ox, yf + oy) > zf + MECH_STEP_UP {
                return true;
            }
        }
        // Also check the handful of elevated bridges
        for &pidx in &self.elevated_prisms {
            let p = &self.prisms[pidx as usize];
            if p.base < zf + MECH_HEIGHT
                && p.top > zf + MECH_STEP_UP
                && xf + rf >= p.min_x
                && xf - rf <= p.max_x
                && yf + rf >= p.min_y
                && yf - rf <= p.max_y
                && p.contains_2d(xf, yf)
            {
                return true;
            }
        }
        false
    }

    /// Ultra-fast 3D Line-of-Sight raymarch on the 2 m height-field (~150 ns per ray!).
    pub fn has_los(&self, a: Vec3, b: Vec3) -> bool {
        if !self.loaded {
            return true;
        }
        let ax = a[0] as f32;
        let ay = a[1] as f32;
        let az = a[2] as f32;
        let dx = b[0] as f32 - ax;
        let dy = b[1] as f32 - ay;
        let dz = b[2] as f32 - az;
        let horiz = (dx * dx + dy * dy).sqrt();
        if horiz < 1.8 {
            return true;
        }

        // March at 1.8 m spacing, skipping 1.2 m around endpoints so roof lips/corners don't self-hit
        let t_margin = (1.2 / horiz).min(0.45);
        let t0 = t_margin;
        let t1 = 1.0 - t_margin;
        let steps = (((horiz * (t1 - t0)) / 1.8).ceil() as usize).clamp(1, 220);
        let dt = (t1 - t0) / steps as f32;
        let inv_step = 1.0 / LOS_STEP;
        let max_ix = (self.los_nx - 1) as isize;
        let max_iy = (self.los_ny - 1) as isize;

        for i in 0..=steps {
            let t = t0 + dt * i as f32;
            let px = ax + dx * t;
            let py = ay + dy * t;
            let pz = az + dz * t;
            let ix = (((px - self.min_x) * inv_step).floor() as isize).clamp(0, max_ix) as usize;
            let iy = (((py - self.min_y) * inv_step).floor() as isize).clamp(0, max_iy) as usize;
            if pz < self.solid_top[iy * self.los_nx + ix] - 0.25 {
                return false;
            }
        }

        // Check elevated bridges (typically <= 9 across the entire map)
        if !self.elevated_prisms.is_empty() {
            let af = [ax, ay, az];
            let bf = [b[0] as f32, b[1] as f32, b[2] as f32];
            for &pidx in &self.elevated_prisms {
                if self.prisms[pidx as usize].intersects_segment_3d(af, bf) {
                    return false;
                }
            }
        }
        true
    }

    /// Find the best cover cell (including our current cell!) to break LoS from `threats`.
    /// Works on BOTH ground and rooftops!
    pub fn find_cover_pos(
        &self,
        from: Vec3,
        threats: &[Vec3],
        zone_center: Vec2,
        zone_radius: f64,
        max_search_r: f64,
    ) -> Option<Vec3> {
        if !self.loaded || threats.is_empty() {
            return None;
        }
        let on_roof = self.is_on_roof(from);
        let (sx, sy) = self.world_to_nav(from[0], from[1]);
        let rad_cells = (max_search_r as f32 / NAV_STEP).ceil() as isize;
        let mut best: Option<(f64, Vec3)> = None;
        let threats_cap = &threats[..threats.len().min(4)];

        for dy in -rad_cells..=rad_cells {
            let gy = sy as isize + dy;
            if gy < 0 || gy as usize >= self.nav_ny {
                continue;
            }
            for dx in -rad_cells..=rad_cells {
                let gx = sx as isize + dx;
                if gx < 0 || gx as usize >= self.nav_nx {
                    continue;
                }
                let gxu = gx as usize;
                let gyu = gy as usize;
                let idx = gyu * self.nav_nx + gxu;

                let cz: f64;
                let is_cover_edge: bool;
                if on_roof {
                    let rt = self.roof_top[idx];
                    if !rt.is_nan() && (rt as f64 - from[2]).abs() < 1.8 {
                        cz = rt as f64;
                        is_cover_edge = true;
                    } else if self.walkable[idx] && self.wall_dist[idx] <= 2 {
                        // Also allow dropping off roof behind building wall
                        let w = self.nav_to_world(gxu, gyu);
                        cz = self.terrain_z(w[0] as f32, w[1] as f32) as f64;
                        is_cover_edge = true;
                    } else {
                        continue;
                    }
                } else {
                    if !self.walkable[idx] {
                        continue;
                    }
                    let wd = self.wall_dist[idx];
                    if wd == 0 || wd > 3 {
                        continue;
                    }
                    let w = self.nav_to_world(gxu, gyu);
                    cz = self.terrain_z(w[0] as f32, w[1] as f32) as f64;
                    is_cover_edge = wd == 1;
                }

                let w = self.nav_to_world(gxu, gyu);
                let dist_me = dist2d(w, [from[0], from[1]]);
                if dist_me > max_search_r {
                    continue;
                }
                let dist_zone = dist2d(w, zone_center);
                if dist_zone > (zone_radius - 6.0).max(5.0) {
                    continue;
                }

                // Check center-of-mass AND eye height so we are truly hidden (not exposing our head!)
                let candidate_eye = [w[0], w[1], cz + 7.5];
                let mut exposed = 0usize;
                let mut min_threat_dist = f64::INFINITY;
                for &th in threats_cap {
                    let td = dist2d(w, [th[0], th[1]]);
                    min_threat_dist = min_threat_dist.min(td);
                    if self.has_los(candidate_eye, th) {
                        exposed += 1;
                    }
                }

                // Strong stability bonus for staying put when ALREADY safely hidden!
                let stay_bonus = if exposed == 0 && dist_me < 5.0 { 10.0 } else { 0.0 };
                let roof_bonus = if on_roof && (cz - from[2]).abs() < 1.8 { 14.0 } else { 0.0 };

                let score = -(exposed as f64) * 240.0
                    - dist_me * 1.45
                    + min_threat_dist.min(75.0) * 0.45
                    - (dist_zone / zone_radius.max(10.0)) * 10.0
                    + if is_cover_edge { 4.0 } else { 0.0 }
                    + stay_bonus
                    + roof_bonus;

                if best.as_ref().is_none_or(|(bs, _)| score > *bs) {
                    best = Some((score, [w[0], w[1], cz]));
                }
            }
        }
        best.map(|(_, pos)| pos)
    }

    /// Slice-the-pie: find a nearby spot that has LoS ONLY to `focus_eye` while buildings hide us from `other_threats`.
    pub fn find_slice_pie_pos(
        &self,
        from: Vec3,
        focus_eye: Vec3,
        other_threats: &[Vec3],
        zone_center: Vec2,
        zone_radius: f64,
        desired_range: f64,
    ) -> Option<Vec3> {
        if !self.loaded || other_threats.is_empty() {
            return None;
        }
        let on_roof = self.is_on_roof(from);
        let (sx, sy) = self.world_to_nav(from[0], from[1]);
        let rad_cells = 7isize; // 28 m search
        let others = &other_threats[..other_threats.len().min(4)];
        let mut best: Option<(f64, Vec3)> = None;

        for dy in -rad_cells..=rad_cells {
            let gy = sy as isize + dy;
            if gy < 0 || gy as usize >= self.nav_ny {
                continue;
            }
            for dx in -rad_cells..=rad_cells {
                let gx = sx as isize + dx;
                if gx < 0 || gx as usize >= self.nav_nx {
                    continue;
                }
                let gxu = gx as usize;
                let gyu = gy as usize;
                let idx = gyu * self.nav_nx + gxu;
                let cz = if on_roof {
                    let rt = self.roof_top[idx];
                    if !rt.is_nan() && (rt as f64 - from[2]).abs() < 1.8 {
                        rt as f64
                    } else {
                        continue;
                    }
                } else {
                    if !self.walkable[idx] || self.wall_dist[idx] > 2 {
                        continue;
                    }
                    let w = self.nav_to_world(gxu, gyu);
                    self.terrain_z(w[0] as f32, w[1] as f32) as f64
                };

                let w = self.nav_to_world(gxu, gyu);
                if dist2d(w, zone_center) > (zone_radius - 6.0).max(5.0) {
                    continue;
                }
                let dist_me = dist2d(w, [from[0], from[1]]);
                if dist_me > 28.0 {
                    continue;
                }
                let eye = [w[0], w[1], cz + 7.5];
                if !self.has_los(eye, focus_eye) {
                    continue;
                }
                let mut exposed_others = 0usize;
                for &th in others {
                    if self.has_los(eye, th) {
                        exposed_others += 1;
                    }
                }
                if exposed_others >= others.len() {
                    continue; // doesn't isolate anyone
                }
                let d_focus = dist2d(w, [focus_eye[0], focus_eye[1]]);
                let score = -(exposed_others as f64) * 180.0
                    - dist_me * 1.1
                    - (d_focus - desired_range).abs() * 0.4;
                if best.as_ref().is_none_or(|(bs, _)| score > *bs) {
                    best = Some((score, [w[0], w[1], cz]));
                }
            }
        }
        best.map(|(_, pos)| pos)
    }

    /// Find a `(hide_pos, peek_pos)` corner pair near `from` against `enemy_eye` for laser pre-charge peeking!
    pub fn find_peek_pair(
        &self,
        from: Vec3,
        enemy_eye: Vec3,
        zone_center: Vec2,
        zone_radius: f64,
    ) -> Option<(Vec3, Vec3)> {
        if !self.loaded {
            return None;
        }
        let on_roof = self.is_on_roof(from);
        let (sx, sy) = self.world_to_nav(from[0], from[1]);
        let rad = 5isize; // 20 m
        let card = [(1isize, 0isize), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)];
        let mut best: Option<(f64, Vec3, Vec3)> = None;

        for dy in -rad..=rad {
            let gy = sy as isize + dy;
            if gy < 1 || gy as usize + 1 >= self.nav_ny {
                continue;
            }
            for dx in -rad..=rad {
                let gx = sx as isize + dx;
                if gx < 1 || gx as usize + 1 >= self.nav_nx {
                    continue;
                }
                let gxu = gx as usize;
                let gyu = gy as usize;
                let idx = gyu * self.nav_nx + gxu;
                let cz = if on_roof {
                    let rt = self.roof_top[idx];
                    if !rt.is_nan() && (rt as f64 - from[2]).abs() < 1.8 {
                        rt as f64
                    } else {
                        continue;
                    }
                } else {
                    if !self.walkable[idx] || self.wall_dist[idx] != 1 {
                        continue;
                    }
                    let w = self.nav_to_world(gxu, gyu);
                    self.terrain_z(w[0] as f32, w[1] as f32) as f64
                };

                let w = self.nav_to_world(gxu, gyu);
                let dist_me = dist2d(w, [from[0], from[1]]);
                if dist_me > 18.0 || dist2d(w, zone_center) > (zone_radius - 6.0).max(5.0) {
                    continue;
                }
                let hide_eye = [w[0], w[1], cz + 7.5];
                if self.has_los(hide_eye, enemy_eye) {
                    continue; // not hidden!
                }

                // Look for an immediately adjacent peek cell (1 or 2 steps away) that HAS LoS!
                for &(pdx, pdy) in &card {
                    for mult in [1isize, 2] {
                        let px = gx + pdx * mult;
                        let py = gy + pdy * mult;
                        if px < 0 || py < 0 || px as usize >= self.nav_nx || py as usize >= self.nav_ny {
                            continue;
                        }
                        let pidx = py as usize * self.nav_nx + px as usize;
                        let pcz = if on_roof {
                            let rt = self.roof_top[pidx];
                            if !rt.is_nan() && (rt as f64 - from[2]).abs() < 1.8 {
                                rt as f64
                            } else {
                                continue;
                            }
                        } else {
                            if !self.walkable[pidx] {
                                continue;
                            }
                            let pw = self.nav_to_world(px as usize, py as usize);
                            self.terrain_z(pw[0] as f32, pw[1] as f32) as f64
                        };
                        let pw = self.nav_to_world(px as usize, py as usize);
                        let peek_eye = [pw[0], pw[1], pcz + 7.0];
                        if self.has_los(peek_eye, enemy_eye) {
                            let sep = dist2d(w, pw);
                            let score = -dist_me * 1.4 - sep * 2.5;
                            if best.as_ref().is_none_or(|(bs, _, _)| score > *bs) {
                                best = Some((score, [w[0], w[1], cz], [pw[0], pw[1], pcz]));
                            }
                            break;
                        }
                    }
                }
            }
        }
        best.map(|(_, h, p)| (h, p))
    }

    /// Find a nearby climbable rooftop anchor inside the safe next zone.
    pub fn find_nearby_rooftop(
        &self,
        from: Vec3,
        zone_center: Vec2,
        zone_radius: f64,
        max_dist: f64,
        max_climb_dz: f64,
    ) -> Option<RoofAnchor> {
        if !self.loaded {
            return None;
        }
        let mut best: Option<(f64, RoofAnchor)> = None;
        for &a in &self.roof_anchors {
            if a.climb_dz > max_climb_dz {
                continue;
            }
            let d_me = dist2d([from[0], from[1]], [a.launch_pos[0], a.launch_pos[1]]);
            if d_me > max_dist {
                continue;
            }
            let d_zone = dist2d([a.roof_pos[0], a.roof_pos[1]], zone_center);
            if d_zone > (zone_radius - 12.0).max(8.0) {
                continue;
            }
            // Prefer medium height (~12-18m: cheap energy to climb, great cover!) near zone center
            let h_score = 20.0 - (a.climb_dz - 14.0).abs();
            let score = h_score * 1.5 - d_me * 0.9 - (d_zone / zone_radius.max(10.0)) * 25.0;
            if best.as_ref().is_none_or(|(bs, _)| score > *bs) {
                best = Some((score, a));
            }
        }
        best.map(|(_, a)| a)
    }

    pub fn nearest_walkable_cell(&self, x: f64, y: f64, max_radius: usize) -> Option<(usize, usize)> {
        let (sx, sy) = self.world_to_nav(x, y);
        if self.is_walkable_cell(sx, sy) {
            return Some((sx, sy));
        }
        for r in 1..=max_radius as isize {
            let mut best: Option<(f64, usize, usize)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let gx = sx as isize + dx;
                    let gy = sy as isize + dy;
                    if gx >= 0
                        && gy >= 0
                        && (gx as usize) < self.nav_nx
                        && (gy as usize) < self.nav_ny
                        && self.is_walkable_cell(gx as usize, gy as usize)
                    {
                        let w = self.nav_to_world(gx as usize, gy as usize);
                        let d2 = (w[0] - x).powi(2) + (w[1] - y).powi(2);
                        if best.is_none_or(|(bd, _, _)| d2 < bd) {
                            best = Some((d2, gx as usize, gy as usize));
                        }
                    }
                }
            }
            if let Some((_, gx, gy)) = best {
                return Some((gx, gy));
            }
        }
        None
    }

    pub fn line_walkable(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let d = (dx * dx + dy * dy).sqrt();
        let steps = ((d / (NAV_STEP as f64 * 0.5)).ceil() as usize).max(1);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let (gx, gy) = self.world_to_nav(a[0] + dx * t, a[1] + dy * t);
            if !self.is_walkable_cell(gx, gy) {
                return false;
            }
        }
        true
    }
}

fn find_map_file(dir: &Path, files: &[String], suffix: &str, fallback: &str) -> PathBuf {
    for f in files {
        if f.ends_with(suffix) {
            let candidate = dir.join(f);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    dir.join(fallback)
}
