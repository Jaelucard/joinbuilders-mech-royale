
use mechbot::combat::{aim_angles,evaluate_combat,CombatTarget};
use mechbot::map::ArenaMap;
use mechbot::protocol::{Config,Observation,RawObservation};
use mechbot::world::WorldState;
use serde_json::json;
fn main(){
 let cfg:Config=serde_json::from_str(&std::fs::read_to_string("/input/hello_config.json").unwrap()).unwrap();
 for distance in [100.0,130.0,160.0] {
 let initial=aim_angles([0.0,0.0,7.0],[distance,0.0,4.5]);
 let raw:RawObservation=serde_json::from_value(json!({"tick":100,"t":5,"alive":16,
 "self":{"id":0,"pos":[0,0,0],"vel":[0,0,0],"aim":initial,"hp":1400,"max_hp":1400,"energy":100,"max_energy":100,"grounded":true,"cubes":0,"in_zone":true,"stats":cfg.mech,
 "weapons":[{"kind":"gun","cooldown":0.14,"ammo":12,"reload":0,"stats":cfg.weapons.gun},{"kind":"mortar","cooldown":0,"stats":cfg.weapons.mortar}]},
 "zone":{"center":[0,0],"radius":500,"next_center":[0,0],"next_radius":380,"shrink_start":200,"shrink_end":240,"dps":20}})).unwrap();
 let mut obs:Observation=raw.into();
 let target=CombatTarget{is_mech:true,is_crate:false,pos:[distance,0.0,4.5],vel:[0.0,0.0,0.0],hp:1400.0,radius:2.5,half_height:4.5,has_mortar:false,has_laser:false,has_lightning:false,score:1.0};
 let world=WorldState::new(16);
 for step in 0..3 {
  let d=evaluate_combat(&obs,&cfg,&ArenaMap::empty(),&world,Some(&target),false,false,false,false);
  println!("d={distance} decision={step} current_pitch={:.6} gun_cd={:.3} command_pitch={:.6} fire={:?}",obs.self_obs.aim[1],obs.self_obs.weapons[0].as_ref().unwrap().cooldown,d.aim.unwrap()[1],d.fire);
  let command=d.aim.unwrap();
  let mut fired=[false,false];
  for _ in 0..2 {
   for axis in 0..2 {let rate=if axis==0{4.0}else{3.0};obs.self_obs.aim[axis]+=(command[axis]-obs.self_obs.aim[axis]).clamp(-rate*0.05,rate*0.05);}
   for slot in 0..2 {
    let w=obs.self_obs.weapons[slot].as_mut().unwrap();w.cooldown=(w.cooldown-0.05).max(0.0);
    if d.fire[slot]&&w.cooldown==0.0 {fired[slot]=true;w.cooldown=if slot==0{0.24}else{2.6};}
   }
  }
  println!("  two_tick_fires={:?} end_pitch={:.6}",fired,obs.self_obs.aim[1]);
 }
 }
}
