"""Frozen combat variants for composition on the confirmed v3 baseline.

Only src/combat.rs is replaced. `shot` is an experimental, bounded relaxation
of the window policy; it is not a scoring recommendation.
"""
from functools import lru_cache
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OPTIONS = ("baseline", "shared", "window", "shot")
SHARED_REVISION = "7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45"
WINDOW_REVISION = "3fbca8bd51f653a6cb4ab77f2e763b585a49954b"


@lru_cache(maxsize=None)
def _combat_at(revision: str) -> str:
    return subprocess.check_output(
        ["git", "show", f"{revision}:src/combat.rs"], cwd=ROOT
    ).decode("utf-8")


def _once(source: str, before: str, after: str) -> str:
    if source.count(before) != 1:
        raise ValueError("Frozen combat anchor must match exactly once")
    return source.replace(before, after)


def _shot(source: str) -> str:
    source = _once(source,
        "/// Borrow gun cooldown only for a mortar shot that fits the current action and\n"
        "/// leaves enough time to return to gun lead before its next ready tick.",
        "/// Experimental handoff: require an immediate aligned mortar shot, but\n"
        "/// permit at most one extra two-tick action of gun delay at a fixed lead.\n"
        "/// No preparation is allowed when this action cannot fire the mortar.")
    source = _once(source,
        "    let return_ticks = ((gun_cooldown - 0.10) / 0.05).ceil().max(1.0);\n"
        "    let return_s = return_ticks * 0.05;",
        "    let ready_ticks = ((gun_cooldown - 0.10) / 0.05).ceil().max(1.0) as u64;\n"
        "    // If the ready tick is the second tick of an action, a rejected\n"
        "    // fire flag is reconsidered on the next action's FIRST tick. A\n"
        "    // four-tick slew budget would incorrectly permit a fifth-tick\n"
        "    // shot, exceeding the intended one-action delay.\n"
        "    let extra_ticks = if ready_ticks % 2 == 0 { 1 } else { 2 };\n"
        "    let return_s = (ready_ticks + extra_ticks) as f64 * 0.05;")
    start = source.index("    #[test]\n    fn cooldown_window_rejects_a_mortar_shot_that_delays_ready_gun()")
    end = source.index("    #[test]\n    fn reloading_gun_still_allows_multi_decision_mortar_preparation()", start)
    source = source[:start] + SHOT_TESTS + source[end:]
    return source


SHOT_TESTS = '''    #[test]
    fn immediate_mortar_can_trade_one_action_for_a_real_shot() {
        for gun_slot in 0..2 {
            let (mut obs, t) = cooldown_window_fixture(160.0, 0.14, 0.08, gun_slot);
            let first = decide(&obs, Some(&t), false);
            let fired = advance_gun_mortar_action(&mut obs, &first);
            assert!(fired[1 - gun_slot], "the trade must buy an actual mortar shot");
            assert!(!fired[gun_slot]);

            let second = decide(&obs, Some(&t), false);
            assert_eq!(advance_gun_mortar_action(&mut obs, &second), [false, false]);
            let third = decide(&obs, Some(&t), false);
            let fired = advance_gun_mortar_action(&mut obs, &third);
            assert!(fired[gun_slot], "gun must return within one extra action");
            assert!(!fired[1 - gun_slot]);
        }
    }

    #[test]
    fn immediate_mortar_rejects_an_excessive_gun_return_slew() {
        let (obs, _) = cooldown_window_fixture(100.0, 0.14, 0.0, 0);
        let mortar = ShotAlignment {
            desired: obs.self_obs.aim, yaw_tolerance: 0.09, pitch_tolerance: 0.07,
        };
        let mut gun = ShotAlignment {
            desired: [1.0, obs.self_obs.aim[1]], yaw_tolerance: 0.02, pitch_tolerance: 0.02,
        };
        assert!(!mortar_fits_gun_cooldown_window(&obs.self_obs, gun, mortar, 0.14, 0.0));
        gun.desired[0] = 0.55;
        assert!(mortar_fits_gun_cooldown_window(&obs.self_obs, gun, mortar, 0.14, 0.0));
        // Ready on the second return tick: only tick three, not four, fits
        // the next actual firing opportunity within a one-action delay.
        gun.desired[0] = 0.75;
        assert!(!mortar_fits_gun_cooldown_window(&obs.self_obs, gun, mortar, 0.19, 0.0));
    }

'''


def apply(sources: dict[str, str], option: str) -> dict[str, str]:
    """Return a new source mapping, preserving every non-combat input verbatim."""
    if option not in OPTIONS:
        raise ValueError(f"Unknown combat option {option!r}; expected {OPTIONS}")
    result = dict(sources)
    if option == "baseline":
        return result
    revision = SHARED_REVISION if option == "shared" else WINDOW_REVISION
    result["src/combat.rs"] = _combat_at(revision)
    if option == "shot":
        result["src/combat.rs"] = _shot(result["src/combat.rs"])
    return result
