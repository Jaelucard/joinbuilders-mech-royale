"""Composable v3 brain experiments: drop waiting and credible laser threats.

Only exact, asserted source regions are changed. Navigation edits already in the
input dictionary are retained. These are experimental policies, not score claims.
"""
from functools import lru_cache
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
V3 = "411af5f530934aa21d8cb5dfff35259219595159"
DROP_REPAIR = "7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45"
OPTIONS = ("baseline", "drop", "threat", "drop_threat")


@lru_cache(maxsize=None)
def _brain(revision: str) -> str:
    return subprocess.check_output(
        ["git", "show", f"{revision}:src/brain.rs"], cwd=ROOT
    ).decode("utf-8")


def _replace_once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError(f"Expected one brain anchor, found {source.count(old)}: {old[:80]!r}")
    return source.replace(old, new, 1)


def _between(source: str, start: str, end: str) -> str:
    begin = source.index(start)
    return source[begin:source.index(end, begin)]


def _drop(source: str) -> str:
    old = _brain(V3)
    repaired = _brain(DROP_REPAIR)
    hold = _between(repaired, "struct DropHold {", "pub struct BotBrain")
    source = _replace_once(source, "pub struct BotBrain", hold + "pub struct BotBrain")
    field = "    goal_track: Option<(&'static str, Vec2, u64, f64)>,\n"
    source = _replace_once(source, field, field + "    drop_hold: Option<DropHold>,\n")
    source = _replace_once(source, "            goal_track: None,\n", "            goal_track: None,\n            drop_hold: None,\n")
    old_track = _between(old, "    fn track_goal_progress(", "    pub fn act(")
    new_track = _between(repaired, "    // Returns true while intentionally holding inside a drop ring.", "    pub fn act(")
    source = _replace_once(source, old_track, new_track)
    source = _replace_once(source,
        "        self.track_goal_progress(state_label, goal2, pos2, obs.tick, obs.t);",
        "        let holding_drop = self.track_goal_progress(state_label, goal2, obs);")
    source = _replace_once(source,
        "            self.prev_wanted_move,\n            is_travel_state,",
        "            self.prev_wanted_move && !holding_drop,\n            is_travel_state && !holding_drop,")
    source = _replace_once(source,
        "        self.prev_wanted_move = dist_to_goal > 4.0 && !holding_cover;",
        "        self.prev_wanted_move = dist_to_goal > 4.0 && !holding_cover && !holding_drop;")
    # Port only the nine supply-drop regressions, not candidate navigation or
    # combat policy. They are cfg(test), so release behavior is solely above.
    tests = _between(repaired, "#[cfg(test)]\nmod tests {", "    fn corner_map()")
    tests = tests.replace("mod tests {", "mod experiment_drop_tests {", 1)
    return source + "\n" + tests + "}\n"


def _threat(source: str, has_drop: bool) -> str:
    old = _brain(V3)
    false_finish = _between(old,
        "        // Only duck to cover reachable within ~0.75s",
        "        let laser_duck_cover =")
    source = _replace_once(source, false_finish,
        "        // Own charge and an unrelated low-HP enemy do not prove a kill.\n"
        "        // Keep defensive fire; select emergency cover only for a credible charger.\n")
    source = _replace_once(source,
        "if !urgent_zone && !charging_los_snipers.is_empty() && !my_laser_about_to_kill {",
        "if !urgent_zone && !charging_los_snipers.is_empty() {")
    source = _replace_once(source,
        "            if e.charging && has_los_to_e && ed < 275.0 {",
        "            // Reuse the existing threat evidence: an aim cone toward us,\n"
        "            // or recent damage by this actor. Charge alone can be aimed elsewhere.\n"
        "            if e.charging && has_los_to_e && ed < 275.0\n"
        "                && (e.aiming_at_me || obs.t - e.last_damaged_me_t < 3.0)\n"
        "            {")
    repaired = _brain(DROP_REPAIR)
    fixture = _between(repaired, "#[cfg(test)]\nmod tests {", "    #[test]\n    fn early_drop_arrival")
    fixture = fixture.replace("mod tests {", "mod experiment_threat_tests {", 1)
    if not has_drop:
        fixture = fixture.replace("goal_track:None,drop_hold:None,", "goal_track:None,")
    corner = _between(repaired, "    fn corner_map()", "    #[test]\n    fn nonlethal_own_laser")
    tests = (Path(__file__).with_name("brain") / "threat_tests.rs").read_text(encoding="utf-8")
    return source + "\n" + fixture + corner + tests + "}\n"


def apply(sources: dict[str, str], option: str) -> dict[str, str]:
    if option not in OPTIONS:
        raise ValueError(f"Unknown brain option: {option}")
    result = dict(sources)
    if option == "baseline":
        return result
    brain = result["src/brain.rs"]
    has_drop = option in ("drop", "drop_threat")
    if has_drop:
        brain = _drop(brain)
    if option in ("threat", "drop_threat"):
        brain = _threat(brain, has_drop)
    result["src/brain.rs"] = brain
    return result
