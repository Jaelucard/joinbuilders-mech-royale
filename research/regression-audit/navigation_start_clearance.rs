// Append to src/map.rs only inside a disposable preserved builder container.
// AUDIT_VARIANT=baseline or candidate; this asserts the observed unfixed behavior.
#[cfg(test)]
mod audit_startclearance {
    use super::*;

    #[test]
    fn flat_wall_valid_body_recovery() {
        let mut m = ArenaMap::empty();
        m.loaded = true;
        m.min_x = 0.;
        m.max_x = 32.;
        m.min_y = 0.;
        m.max_y = 32.;
        m.los_nx = 16;
        m.los_ny = 16;
        m.terrain_grid = vec![0.; 256];
        m.solid_top = vec![0.; 256];
        m.building_top = (0..256)
            .map(|i| if i % 16 >= 8 { 10. } else { f32::NEG_INFINITY })
            .collect();
        m.nav_nx = 8;
        m.nav_ny = 8;
        m.nav_z = vec![0.; 64];
        m.roof_top = vec![f32::NAN; 64];
        m.walkable = (0..64).map(|i| i % 8 < 3).collect();
        m.wall_dist = (0..64).map(|i| if i % 8 < 3 { 1 } else { 0 }).collect();

        let p = [13.45, 10., 0.];
        let goal = [10., 10.];
        assert!(!m.is_blocked_at(p[0], p[1], p[2], 2.5));
        assert!(m.is_blocked_at(p[0], p[1], p[2], 2.65));
        for i in 0..=100 {
            let x = p[0] + (goal[0] - p[0]) * i as f64 / 100.;
            assert!(!m.is_blocked_at(x, 10., 0., 2.5));
        }
        let cell = m.world_to_nav(p[0], p[1]);
        assert!(!m.is_walkable_cell(cell.0, cell.1));

        let mut nav = crate::nav::Navigator::new(&m);
        let wp = nav.next_waypoint(&m, 0, p, goal);
        let variant = std::env::var("AUDIT_VARIANT").unwrap();
        println!(
            "AUDIT variant={} start={:?} wall_x=16 goal={:?} \
             body_2_5_clear=true radius_2_65_clear=false \
             raster_start_walkable=false path_body_clear=true waypoint={:?}",
            variant, p, goal, wp
        );
        if variant == "baseline" {
            assert_eq!(wp, goal);
        } else {
            assert_eq!(wp, [p[0], p[1]]);
        }
    }
}
