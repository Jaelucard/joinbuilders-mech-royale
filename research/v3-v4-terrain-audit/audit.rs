#[cfg(test)]
mod terrain_resolution_audit {
    use super::*;
    use serde_json::json;

    fn ramp(gradient: f32) -> ArenaMap {
        let mut m=ArenaMap::empty();
        m.loaded=true;
        m.min_x=0.;m.min_y=0.;m.max_x=32.;m.max_y=32.;
        m.t_min_x=0.;m.t_min_y=0.;m.t_step=1.;m.t_nx=33;m.t_ny=33;
        m.terrain=(0..1089).map(|i|(i%33) as f32*gradient).collect();
        m.los_nx=16;m.los_ny=16;
        m.building_top=vec![f32::NEG_INFINITY;256];
        m.terrain_grid=(0..256).map(|i|((i%16) as f32*2.+1.)*gradient).collect();
        m
    }

    #[test]
    fn cached_height_can_block_a_uniform_45_degree_ramp() {
        let m=ramp(1.0);
        let p=[9.9,10.0,9.9];
        assert!(!m.audit_v3_is_blocked_at(p[0],p[1],p[2],2.5));
        assert!(m.is_blocked_at(p[0],p[1],p[2],2.5));
        println!("SYNTHETIC_JSON {}",detail(&m,p[0],p[1],p[2],2.5));
    }

    #[test]
    fn cached_height_can_clear_a_uniform_steep_ramp() {
        let m=ramp(1.4);
        let p=[10.5,10.0,14.7];
        assert!(m.audit_v3_is_blocked_at(p[0],p[1],p[2],2.5));
        assert!(!m.is_blocked_at(p[0],p[1],p[2],2.5));
        println!("SYNTHETIC_JSON {}",detail(&m,p[0],p[1],p[2],2.5));
    }

    fn detail(m: &ArenaMap, x: f64, y: f64, feet: f64, radius: f64) -> serde_json::Value {
        let mut samples = Vec::new();
        for [dx,dy] in [[0.,0.],[radius,0.],[-radius,0.],[0.,radius],[0.,-radius]] {
            samples.push(json!({"xy":[x+dx,y+dy],
                "exact":m.terrain_z((x+dx) as f32,(y+dy) as f32),
                "fast":m.terrain_z_fast((x+dx) as f32,(y+dy) as f32)}));
        }
        json!({"position":[x,y,feet],"radius":radius,
            "v3_blocked":m.audit_v3_is_blocked_at(x,y,feet,radius),
            "v4_blocked":m.is_blocked_at(x,y,feet,radius),"samples":samples})
    }

    #[test]
    fn actual_map_resolution_effects() {
        let m = ArenaMap::load(&HelloMap{dir:"/input/arena".into(),files:vec![]});
        assert!(m.loaded);
        let mut count=0usize;
        let mut error_sum=0.;
        let mut errors=Vec::new();
        let mut max_error=(0f32,0f64,0f64,0f32,0f32);
        let mut gate_skips=0usize;
        let mut gate_nav_blocked=0usize;
        let mut gate_skips_v3_clear=0usize;
        let mut gate_witness=None;
        let mut diffs=[[0usize;2];2];
        let mut witnesses:Vec<Vec<Option<serde_json::Value>>>=vec![vec![None,None],vec![None,None]];
        let mut y=m.min_y as f64+8.271;
        while y<m.max_y as f64-8. {
            let mut x=m.min_x as f64+8.137;
            while x<m.max_x as f64-8. {
                let exact=m.terrain_z(x as f32,y as f32);
                let fast=m.terrain_z_fast(x as f32,y as f32);
                let error=fast-exact;
                count+=1; error_sum+=(error as f64).abs(); errors.push(error.abs());
                if error.abs()>max_error.0.abs() {max_error=(error,x,y,exact,fast);}
                if exact>fast+1.5 {
                    gate_skips+=1;
                    let cell=m.world_to_nav(x,y);
                    if m.wall_dist_cell(cell.0,cell.1)==0 {
                        gate_nav_blocked+=1;
                        if !m.audit_v3_is_blocked_at(x,y,exact as f64,2.5) {
                            gate_skips_v3_clear+=1;
                            if gate_witness.is_none() {
                                gate_witness=Some(json!({"position":[x,y,exact as f64],"exact":exact,"fast":fast,"wall_dist":0,
                                    "v3_blocked_radius_2_5":false,"v4_blocked_radius_2_5":m.is_blocked_at(x,y,exact as f64,2.5)}));
                            }
                        }
                    }
                }
                for (ri,radius) in [1.8,2.5].iter().enumerate() {
                    let old=m.audit_v3_is_blocked_at(x,y,exact as f64,*radius);
                    let new=m.is_blocked_at(x,y,exact as f64,*radius);
                    if old!=new {
                        let direction=if new {0}else{1};
                        diffs[ri][direction]+=1;
                        if witnesses[ri][direction].is_none() {
                            witnesses[ri][direction]=Some(detail(&m,x,y,exact as f64,*radius));
                        }
                    }
                }
                x+=1.;
            }
            y+=1.;
        }
        errors.sort_by(|a,b|a.total_cmp(b));
        println!("AUDIT_JSON {}",json!({
            "kind":"deterministic real-map sample audit, not game outcomes",
            "sample_step_m":1.0,"samples":count,"bounds":[m.min_x,m.min_y,m.max_x,m.max_y],
            "mean_absolute_height_error":error_sum/count as f64,
            "p95_absolute_height_error":errors[(count as f64*0.95) as usize],
            "p99_absolute_height_error":errors[(count as f64*0.99) as usize],
            "max_height_error":{"signed_fast_minus_exact":max_error.0,"xy":[max_error.1,max_error.2],"exact":max_error.3,"fast":max_error.4},
            "grounded_gate_skipped":gate_skips,"grounded_gate_skipped_in_zero_clearance_nav_cell":gate_nav_blocked,"grounded_gate_witness":gate_witness,
            "grounded_gate_skipped_zero_clearance_and_v3_body_clear":gate_skips_v3_clear,
            "collision_difference_columns":["v3_clear_v4_blocked","v3_blocked_v4_clear"],
            "radii":[1.8,2.5],"collision_differences":diffs,"witnesses":witnesses
        }));
        assert!(diffs.iter().flatten().any(|n|*n>0));
    }
}
