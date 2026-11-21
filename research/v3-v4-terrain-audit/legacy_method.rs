impl ArenaMap {
    fn audit_v3_is_blocked_at(&self, x: f64, y: f64, feet_z: f64, radius: f64) -> bool {
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
        let tz_center = self.terrain_z(xf, yf);
        let max_slope_dz = rf.max(1.5) * 1.11 + 0.35; // ~48 deg max walkable slope
        let offsets = [[0.0, 0.0], [rf, 0.0], [-rf, 0.0], [0.0, rf], [0.0, -rf]];
        for [ox, oy] in offsets {
            let sx = xf + ox;
            let sy = yf + oy;
            let bt = self.building_top_at(sx, sy);
            if bt.is_finite() && bt > zf + MECH_STEP_UP {
                return true;
            }
            let tz = self.terrain_z(sx, sy);
            if tz > zf + MECH_STEP_UP && (tz - tz_center) > max_slope_dz {
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

}
