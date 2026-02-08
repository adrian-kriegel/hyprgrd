//! Commands and types used throughout hyprgrd.
//!
//! This module defines the vocabulary that all components share:
//! [`Command`] describes every action the switcher can perform,
//! and [`Direction`] / [`MonitorInfo`] / [`WindowInfo`] provide
//! the supporting data types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Cardinal direction for grid navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Direction::Left => write!(f, "left"),
            Direction::Right => write!(f, "right"),
            Direction::Up => write!(f, "up"),
            Direction::Down => write!(f, "down"),
        }
    }
}

/// Every action the grid switcher can perform.
///
/// Commands are produced by [`CommandSource`](crate::traits::CommandSource)
/// implementations and consumed by the [`GridSwitcher`](crate::switcher::GridSwitcher).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Switch all monitors to workspace at absolute grid position `(x, y)`.
    SwitchTo { x: usize, y: usize },

    /// Move one cell in the given direction, creating the column/row if needed.
    Go(Direction),

    /// Move the currently active window one cell in the given direction and
    /// follow it.
    MoveWindowAndGo(Direction),

    /// Move the focused window to the monitor in the given direction
    /// relative to the monitor it is currently on.
    ///
    /// The direction is determined by comparing monitor center positions.
    /// If no monitor exists in that direction, the command is a no-op.
    MoveWindowToMonitor(Direction),

    /// Move the focused window to the monitor at the given index
    /// (0-based, in the order returned by the window manager).
    ///
    /// If the index is out of range, the command returns an error.
    MoveWindowToMonitorIndex(usize),

    /// Gesture-driven partial move.
    ///
    /// `dx` and `dy` are in the range `[-1.0, 1.0]` and represent how far
    /// along each axis the gesture has traveled relative to one grid cell.
    /// The switcher should show a visualization that tracks this offset.
    PrepareMove { dx: f64, dy: f64 },

    /// Cancel an in-progress gesture, snapping the visualization back.
    CancelMove,

    /// Commit a gesture that crossed the threshold — equivalent to
    /// [`Go`](Command::Go) but explicitly marks the end of a gesture
    /// sequence.
    CommitMove(Direction),

    //  Raw touchpad swipe events (forwarded by the Hyprland plugin) 

    /// A multi-finger swipe has started.
    ///
    /// Sent by the Hyprland plugin when it intercepts a
    /// `swipeBegin` hook.  `fingers` is the number of fingers on the
    /// touchpad.
    SwipeBegin { fingers: u32 },

    /// Incremental finger movement during a swipe.
    ///
    /// `dx` / `dy` are raw pixel deltas from the touchpad, **not**
    /// normalised.  The daemon applies sensitivity scaling internally.
    SwipeUpdate { fingers: u32, dx: f64, dy: f64 },

    /// Fingers lifted — end of a swipe gesture.
    SwipeEnd,
}

/// Static information about a monitor known to the window manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    /// Unique name the window manager uses for this monitor (e.g. `"DP-1"`).
    pub name: String,
    /// Horizontal resolution in pixels.
    pub width: u32,
    /// Vertical resolution in pixels.
    pub height: u32,
    /// X position on the virtual desktop (pixels).
    pub x: i32,
    /// Y position on the virtual desktop (pixels).
    pub y: i32,
}

/// Minimal information about the currently focused window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Window manager address / id.
    pub address: String,
    /// Human-readable title.
    pub title: String,
    /// Name of the monitor the window is on (e.g. `"DP-1"`).
    pub monitor: String,
}

/// Find the monitor in the given direction relative to the current monitor.
///
/// Compares monitor center positions and returns the closest monitor
/// whose center is in the requested direction.  Returns `None` if no
/// monitor exists in that direction or if `current_monitor` is not in
/// the list.
pub fn find_monitor_in_direction<'a>(
    monitors: &'a [MonitorInfo],
    current_monitor: &str,
    direction: Direction,
) -> Option<&'a MonitorInfo> {
    let current = monitors.iter().find(|m| m.name == current_monitor)?;
    let cx = current.x as f64 + current.width as f64 / 2.0;
    let cy = current.y as f64 + current.height as f64 / 2.0;

    monitors
        .iter()
        .filter(|m| m.name != current_monitor)
        .filter(|m| {
            let mx = m.x as f64 + m.width as f64 / 2.0;
            let my = m.y as f64 + m.height as f64 / 2.0;
            match direction {
                Direction::Right => mx > cx,
                Direction::Left => mx < cx,
                Direction::Down => my > cy,
                Direction::Up => my < cy,
            }
        })
        .min_by(|a, b| {
            let dist = |m: &MonitorInfo| -> f64 {
                let dx = m.x as f64 + m.width as f64 / 2.0 - cx;
                let dy = m.y as f64 + m.height as f64 / 2.0 - cy;
                dx * dx + dy * dy
            };
            dist(a)
                .partial_cmp(&dist(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_display() {
        assert_eq!(Direction::Left.to_string(), "left");
        assert_eq!(Direction::Right.to_string(), "right");
        assert_eq!(Direction::Up.to_string(), "up");
        assert_eq!(Direction::Down.to_string(), "down");
    }

    #[test]
    fn command_equality() {
        assert_eq!(
            Command::SwitchTo { x: 1, y: 2 },
            Command::SwitchTo { x: 1, y: 2 }
        );
        assert_ne!(Command::Go(Direction::Left), Command::Go(Direction::Right));
        assert_eq!(
            Command::PrepareMove { dx: 0.5, dy: -0.3 },
            Command::PrepareMove { dx: 0.5, dy: -0.3 }
        );
    }

    #[test]
    fn move_window_to_monitor_command_equality() {
        assert_eq!(
            Command::MoveWindowToMonitor(Direction::Right),
            Command::MoveWindowToMonitor(Direction::Right)
        );
        assert_ne!(
            Command::MoveWindowToMonitor(Direction::Left),
            Command::MoveWindowToMonitor(Direction::Right)
        );
    }

    #[test]
    fn move_window_to_monitor_index_command_equality() {
        assert_eq!(
            Command::MoveWindowToMonitorIndex(0),
            Command::MoveWindowToMonitorIndex(0)
        );
        assert_ne!(
            Command::MoveWindowToMonitorIndex(0),
            Command::MoveWindowToMonitorIndex(1)
        );
    }

    #[test]
    fn monitor_info_creation() {
        let m = MonitorInfo {
            name: "DP-1".into(),
            width: 2560,
            height: 1440,
            x: 0,
            y: 0,
        };
        assert_eq!(m.name, "DP-1");
        assert_eq!(m.width, 2560);
    }

    #[test]
    fn window_info_creation() {
        let w = WindowInfo {
            address: "0x1234".into(),
            title: "Terminal".into(),
            monitor: "DP-1".into(),
        };
        assert_eq!(w.address, "0x1234");
        assert_eq!(w.title, "Terminal");
        assert_eq!(w.monitor, "DP-1");
    }

    //  find_monitor_in_direction tests 

    fn two_monitors_horizontal() -> Vec<MonitorInfo> {
        vec![
            MonitorInfo {
                name: "DP-1".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
            },
            MonitorInfo {
                name: "DP-2".into(),
                width: 1920,
                height: 1080,
                x: 1920,
                y: 0,
            },
        ]
    }

    fn three_monitors_l_shape() -> Vec<MonitorInfo> {
        // DP-1 at left, DP-2 at right, DP-3 below DP-1
        vec![
            MonitorInfo {
                name: "DP-1".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
            },
            MonitorInfo {
                name: "DP-2".into(),
                width: 1920,
                height: 1080,
                x: 1920,
                y: 0,
            },
            MonitorInfo {
                name: "DP-3".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 1080,
            },
        ]
    }

    #[test]
    fn find_monitor_right_of_left() {
        let mons = two_monitors_horizontal();
        let target = find_monitor_in_direction(&mons, "DP-1", Direction::Right);
        assert_eq!(target.map(|m| m.name.as_str()), Some("DP-2"));
    }

    #[test]
    fn find_monitor_left_of_right() {
        let mons = two_monitors_horizontal();
        let target = find_monitor_in_direction(&mons, "DP-2", Direction::Left);
        assert_eq!(target.map(|m| m.name.as_str()), Some("DP-1"));
    }

    #[test]
    fn no_monitor_left_of_leftmost() {
        let mons = two_monitors_horizontal();
        let target = find_monitor_in_direction(&mons, "DP-1", Direction::Left);
        assert!(target.is_none());
    }

    #[test]
    fn no_monitor_right_of_rightmost() {
        let mons = two_monitors_horizontal();
        let target = find_monitor_in_direction(&mons, "DP-2", Direction::Right);
        assert!(target.is_none());
    }

    #[test]
    fn find_monitor_below() {
        let mons = three_monitors_l_shape();
        let target = find_monitor_in_direction(&mons, "DP-1", Direction::Down);
        assert_eq!(target.map(|m| m.name.as_str()), Some("DP-3"));
    }

    #[test]
    fn find_monitor_above() {
        let mons = three_monitors_l_shape();
        let target = find_monitor_in_direction(&mons, "DP-3", Direction::Up);
        assert_eq!(target.map(|m| m.name.as_str()), Some("DP-1"));
    }

    #[test]
    fn find_monitor_right_from_bottom() {
        let mons = three_monitors_l_shape();
        // DP-3 is at (0, 1080); DP-2 is at (1920, 0).
        // DP-2's center_x (2880) > DP-3's center_x (960), so DP-2 is to the right.
        let target = find_monitor_in_direction(&mons, "DP-3", Direction::Right);
        assert_eq!(target.map(|m| m.name.as_str()), Some("DP-2"));
    }

    #[test]
    fn unknown_current_monitor_returns_none() {
        let mons = two_monitors_horizontal();
        let target = find_monitor_in_direction(&mons, "NOPE", Direction::Right);
        assert!(target.is_none());
    }

    #[test]
    fn single_monitor_returns_none() {
        let mons = vec![MonitorInfo {
            name: "DP-1".into(),
            width: 1920,
            height: 1080,
            x: 0,
            y: 0,
        }];
        assert!(find_monitor_in_direction(&mons, "DP-1", Direction::Right).is_none());
        assert!(find_monitor_in_direction(&mons, "DP-1", Direction::Left).is_none());
        assert!(find_monitor_in_direction(&mons, "DP-1", Direction::Up).is_none());
        assert!(find_monitor_in_direction(&mons, "DP-1", Direction::Down).is_none());
    }

    #[test]
    fn find_closest_among_multiple_right() {
        // Three monitors in a row; from DP-1, DP-2 (closer) should be picked over DP-3.
        let mons = vec![
            MonitorInfo {
                name: "DP-1".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
            },
            MonitorInfo {
                name: "DP-2".into(),
                width: 1920,
                height: 1080,
                x: 1920,
                y: 0,
            },
            MonitorInfo {
                name: "DP-3".into(),
                width: 1920,
                height: 1080,
                x: 3840,
                y: 0,
            },
        ];
        let target = find_monitor_in_direction(&mons, "DP-1", Direction::Right);
        assert_eq!(target.map(|m| m.name.as_str()), Some("DP-2"));
    }
}

