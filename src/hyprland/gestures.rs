//! Translates Hyprland touchpad swipe events into hyprgrd [`Command`]s.
//!
//! # How three-finger gestures become `PrepareMove` / `CommitMove`
//!
//! Hyprland emits three event types for touchpad swipes over its IPC event
//! socket (`socket2`):
//!
//! | Event           | Payload               | Meaning                               |
//! |-----------------|-----------------------|---------------------------------------|
//! | `swipebegin`    | `<fingers>`           | A multi-finger swipe has started      |
//! | `swipeupdate`   | `<fingers>,<dx>,<dy>` | Incremental finger movement (pixels)  |
//! | `swipeend`      | `<fingers>`           | Fingers lifted                        |
//!
//! These events are sent in the `EVENT>>DATA\n` format on socket2 at
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`.
//!
//! [`HyprlandGestureSource`] connects to this socket directly (bypassing
//! the `hyprland` crate's event listener, which does not expose swipe
//! events) and:
//!
//! 1. **`swipebegin` with 3 fingers** → starts accumulating `(dx, dy)`.
//! 2. **Each `swipeupdate`** → adds the delta to the accumulator, normalises
//!    to `[-1.0, 1.0]` using [`GestureConfig::sensitivity`], and emits
//!    [`Command::PrepareMove`].
//! 3. **`swipeend`** →
//!    - If the dominant axis exceeds [`GestureConfig::commit_threshold`],
//!      emits [`Command::CommitMove`] in the corresponding direction.
//!    - Otherwise emits [`Command::CancelMove`] (snap back).
//!
//! Four-finger swipes follow the same flow but emit
//! [`Command::MoveWindowAndGo`] instead of `CommitMove`, carrying the
//! active window along.

use crate::command::{Command, Direction};
use crate::traits::CommandSource;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;

/// Tuning knobs for gesture recognition.
///
/// `sensitivity` controls how many pixels of finger travel correspond to
/// one full grid cell (`1.0` in normalised space).  A higher value makes
/// the gesture feel "heavier".
///
/// `commit_threshold` (in `[0.0, 1.0]`) sets the normalised distance the
/// gesture must exceed before the workspace actually switches on finger
/// lift.
///
/// `commit_while_dragging_threshold`: when set (e.g. `0.8`), the workspace
/// switches as soon as the gesture reaches that fraction of the way toward
/// the next cell, without waiting for release. `None` = only commit on
/// release (default).
///
/// `natural_swiping` inverts the gesture direction (swipe right → grid
/// moves left, like "natural" scroll).  Default: `true`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GestureConfig {
    /// Pixels of travel per normalised unit.  Default: `200.0`.
    pub sensitivity: f64,
    /// Normalised threshold to commit a switch on finger lift.  Default: `0.3`.
    pub commit_threshold: f64,
    /// If set (0.0–1.0), commit as soon as the gesture reaches this fraction
    /// toward the next cell, without waiting for release.  Default: `None`.
    pub commit_while_dragging_threshold: Option<f64>,
    /// Number of fingers for a plain workspace switch.  Default: `3`.
    pub switch_fingers: u32,
    /// Number of fingers for move-window-and-switch.  Default: `4`.
    pub move_fingers: u32,
    /// Invert gesture direction (natural swiping).  Default: `true`.
    pub natural_swiping: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            sensitivity: 200.0,
            commit_threshold: 0.3,
            commit_while_dragging_threshold: None,
            switch_fingers: 3,
            move_fingers: 4,
            natural_swiping: true,
        }
    }
}

/// Accumulator for an in-flight swipe gesture.
#[derive(Debug, Default)]
struct SwipeState {
    active: bool,
    fingers: u32,
    dx: f64,
    dy: f64,
}

/// A [`CommandSource`] that listens to Hyprland swipe events via the raw
/// IPC event socket and emits grid commands.
///
/// This struct connects directly to Hyprland's event socket (`socket2`)
/// and translates touchpad swipe gestures into the `PrepareMove` /
/// `CommitMove` / `CancelMove` command vocabulary that the
/// [`GridSwitcher`](crate::switcher::GridSwitcher) understands.
pub struct HyprlandGestureSource {
    config: GestureConfig,
}

impl HyprlandGestureSource {
    /// Create a new gesture source with the given configuration.
    pub fn new(config: GestureConfig) -> Self {
        Self { config }
    }

    /// Create a gesture source with default settings.
    pub fn with_defaults() -> Self {
        Self::new(GestureConfig::default())
    }
}

/// Determine the dominant direction from accumulated deltas.
///
/// When both axes exceed `threshold`, returns a diagonal direction (45°).
/// Otherwise returns the cardinal direction of the stronger axis, or `None`
/// if neither exceeds threshold.
pub(crate) fn dominant_direction(norm_dx: f64, norm_dy: f64, threshold: f64) -> Option<Direction> {
    let abs_x = norm_dx.abs();
    let abs_y = norm_dy.abs();

    let over_x = abs_x >= threshold;
    let over_y = abs_y >= threshold;

    if over_x && over_y {
        Some(match (norm_dx > 0.0, norm_dy > 0.0) {
            (true, true) => Direction::DownRight,
            (true, false) => Direction::UpRight,
            (false, true) => Direction::DownLeft,
            (false, false) => Direction::UpLeft,
        })
    } else if abs_x >= abs_y && over_x {
        Some(if norm_dx > 0.0 {
            Direction::Right
        } else {
            Direction::Left
        })
    } else if abs_y > abs_x && over_y {
        Some(if norm_dy > 0.0 {
            Direction::Down
        } else {
            Direction::Up
        })
    } else {
        None
    }
}

/// Clamp `value` to `[-1.0, 1.0]`.
pub(crate) fn clamp_unit(value: f64) -> f64 {
    value.clamp(-1.0, 1.0)
}

/// Normalised swipe offset: clamp to [-1, 1] per axis, optionally invert (natural swiping).
pub(crate) fn normalised_swipe_offset(
    dx: f64,
    dy: f64,
    sensitivity: f64,
    natural_swiping: bool,
) -> (f64, f64) {
    let norm_dx = clamp_unit(dx / sensitivity);
    let norm_dy = clamp_unit(dy / sensitivity);
    if natural_swiping {
        (-norm_dx, -norm_dy)
    } else {
        (norm_dx, norm_dy)
    }
}

/// Resolve the Hyprland event socket path.
///
/// Hyprland stores its sockets at
/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`.
fn socket2_path() -> Result<PathBuf, HyprlandGestureError> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map_err(|_| HyprlandGestureError("XDG_RUNTIME_DIR not set".into()))?;
    let his = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| HyprlandGestureError("HYPRLAND_INSTANCE_SIGNATURE not set".into()))?;
    Ok(PathBuf::from(format!(
        "{}/hypr/{}/.socket2.sock",
        runtime_dir, his
    )))
}

/// Parse a single event line from socket2.
///
/// Lines have the form `EVENT>>DATA\n`.
fn parse_event_line(line: &str) -> Option<(&str, &str)> {
    let sep = line.find(">>")?;
    Some((&line[..sep], &line[sep + 2..]))
}

/// Process a single event and potentially emit commands.
fn handle_event(
    event: &str,
    data: &str,
    state: &mut SwipeState,
    config: &GestureConfig,
    sink: &mpsc::Sender<Command>,
) {
    match event {
        "swipebegin" => {
            if let Ok(fingers) = data.trim().parse::<u32>() {
                if fingers == config.switch_fingers || fingers == config.move_fingers {
                    debug!("swipe begin: {} fingers", fingers);
                    state.active = true;
                    state.fingers = fingers;
                    state.dx = 0.0;
                    state.dy = 0.0;
                }
            }
        }
        "swipeupdate" => {
            if !state.active {
                return;
            }
            // Format: "<fingers>,<dx>,<dy>"
            let parts: Vec<&str> = data.trim().split(',').collect();
            if parts.len() == 3 {
                if let (Ok(dx), Ok(dy)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) {
                    state.dx += dx;
                    state.dy += dy;
                    let norm_dx = clamp_unit(state.dx / config.sensitivity);
                    let norm_dy = clamp_unit(state.dy / config.sensitivity);
                    debug!("swipe update: dx={:.2} dy={:.2}", norm_dx, norm_dy);
                    let _ = sink.send(Command::PrepareMove {
                        dx: norm_dx,
                        dy: norm_dy,
                    });
                }
            }
        }
        "swipeend" => {
            if !state.active {
                return;
            }
            let norm_dx = clamp_unit(state.dx / config.sensitivity);
            let norm_dy = clamp_unit(state.dy / config.sensitivity);

            let cmd = match dominant_direction(norm_dx, norm_dy, config.commit_threshold) {
                Some(dir) if state.fingers == config.move_fingers => {
                    Command::MoveWindowAndGo(dir)
                }
                Some(dir) => Command::CommitMove(dir),
                None => Command::CancelMove,
            };
            debug!("swipe end: {:?}", cmd);
            let _ = sink.send(cmd);

            state.active = false;
            state.dx = 0.0;
            state.dy = 0.0;
        }
        _ => {
            // Ignore events we don't care about.
        }
    }
}

impl CommandSource for HyprlandGestureSource {
    type Error = HyprlandGestureError;

    /// Connect to Hyprland's event socket and start listening for swipe
    /// events.
    ///
    /// This method **blocks** forever (until the socket is closed or an
    /// error occurs).  Run it on a dedicated thread.
    fn run(&mut self, sink: mpsc::Sender<Command>) -> Result<(), Self::Error> {
        let path = socket2_path()?;
        info!("connecting to socket2: {}", path.display());
        let stream = UnixStream::connect(&path)
            .map_err(|e| HyprlandGestureError(format!("connect to {}: {}", path.display(), e)))?;
        info!("gesture source connected to {}", path.display());
        let reader = BufReader::new(stream);
        let mut state = SwipeState::default();
        let mut first_swipe_logged = false;

        for line in reader.lines() {
            match line {
                Ok(line) if line.is_empty() => continue,
                Ok(line) => {
                    if let Some((raw_event, data)) = parse_event_line(&line) {
                        // Strip any namespace prefix (e.g. "touchpad:swipebegin" → "swipebegin")
                        let event = raw_event
                            .rsplit_once(':')
                            .map(|(_, name)| name)
                            .unwrap_or(raw_event);

                        if !first_swipe_logged && event.starts_with("swipe") {
                            info!(
                                "first swipe event received: raw={:?} parsed={:?} data={:?}",
                                raw_event, event, data
                            );
                            first_swipe_logged = true;
                        }

                        handle_event(event, data, &mut state, &self.config, &sink);
                    }
                }
                Err(e) => {
                    error!("socket2 read error: {}", e);
                    return Err(HyprlandGestureError(format!("read error: {}", e)));
                }
            }
        }

        warn!("socket2 stream ended");
        Ok(())
    }
}

/// Error from the Hyprland gesture source.
#[derive(Debug, thiserror::Error)]
#[error("hyprland gesture error: {0}")]
pub struct HyprlandGestureError(String);

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_unit_works() {
        assert_eq!(clamp_unit(0.5), 0.5);
        assert_eq!(clamp_unit(1.5), 1.0);
        assert_eq!(clamp_unit(-2.0), -1.0);
        assert_eq!(clamp_unit(0.0), 0.0);
    }

    #[test]
    fn dominant_direction_right() {
        assert_eq!(
            dominant_direction(0.5, 0.1, 0.3),
            Some(Direction::Right)
        );
    }

    #[test]
    fn dominant_direction_left() {
        assert_eq!(
            dominant_direction(-0.6, 0.2, 0.3),
            Some(Direction::Left)
        );
    }

    #[test]
    fn dominant_direction_down() {
        assert_eq!(
            dominant_direction(0.1, 0.8, 0.3),
            Some(Direction::Down)
        );
    }

    #[test]
    fn dominant_direction_up() {
        assert_eq!(
            dominant_direction(-0.1, -0.5, 0.3),
            Some(Direction::Up)
        );
    }

    #[test]
    fn dominant_direction_below_threshold_is_none() {
        assert_eq!(dominant_direction(0.1, 0.1, 0.3), None);
    }

    #[test]
    fn dominant_direction_exactly_at_threshold() {
        assert_eq!(
            dominant_direction(0.3, 0.0, 0.3),
            Some(Direction::Right)
        );
    }

    #[test]
    fn dominant_direction_diagonals() {
        let t = 0.3;
        assert_eq!(dominant_direction(0.5, 0.5, t), Some(Direction::DownRight));
        assert_eq!(dominant_direction(0.5, -0.5, t), Some(Direction::UpRight));
        assert_eq!(dominant_direction(-0.5, 0.5, t), Some(Direction::DownLeft));
        assert_eq!(dominant_direction(-0.5, -0.5, t), Some(Direction::UpLeft));
        assert_eq!(dominant_direction(0.2, 0.2, t), None);
    }

    #[test]
    fn default_config_values() {
        let cfg = GestureConfig::default();
        assert_eq!(cfg.sensitivity, 200.0);
        assert_eq!(cfg.commit_threshold, 0.3);
        assert_eq!(cfg.commit_while_dragging_threshold, None);
        assert_eq!(cfg.switch_fingers, 3);
        assert_eq!(cfg.move_fingers, 4);
        assert!(cfg.natural_swiping);
    }

    #[test]
    fn parse_event_line_valid() {
        assert_eq!(
            parse_event_line("swipebegin>>3"),
            Some(("swipebegin", "3"))
        );
        assert_eq!(
            parse_event_line("swipeupdate>>3,10.5,-2.3"),
            Some(("swipeupdate", "3,10.5,-2.3"))
        );
    }

    #[test]
    fn parse_event_line_no_separator() {
        assert_eq!(parse_event_line("garbage"), None);
    }

    #[test]
    fn handle_event_swipe_begin_activates_state() {
        let cfg = GestureConfig::default();
        let (tx, _rx) = mpsc::channel();
        let mut state = SwipeState::default();

        handle_event("swipebegin", "3", &mut state, &cfg, &tx);
        assert!(state.active);
        assert_eq!(state.fingers, 3);
    }

    #[test]
    fn handle_event_swipe_begin_ignores_wrong_finger_count() {
        let cfg = GestureConfig::default();
        let (tx, _rx) = mpsc::channel();
        let mut state = SwipeState::default();

        handle_event("swipebegin", "2", &mut state, &cfg, &tx);
        assert!(!state.active);
    }

    #[test]
    fn handle_event_swipe_update_accumulates_and_emits() {
        let cfg = GestureConfig::default();
        let (tx, rx) = mpsc::channel();
        let mut state = SwipeState {
            active: true,
            fingers: 3,
            dx: 0.0,
            dy: 0.0,
        };

        handle_event("swipeupdate", "3,50.0,10.0", &mut state, &cfg, &tx);
        assert_eq!(state.dx, 50.0);
        assert_eq!(state.dy, 10.0);

        let cmd = rx.try_recv().unwrap();
        assert_eq!(
            cmd,
            Command::PrepareMove {
                dx: 50.0 / cfg.sensitivity,
                dy: 10.0 / cfg.sensitivity,
            }
        );
    }

    #[test]
    fn handle_event_swipe_end_commits_right() {
        let cfg = GestureConfig::default();
        let (tx, rx) = mpsc::channel();
        let mut state = SwipeState {
            active: true,
            fingers: 3,
            dx: 250.0,
            dy: 10.0,
        };

        handle_event("swipeend", "3", &mut state, &cfg, &tx);
        let cmd = rx.try_recv().unwrap();
        assert_eq!(cmd, Command::CommitMove(Direction::Right));
        assert!(!state.active);
    }

    #[test]
    fn handle_event_swipe_end_cancels_small_gesture() {
        let cfg = GestureConfig::default();
        let (tx, rx) = mpsc::channel();
        let mut state = SwipeState {
            active: true,
            fingers: 3,
            dx: 10.0,
            dy: 5.0,
        };

        handle_event("swipeend", "3", &mut state, &cfg, &tx);
        let cmd = rx.try_recv().unwrap();
        assert_eq!(cmd, Command::CancelMove);
    }

    #[test]
    fn handle_event_four_finger_swipe_moves_window() {
        let cfg = GestureConfig::default();
        let (tx, rx) = mpsc::channel();
        let mut state = SwipeState {
            active: true,
            fingers: 4,
            dx: -300.0,
            dy: 10.0,
        };

        handle_event("swipeend", "4", &mut state, &cfg, &tx);
        let cmd = rx.try_recv().unwrap();
        assert_eq!(cmd, Command::MoveWindowAndGo(Direction::Left));
    }

    /// End-to-end simulation: begin → multiple updates → end.
    #[test]
    fn full_gesture_simulation() {
        let cfg = GestureConfig::default();
        let (tx, rx) = mpsc::channel();
        let mut state = SwipeState::default();

        // Begin
        handle_event("swipebegin", "3", &mut state, &cfg, &tx);
        assert!(state.active);

        // Simulate 10 incremental rightward updates (25px each)
        for _ in 0..10 {
            handle_event("swipeupdate", "3,25.0,2.0", &mut state, &cfg, &tx);
        }

        // End
        handle_event("swipeend", "3", &mut state, &cfg, &tx);

        let cmds: Vec<Command> = rx.try_iter().collect();
        // 10 PrepareMove + 1 CommitMove
        assert_eq!(cmds.len(), 11);

        // All intermediate commands are PrepareMove
        for cmd in &cmds[..10] {
            assert!(matches!(cmd, Command::PrepareMove { .. }));
        }

        // Final command commits rightward
        assert_eq!(cmds[10], Command::CommitMove(Direction::Right));
    }

    /// Unknown events are silently ignored.
    #[test]
    fn unknown_events_ignored() {
        let cfg = GestureConfig::default();
        let (tx, rx) = mpsc::channel();
        let mut state = SwipeState::default();

        handle_event("workspace", "2", &mut state, &cfg, &tx);
        handle_event("activewindow", "kitty,~", &mut state, &cfg, &tx);
        assert!(rx.try_recv().is_err());
    }
}
