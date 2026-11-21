"""Pinned navigation/canopy source transformations on the confirmed v3 baseline.

No production files are read or written. Apply this component before the combat
and brain components. Returned strings are normalized to LF for reproducibility.
"""
from functools import lru_cache
from pathlib import Path
import subprocess

V3 = "411af5f530934aa21d8cb5dfff35259219595159"
V4 = "5b783d19f138239033be32b527318f9247a9f9b8"
REPAIRED = "e3569754142f1cd8a9373226dc6ccd0a754d32a8"
OPTIONS = ("baseline", "safe", "canopy", "safe_canopy")
ROOT = Path(__file__).resolve().parents[2]

TERRAIN_TESTS = r'''
#[cfg(test)]
mod experiment_terrain_precision_tests {
    use super::*;

    fn ramp(gradient: f32) -> ArenaMap {
        let mut m = ArenaMap::empty();
        m.loaded = true;
        m.min_x = 0.; m.min_y = 0.; m.max_x = 32.; m.max_y = 32.;
        m.t_min_x = 0.; m.t_min_y = 0.; m.t_step = 1.;
        m.t_nx = 33; m.t_ny = 33;
        m.terrain = (0..1089).map(|i| (i % 33) as f32 * gradient).collect();
        m.los_nx = 16; m.los_ny = 16;
        m.building_top = vec![f32::NEG_INFINITY; 256];
        m.solid_top = vec![0.; 256];
        m
    }

    #[test]
    fn interpolated_collision_accepts_uniform_45_degree_ramp() {
        let m = ramp(1.0);
        assert!((m.terrain_z(9.9, 10.0) - 9.9).abs() < 1e-4);
        assert!(!m.is_blocked_at(9.9, 10.0, 9.9, 2.5));
    }

    #[test]
    fn interpolated_collision_rejects_steeper_uniform_ramp() {
        let m = ramp(1.4);
        assert!(m.is_blocked_at(10.5, 10.0, 14.7, 2.5));
    }
}
'''


@lru_cache(maxsize=None)
def _blob(revision: str, name: str) -> str:
    return subprocess.check_output(
        ["git", "show", f"{revision}:{name}"], cwd=ROOT
    ).decode("utf-8").replace("\r\n", "\n")


def _replace_once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError(f"Expected one source anchor, found {source.count(old)}: {old[:100]!r}")
    return source.replace(old, new, 1)


def _section(source: str, start: str, end: str) -> str:
    if source.count(start) != 1 or source.count(end) != 1:
        raise ValueError("Ambiguous section boundaries in pinned source")
    lo, hi = source.index(start), source.index(end)
    if hi <= lo:
        raise ValueError("Reversed section boundaries")
    return source[lo:hi]


def apply(sources: dict[str, str], option: str) -> dict[str, str]:
    """Return a copy with only the requested navigation/canopy component added."""
    if option not in OPTIONS:
        raise ValueError(f"Unknown navigation option {option!r}; choose from {OPTIONS}")
    result = dict(sources)
    if option == "baseline":
        return result
    if result["src/map.rs"].replace("\r\n", "\n") != _blob(V3, "src/map.rs"):
        raise ValueError("Navigation must start from the exact v3 map source")
    if result["src/nav.rs"].replace("\r\n", "\n") != _blob(V3, "src/nav.rs"):
        raise ValueError("Navigation must start from the exact v3 navigator source")

    canopy = option in ("canopy", "safe_canopy")
    safe = option in ("safe", "safe_canopy")
    map_source = _blob(V4 if canopy else V3, "src/map.rs")
    if canopy:
        # The cache definition remains available, but bot behavior uses the v3
        # interpolation. This includes canopy construction's nonwalkable cells.
        if map_source.count("self.terrain_z_fast(") != 4:
            raise ValueError("Unexpected cached-query count in pinned v4 map")
        map_source = map_source.replace("self.terrain_z_fast(", "self.terrain_z(")

    if safe:
        repaired_map = _blob(REPAIRED, "src/map.rs")
        impl_end = "\n}\n\nfn find_map_file"
        old_methods = _section(map_source, "    pub fn line_walkable(", impl_end)
        new_methods = _section(
            repaired_map,
            "    /// Escape a conservative raster boundary only when the existing physical",
            impl_end,
        )
        map_source = _replace_once(map_source, old_methods, new_methods)
        tests_start = "#[cfg(test)]\nmod navigation_tests"
        tests = repaired_map[repaired_map.index(tests_start):]
        if not canopy:
            # V3 has no unused 2 m terrain cache. These fixture assignments are
            # redundant for the bilinear implementation and are the only test
            # adaptation; terrain and its 1 m samples remain unchanged.
            tests = "\n".join(
                line for line in tests.split("\n") if "map.terrain_grid =" not in line
            )
        map_source += "\n" + tests
        result["src/nav.rs"] = _blob(REPAIRED, "src/nav.rs")

        # Only route selection changes in brain.rs. Drop holding, laser threats,
        # combat priority, micro-steering and all other policy remain intact.
        brain = result["src/brain.rs"].replace("\r\n", "\n")
        v3_brain = _blob(V3, "src/brain.rs")
        repaired_brain = _blob(REPAIRED, "src/brain.rs")
        old_block = _section(v3_brain, "        let waypoint = if on_roof", "\n\n        // 8. 16-Direction Micro-Steering")
        new_block = _section(repaired_brain, "        let target_is_higher =", "\n\n        // 8. 16-Direction Micro-Steering")
        old_declaration = (
            "        // 9. Jump & Boost / Jetpack decision\n"
            "        let target_is_higher =\n"
            '            goal_pos3[2] - pos[2] > 1.3 && dist_to_goal < 14.0 && state_label != "ZONE";\n'
        )
        brain = _replace_once(brain, old_declaration, "        // 9. Jump & Boost / Jetpack decision\n")
        result["src/brain.rs"] = _replace_once(brain, old_block, new_block)
    result["src/map.rs"] = map_source + "\n" + TERRAIN_TESTS
    return result
