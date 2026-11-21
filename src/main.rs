mod brain;
mod combat;
mod map;
mod nav;
mod protocol;
mod world;

use brain::BotBrain;
use protocol::{parse_server_message, Ready, ServerMessage};
use std::io::{self, BufRead, BufReader, Write};
use std::sync::mpsc;
use std::thread;

const BOT_NAME: &str = "vanguard-v1";

fn main() {
    let (tx, rx) = mpsc::channel::<Option<Vec<String>>>();

    // Background stdin reader: drains all already-buffered complete lines in one batch
    // so queued observations (e.g. bursts) never race across threads.
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = BufReader::with_capacity(65536, stdin.lock());
        loop {
            let mut batch = Vec::new();
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(None);
                    break;
                }
                Ok(_) => {
                    if !line.trim().is_empty() {
                        batch.push(line);
                    }
                    // Immediately drain any additional complete lines already sitting in BufReader's buffer
                    while reader.buffer().contains(&b'\n') {
                        let mut next_line = String::new();
                        match reader.read_line(&mut next_line) {
                            Ok(0) => break,
                            Ok(_) => {
                                if !next_line.trim().is_empty() {
                                    batch.push(next_line);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    if !batch.is_empty() && tx.send(Some(batch)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("stdin read error: {}", e);
                    let _ = tx.send(None);
                    break;
                }
            }
        }
    });

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut brain: Option<BotBrain> = None;

    while let Ok(first) = rx.recv() {
        let mut lines = match first {
            Some(b) => b,
            None => break,
        };
        // Also drain any additional batches queued while we were computing
        while let Ok(more) = rx.try_recv() {
            match more {
                Some(mut b) => lines.append(&mut b),
                None => {
                    // Process remaining lines up to EOF, then exit
                    process_batch(&lines, &mut brain, &mut out);
                    return;
                }
            }
        }
        if process_batch(&lines, &mut brain, &mut out) {
            break;
        }
    }
}

/// Process a batch of JSON lines; returns `true` when `end` is received.
fn process_batch<W: Write>(lines: &[String], brain: &mut Option<BotBrain>, out: &mut W) -> bool {
    let mut latest_obs = None;

    for raw in lines {
        match parse_server_message(raw) {
            Ok(ServerMessage::Hello(hello)) => {
                *brain = Some(BotBrain::new(*hello));
                let ready = Ready::new(BOT_NAME);
                send_json(out, &ready);
            }
            Ok(ServerMessage::Obs(obs)) => {
                latest_obs = Some(obs);
            }
            Ok(ServerMessage::End(end)) => {
                eprintln!(
                    "{} finished: place={} kills={}",
                    BOT_NAME, end.place, end.kills
                );
                return true;
            }
            Ok(ServerMessage::Unknown) => {}
            Err(e) => {
                eprintln!("parse error: {} (line len={})", e, raw.len());
            }
        }
    }

    if let Some(obs) = latest_obs {
        let action = if let Some(b) = brain.as_mut() {
            b.act(&obs)
        } else {
            protocol::Action {
                tick: obs.tick,
                ..Default::default()
            }
        };
        send_json(out, &action);
    }

    false
}

fn send_json<W: Write, T: serde::Serialize>(out: &mut W, msg: &T) {
    if let Ok(line) = serde_json::to_string(msg) {
        let _ = out.write_all(line.as_bytes());
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use crate::combat::{solve_linear_intercept, solve_mortar_pitches};
    use crate::map::ArenaMap;
    use crate::nav::Navigator;
    use crate::protocol::HelloMap;

    #[test]
    fn test_linear_intercept() {
        let origin = [0.0, 0.0, 7.0];
        let target = [100.0, 0.0, 4.5];
        let vel = [0.0, 15.0, 0.0];
        let (pred, t) = solve_linear_intercept(origin, 252.0, target, vel).unwrap();
        assert!(t > 0.35 && t < 0.45);
        assert!((pred[1] - 15.0 * t).abs() < 1e-4);
    }

    #[test]
    fn test_mortar_solver() {
        let (low, high) = solve_mortar_pitches(120.0, 0.0, 72.0, 22.0).unwrap();
        assert!(low > 0.0 && low < high && high < std::f64::consts::FRAC_PI_2);
        // Verify range formula: v^2 * sin(2*theta) / g == 120
        let r_low = 72.0_f64.powi(2) * (2.0 * low).sin() / 22.0;
        let r_high = 72.0_f64.powi(2) * (2.0 * high).sin() / 22.0;
        assert!((r_low - 120.0).abs() < 1e-4);
        assert!((r_high - 120.0).abs() < 1e-4);
    }

    #[test]
    fn test_empty_map_and_nav_fallback() {
        let map = ArenaMap::load(&HelloMap {
            dir: "/nonexistent".into(),
            files: vec![],
        });
        assert!(!map.loaded);
        let mut nav = Navigator::new(&map);
        let wp = nav.next_waypoint(&map, 1, [0.0, 0.0, 0.0], [50.0, 50.0]);
        assert_eq!(wp, [50.0, 50.0]);
    }

    #[test]
    fn test_real_arena_nav_and_los() {
        if !std::path::Path::new("kit/arena/v2/world.json").exists() {
            return;
        }
        let map = ArenaMap::load(&HelloMap {
            dir: "kit/arena".into(),
            files: vec!["v2/world.json".into(), "v2/terrain.i16".into()],
        });
        assert!(map.loaded);
        assert!(map.prisms.len() > 200);
        // High-altitude open sky LoS must be clear
        assert!(map.has_los([0.0, 0.0, 120.0], [100.0, 100.0, 120.0]));
        // Pathfinding across campus should return a valid waypoint
        let mut nav = Navigator::new(&map);
        let wp = nav.next_waypoint(&map, 1, [0.0, 0.0, 0.0], [200.0, -100.0]);
        assert!(wp[0].is_finite() && wp[1].is_finite());
    }
}
