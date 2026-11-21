    fn charger_fixture(gun_only: bool) -> (BotBrain, Observation) {
        let (mut brain, mut obs) = fixture();
        brain.map = corner_map();
        brain.nav = Navigator::new(&brain.map);
        obs.drops.clear();
        obs.self_obs.pos = [0.0, 12.0, 0.0];
        obs.self_obs.aim = [0.0, -0.05];
        if !gun_only {
            obs.self_obs.weapons[1] = Some(serde_json::from_value(json!({
                "kind":"laser","cooldown":0,"charge":0.5,
                "stats":{"damage":315,"charge":0.75,"min_frac":0.35}
            })).unwrap());
        }
        obs.mechs = serde_json::from_value(json!([{
            "id":1,"kind":"mech","pos":[50,12,0],"vel":[0,0,0],
            "aim":[std::f64::consts::PI,0],"hp":350,"max_hp":1400,
            "cubes":0,"weapons":["laser"],"charging":true
        }])).unwrap();
        (brain, obs)
    }

    #[test]
    fn aimed_charger_is_not_ignored_for_an_unproven_own_kill() {
        let (mut brain, obs) = charger_fixture(false);
        // At zero cubes the own 0.5 s charge cannot kill this 350 HP target.
        let action = brain.act(&obs);
        assert!(label(&action).starts_with("LASER_DUCK"), "{}", label(&action));
        assert_eq!(action.fire.unwrap()[0], true, "retain useful return fire");
    }

    #[test]
    fn charger_facing_elsewhere_does_not_abort_our_charge() {
        let (mut brain, mut obs) = charger_fixture(false);
        obs.mechs[0].aim = [0.0, 0.0];
        // Stay below the old 0.5 s exemption, so the legacy broad threat
        // trigger really would abort this charge merely on seeing a charger.
        obs.self_obs.weapons[1].as_mut().unwrap().charge = Some(0.3);
        let action = brain.act(&obs);
        assert!(!label(&action).starts_with("LASER_DUCK"), "{}", label(&action));
        assert_eq!(action.fire, Some([true, true]), "preserve gun fire and laser charge");
    }

    #[test]
    fn recently_damaging_charger_remains_a_threat_while_turning() {
        let (mut brain, mut obs) = charger_fixture(false);
        obs.mechs[0].aim = [0.0, 0.0];
        brain.act(&obs); // Establish the tracked actor before the damage event.
        obs.t += 0.1;
        obs.tick += 2;
        obs.events = vec![json!({"type":"damaged","by":1,"weapon":"laser","amount":1})];
        let action = brain.act(&obs);
        assert!(label(&action).starts_with("LASER_DUCK"), "{}", label(&action));
        assert!(action.fire.unwrap()[0]);
    }

    #[test]
    fn gun_only_keeps_defensive_fire_while_ducking_a_charger() {
        let (mut brain, obs) = charger_fixture(true);
        let action = brain.act(&obs);
        assert!(label(&action).starts_with("LASER_DUCK"), "{}", label(&action));
        assert_eq!(action.fire, Some([true, false]));
    }

    #[test]
    fn exposed_regen_does_not_become_a_blanket_firing_ban() {
        for charge in [0.0, 0.3] {
            let (mut brain, mut obs) = fixture();
            obs.drops.clear();
            obs.self_obs.hp = 600.0;
            obs.self_obs.aim = [0.0, -0.05];
            obs.self_obs.weapons[1] = Some(serde_json::from_value(json!({
                "kind":"laser","cooldown":0,"charge":charge
            })).unwrap());
            obs.mechs = serde_json::from_value(json!([{
                "id":1,"kind":"mech","pos":[50,0,0],"vel":[0,0,0],
                "aim":[std::f64::consts::PI,0],"hp":1400,"max_hp":1400,
                "cubes":0,"weapons":["gun"],"charging":false
            }])).unwrap();
            let action = brain.act(&obs);
            assert!(label(&action).starts_with("REGEN"), "{}", label(&action));
            assert_eq!(action.fire, Some([true, true]));
        }
    }
