// Standalone baseline/candidate diagnostic; pass --baseline for commit 5b783d1.
use mechbot::brain::BotBrain;
use mechbot::combat::{aim_angles, evaluate_combat, solve_linear_intercept, CombatTarget};
use mechbot::map::ArenaMap;
use mechbot::protocol::{Config, Hello, HelloMap, Observation, RawObservation};
use mechbot::world::WorldState;
use serde_json::json;

fn main() {
    let baseline = std::env::args().any(|arg| arg == "--baseline");
    let cfg: Config = serde_json::from_str(
        &std::fs::read_to_string("/input/hello_config.json").unwrap(),
    ).unwrap();
    let raw: RawObservation = serde_json::from_value(json!({
        "tick":1740,"t":87.0,"alive":16,
        "self":{"id":0,"pos":[0,0,0],"vel":[0,0,0],
            "aim":[0.009749691,-0.0249936],"hp":1400,"max_hp":1400,
            "energy":100,"max_energy":100,"grounded":true,"cubes":0,
            "weapons":[
                {"kind":"gun","cooldown":0,"ammo":12,"reload":0,"stats":cfg.weapons.gun},
                {"kind":"laser","cooldown":0,"charge":0.3,"stats":cfg.weapons.laser}],
            "in_zone":true,"stats":cfg.mech},
        "zone":{"center":[0,0],"radius":500,"next_center":[0,0],
            "next_radius":380,"shrink_start":200,"shrink_end":240,"dps":20}
    })).unwrap();
    let mut obs: Observation = raw.into();
    let target = CombatTarget {
        is_mech:true,is_crate:false,pos:[100.0,0.0,4.5],vel:[0.0,19.5,0.0],
        hp:1400.0,radius:2.5,half_height:4.5,has_mortar:false,
        has_laser:false,has_lightning:false,score:1.0,
    };
    let world = WorldState::new(16);
    let d = evaluate_combat(&obs,&cfg,&ArenaMap::empty(),&world,
        Some(&target),false,false,false,false);
    let (p,_) = solve_linear_intercept([0.0,0.0,7.0],252.0,target.pos,target.vel).unwrap();
    let gun_aim = aim_angles([0.0,0.0,7.0],p);
    println!("shared aim: fire={:?}, chosen={:?}, gun required={:?}, lateral separation={:.3}m",
        d.fire,d.aim,gun_aim,p[1]-100.0*d.aim.unwrap()[0].tan());
    let mismatch = (gun_aim[0]-d.aim.unwrap()[0]).abs();
    if baseline {
        assert!(d.fire[0] && mismatch>0.05);
    } else {
        assert!(d.fire[0] && mismatch<1e-6, "keep useful gun fire with its actual lead");
    }

    obs.self_obs.weapons[1] = None;
    obs.self_obs.aim = [0.0,0.0];
    obs.drops = serde_json::from_value(json!([
        {"id":9000,"pos":[0,0,0],"radius":8,"lands_in":8,
            "capture_s":4,"progress":0,"holder":null}
    ])).unwrap();
    let mut brain = BotBrain::new(Hello {
        protocol:"mechfight/1".into(),slot:0,seed:1,
        map:HelloMap{dir:"/missing-probe-map".into(),files:vec![]},config:cfg,
    });
    let mut previous = String::new();
    let mut abandoned = false;
    for n in 0..120 {
        obs.tick = 1740+n*2;
        obs.t = 87.0+n as f64*0.1;
        obs.drops[0].lands_in = (95.0-obs.t).max(0.0);
        if obs.t>=95.0 {
            obs.drops[0].holder=Some(0);
            obs.drops[0].progress=((obs.t-95.0)/4.0).min(1.0);
        }
        let a = brain.act(&obs);
        let label = a.debug.unwrap().label.unwrap_or_default();
        if label != previous {
            println!("drop: t={:.1}, lands_in={:.1}, {}",
                obs.t,obs.drops[0].lands_in,label);
            previous = label.clone();
        }
        if n == 0 { assert!(label.contains("DROP")); }
        if n > 0 && !label.contains("DROP") { abandoned=true; break; }
    }
    assert_eq!(abandoned,baseline,"baseline abandons; candidate waits through landing and capture");
    println!("Combat/drop expectations passed (baseline={}).",baseline);

    if std::path::Path::new("/input/arena/v2/world.json").exists() {
        let map = ArenaMap::load(&HelloMap {
            dir:"/input/arena".into(),files:vec![],
        });
        let a = [-242.38,48.91];
        let b = [-238.38,48.91];
        let (ax,ay) = map.world_to_nav(a[0],a[1]);
        let (bx,by) = map.world_to_nav(b[0],b[1]);
        let rise = map.nav_z[by*map.nav_nx+bx]-map.nav_z[ay*map.nav_nx+ax];
        let clear = map.line_walkable(a,b);
        let mut nav = mechbot::nav::Navigator::new(&map);
        let waypoint = nav.next_waypoint(&map,1,[-246.38,48.91,-6.7],b);
        println!("navigation: rise={:.6}m, cells_walkable={}/{}, line_walkable={}, waypoint={:?}",
            rise,map.walkable[ay*map.nav_nx+ax],map.walkable[by*map.nav_nx+bx],clear,waypoint);
        assert!(rise>4.2);
        if baseline {
            assert!(clear && waypoint==b);
        } else {
            assert!(!clear && waypoint!=b);
            assert!(map.line_walkable([-246.38,48.91],waypoint));
        }
        println!("Navigation expectations passed (baseline={}).",baseline);
    }
}
