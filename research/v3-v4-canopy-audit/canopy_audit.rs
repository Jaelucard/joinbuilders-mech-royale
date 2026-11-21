use mechbot::map::ArenaMap;
use mechbot::protocol::{dist2d, HelloMap, Vec3};
use serde_json::json;

fn details(map: &ArenaMap, p: Vec3, from: Vec3, threat: Vec3) -> serde_json::Value {
    let c = map.world_to_nav(p[0], p[1]);
    json!({"pos":p,"canopy":map.nav_canopy[c.1*map.nav_nx+c.0],
        "travel_m":dist2d([from[0],from[1]],[p[0],p[1]]),
        "threat_distance_m":dist2d([threat[0],threat[1]],[p[0],p[1]]),
        "hard_los_exposed":map.has_los([p[0],p[1],p[2]+7.5],threat)})
}

fn main() {
    let mut map = ArenaMap::load(&HelloMap{dir:"/arena".into(),files:vec![]});
    assert!(map.loaded);
    let canopy = map.nav_canopy.clone();
    let mut histogram=[0usize;11];
    let mut eligible=0;
    for (i,&v) in canopy.iter().enumerate() {
        if map.walkable[i] && map.wall_dist[i]>0 && map.wall_dist[i]<=3 {
            histogram[v as usize]+=1; eligible+=1;
        }
    }
    println!("CANOPY_SUMMARY {}",json!({"eligible_ground_cells":eligible,"canopy_histogram":histogram}));
    let zone=[0.0,0.0];
    let mut checked=0;
    let mut changed=0;
    'scan: for gy in (2..map.nav_ny-2).step_by(2) {
        for gx in (2..map.nav_nx-2).step_by(2) {
            let idx=gy*map.nav_nx+gx;
            if !map.walkable[idx] || map.wall_dist[idx]==0 || map.wall_dist[idx]>3 {continue;}
            let p=map.nav_to_world(gx,gy);
            let from=[p[0],p[1],map.terrain_z(p[0] as f32,p[1] as f32) as f64];
            if dist2d(p,zone)>460.0 {continue;}
            if map.is_on_roof(from) || map.is_blocked_at(from[0],from[1],from[2],2.5) {continue;}
            if !(gy-2..=gy+2).any(|y| (gx-2..=gx+2).any(|x| canopy[y*map.nav_nx+x]>0)) {continue;}
            for range in [55.0,80.0,120.0] {
                for direction in 0..12 {
                    let a=direction as f64*std::f64::consts::TAU/12.0;
                    let tx=from[0]+range*a.cos();let ty=from[1]+range*a.sin();
                    if tx<map.min_x as f64 || tx>map.max_x as f64 || ty<map.min_y as f64 || ty>map.max_y as f64 {continue;}
                    let tz=map.terrain_z(tx as f32,ty as f32) as f64;
                    let tc=map.world_to_nav(tx,ty);
                    if !map.is_walkable_cell(tc.0,tc.1) || map.is_blocked_at(tx,ty,tz,2.5) {continue;}
                    let threat=[tx,ty,tz+7.5];
                    for prefer_far in [false,true] {
                        let radius=if prefer_far {44.0} else {19.0};
                        let enabled=map.find_cover_pos(from,&[threat],zone,480.0,radius,prefer_far);
                        map.nav_canopy.fill(0);
                        let disabled=map.find_cover_pos(from,&[threat],zone,480.0,radius,prefer_far);
                        map.nav_canopy.copy_from_slice(&canopy);
                        checked+=1;
                        if enabled!=disabled {
                            changed+=1;
                            println!("CANOPY_WITNESS {}",json!({"from":from,"threat":threat,"prefer_far":prefer_far,
                                "search_radius":radius,"zone_center":zone,"zone_radius":480,
                                "enabled":enabled.map(|p|details(&map,p,from,threat)),
                                "disabled":disabled.map(|p|details(&map,p,from,threat))}));
                        }
                        if changed>=8 || checked>=12000 {break 'scan;}
                    }
                }
            }
        }
    }
    println!("CANOPY_SCAN {}",json!({"checked":checked,"changed":changed,"stop_after_changes":8,"maximum_checks":12000,"sampling_note":"bounded synthetic threat/origin scan on actual arena, not a gameplay frequency estimate"}));
}
