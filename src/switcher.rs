//! The main orchestrator that ties the grid, window manager, and command
//! sources together.
//!
//! [`GridSwitcher`] owns the [`Grid`] and reacts to [`Command`]s by updating
//! the grid state and issuing calls to the [`WindowManager`] trait.

use crate::command::{find_monitor_in_direction, Command};
use crate::grid::Grid;
use crate::hyprland::gestures::{clamp_unit, dominant_direction, GestureConfig};
use crate::traits::{VisualizerEvent, VisualizerState, WindowManager};
use log::{debug, info, warn};
use std::sync::mpsc;

/// Possible errors from the switcher.
#[derive(Debug, thiserror::Error)]
pub enum SwitcherError {
    /// The window manager returned an error.
    #[error("window manager error: {0}")]
    WindowManager(String),
}

/// State of an in-flight touchpad swipe gesture.
#[derive(Debug, Default)]
struct ActiveSwipe {
    fingers: u32,
    dx: f64,
    dy: f64,
}

/// Orchestrates grid navigation and window-manager calls.
///
/// The switcher is generic over any [`WindowManager`] implementation, making
/// it completely independent of Hyprland or any other concrete backend.
///
/// # Typical usage
///
/// ```ignore
/// let wm = HyprlandWm::new()?;
/// let mut switcher = GridSwitcher::new(wm, vec!["DP-1".into()]);
/// switcher.handle(Command::Go(Direction::Right))?;
/// ```
pub struct GridSwitcher<W: WindowManager> {
    wm: W,
    grid: Grid,
    vis_tx: Option<mpsc::Sender<VisualizerEvent>>,
    gesture_config: GestureConfig,
    active_swipe: Option<ActiveSwipe>,
}

impl<W: WindowManager> GridSwitcher<W> {
    /// Create a new switcher.
    ///
    /// `monitors` lists the monitor names that should switch in unison.
    /// The grid is initialized as 1×1 at position `(0, 0)`.
    pub fn new(wm: W, monitors: Vec<String>) -> Self {
        Self {
            wm,
            grid: Grid::new(monitors),
            vis_tx: None,
            gesture_config: GestureConfig::default(),
            active_swipe: None,
        }
    }

    /// Set the gesture configuration (sensitivity, thresholds, finger
    /// counts).  Only affects the processing of raw
    /// [`SwipeBegin`](Command::SwipeBegin) /
    /// [`SwipeUpdate`](Command::SwipeUpdate) /
    /// [`SwipeEnd`](Command::SwipeEnd) commands.
    pub fn set_gesture_config(&mut self, config: GestureConfig) {
        self.gesture_config = config;
    }

    /// Attach a visualizer event channel.
    ///
    /// The switcher will send [`VisualizerEvent::Show`] during gesture
    /// commands (`PrepareMove`) and [`VisualizerEvent::Hide`] when the
    /// gesture ends (`CommitMove` / `CancelMove`).
    ///
    /// The receiver end can be owned by any independent listener — the
    /// GTK overlay, a debug logger, etc.
    pub fn set_visualizer(&mut self, tx: mpsc::Sender<VisualizerEvent>) {
        self.vis_tx = Some(tx);
    }

    /// Return a shared reference to the underlying grid (useful for
    /// inspecting state in tests or for rendering the visualization).
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Process a single [`Command`].
    ///
    /// Returns `Ok(())` on success.  If the window manager fails, the grid
    /// state has **already** been updated (the switcher is optimistic); a
    /// retry or recovery strategy is left to the caller.
    pub fn handle(&mut self, cmd: Command) -> Result<(), SwitcherError> {
        match cmd {
            Command::SwitchTo { x, y } => {
                info!("switch to ({}, {})", x, y);
                self.grid.switch_to(x, y);
                self.apply_current_workspace()?;
                // Flash the visualizer so the user sees the new position.
                self.show_visualizer(0.0, 0.0);
                self.hide_visualizer();
            }

            Command::Go(dir) => {
                info!("go {}", dir);
                self.grid.go(dir);
                self.apply_current_workspace()?;
                // Flash the visualizer so the user sees the new position.
                self.show_visualizer(0.0, 0.0);
                self.hide_visualizer();
            }

            Command::MoveWindowAndGo(dir) => {
                info!("move window and go {}", dir);
                // First, determine the target mapping *before* switching
                let mut preview = self.grid.clone();
                preview.go(dir);
                if let Some(mapping) = preview.current_mapping() {
                    // Move the window to the first monitor's target workspace
                    // (we pick the first; multi-monitor move is a future
                    // enhancement).
                    if let Some((_, ws_id)) = mapping.iter().next() {
                        self.wm
                            .move_window_to_workspace(ws_id)
                            .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
                    }
                }
                self.grid.go(dir);
                self.apply_current_workspace()?;
                // Flash the visualizer so the user sees the new position.
                self.show_visualizer(0.0, 0.0);
                self.hide_visualizer();
            }

            Command::MoveWindowToMonitor(dir) => {
                info!("move window to monitor {}", dir);
                let monitors = self
                    .wm
                    .monitors()
                    .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
                let active = self
                    .wm
                    .active_window()
                    .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
                if let Some(win) = active {
                    if let Some(target) =
                        find_monitor_in_direction(&monitors, &win.monitor, dir)
                    {
                        info!("  → moving to monitor {}", target.name);
                        self.wm
                            .move_window_to_monitor(&target.name)
                            .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
                    } else {
                        warn!("no monitor {} of {}", dir, win.monitor);
                    }
                } else {
                    debug!("no active window, nothing to move");
                }
            }

            Command::MoveWindowToMonitorIndex(idx) => {
                info!("move window to monitor index {}", idx);
                let monitors = self
                    .wm
                    .monitors()
                    .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
                if idx >= monitors.len() {
                    return Err(SwitcherError::WindowManager(format!(
                        "monitor index {} out of range (have {})",
                        idx,
                        monitors.len()
                    )));
                }
                self.wm
                    .move_window_to_monitor(&monitors[idx].name)
                    .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
            }

            Command::PrepareMove { dx, dy } => {
                debug!("prepare move dx={:.2} dy={:.2}", dx, dy);
                self.show_visualizer(dx, dy);
            }

            Command::CancelMove => {
                debug!("cancel move");
                self.hide_visualizer();
            }

            Command::CommitMove(dir) => {
                info!("commit move {}", dir);
                self.grid.go(dir);
                self.apply_current_workspace()?;
                // Show the final position, then schedule hide.
                self.show_visualizer(0.0, 0.0);
                self.hide_visualizer();
            }

            //  Raw swipe events (from the Hyprland plugin) 

            Command::SwipeBegin { fingers } => {
                let cfg = &self.gesture_config;
                if fingers == cfg.switch_fingers || fingers == cfg.move_fingers {
                    debug!("swipe begin: {} fingers", fingers);
                    self.active_swipe = Some(ActiveSwipe {
                        fingers,
                        dx: 0.0,
                        dy: 0.0,
                    });
                }
            }

            Command::SwipeUpdate { fingers: _, dx, dy } => {
                if let Some(ref mut swipe) = self.active_swipe {
                    swipe.dx += dx;
                    swipe.dy += dy;
                    let cfg = &self.gesture_config;
                    let norm_dx = clamp_unit(swipe.dx / cfg.sensitivity);
                    let norm_dy = clamp_unit(swipe.dy / cfg.sensitivity);
                    debug!("swipe update: dx={:.2} dy={:.2}", norm_dx, norm_dy);
                    self.show_visualizer(norm_dx, norm_dy);
                }
            }

            Command::SwipeEnd => {
                if let Some(swipe) = self.active_swipe.take() {
                    let cfg = &self.gesture_config;
                    let norm_dx = clamp_unit(swipe.dx / cfg.sensitivity);
                    let norm_dy = clamp_unit(swipe.dy / cfg.sensitivity);

                    match dominant_direction(norm_dx, norm_dy, cfg.commit_threshold) {
                        Some(dir) if swipe.fingers == cfg.move_fingers => {
                            info!("swipe commit: move window and go {}", dir);
                            // Same logic as MoveWindowAndGo:
                            let mut preview = self.grid.clone();
                            preview.go(dir);
                            if let Some(mapping) = preview.current_mapping() {
                                if let Some((_, ws_id)) = mapping.iter().next() {
                                    self.wm
                                        .move_window_to_workspace(ws_id)
                                        .map_err(|e| {
                                            SwitcherError::WindowManager(e.to_string())
                                        })?;
                                }
                            }
                            self.grid.go(dir);
                            self.apply_current_workspace()?;
                            self.show_visualizer(0.0, 0.0);
                            self.hide_visualizer();
                        }
                        Some(dir) => {
                            info!("swipe commit: go {}", dir);
                            self.grid.go(dir);
                            self.apply_current_workspace()?;
                            self.show_visualizer(0.0, 0.0);
                            self.hide_visualizer();
                        }
                        None => {
                            debug!("swipe cancel (below threshold)");
                            self.hide_visualizer();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    //  Visualizer helpers 

    /// Send the current grid state (plus gesture offsets) to the
    /// visualizer channel, if one is attached.
    fn show_visualizer(&mut self, offset_x: f64, offset_y: f64) {
        if let Some(tx) = &self.vis_tx {
            let state = VisualizerState::from_grid(&self.grid, offset_x, offset_y);
            let _ = tx.send(VisualizerEvent::Show(state));
        }
    }

    /// Send a hide event to the visualizer channel, if one is attached.
    fn hide_visualizer(&mut self) {
        if let Some(tx) = &self.vis_tx {
            let _ = tx.send(VisualizerEvent::Hide);
        }
    }

    /// Tell the window manager to switch every monitor to the workspace ids
    /// stored in the current grid cell.
    fn apply_current_workspace(&self) -> Result<(), SwitcherError> {
        let mapping = self
            .grid
            .current_mapping()
            .expect("current cell must always have a mapping");

        for (monitor, ws_id) in mapping.iter() {
            debug!("  {} -> workspace {}", monitor, ws_id);
            self.wm
                .switch_workspace(monitor, ws_id)
                .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
        }
        Ok(())
    }
}

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Direction, MonitorInfo, WindowInfo};
    use std::cell::RefCell;

    /// Record-keeping mock window manager.
    #[derive(Debug, Default)]
    struct RecorderWm {
        switches: RefCell<Vec<(String, i32)>>,
        moves: RefCell<Vec<i32>>,
        monitor_moves: RefCell<Vec<String>>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("recorder error")]
    struct RecorderErr;

    impl WindowManager for RecorderWm {
        type Error = RecorderErr;

        fn monitors(&self) -> Result<Vec<MonitorInfo>, RecorderErr> {
            Ok(vec![
                MonitorInfo {
                    name: "DP-1".into(),
                    width: 2560,
                    height: 1440,
                    x: 0,
                    y: 0,
                },
                MonitorInfo {
                    name: "HDMI-A-1".into(),
                    width: 1920,
                    height: 1080,
                    x: 2560,
                    y: 0,
                },
            ])
        }

        fn switch_workspace(&self, mon: &str, ws: i32) -> Result<(), RecorderErr> {
            self.switches.borrow_mut().push((mon.into(), ws));
            Ok(())
        }

        fn move_window_to_workspace(&self, ws: i32) -> Result<(), RecorderErr> {
            self.moves.borrow_mut().push(ws);
            Ok(())
        }

        fn move_window_to_monitor(&self, monitor: &str) -> Result<(), RecorderErr> {
            self.monitor_moves.borrow_mut().push(monitor.into());
            Ok(())
        }

        fn active_window(&self) -> Result<Option<WindowInfo>, RecorderErr> {
            Ok(Some(WindowInfo {
                address: "0xbeef".into(),
                title: "test".into(),
                monitor: "DP-1".into(),
            }))
        }
    }

    fn make_switcher() -> GridSwitcher<RecorderWm> {
        GridSwitcher::new(RecorderWm::default(), vec!["DP-1".into(), "HDMI-A-1".into()])
    }

    #[test]
    fn go_right_switches_both_monitors() {
        let mut s = make_switcher();
        s.handle(Command::Go(Direction::Right)).unwrap();
        let switches = s.wm.switches.borrow();
        // Two monitors should each have received a switch call
        assert_eq!(switches.len(), 2);
        // They should reference the same pair of workspace ids (from cell (1,0))
        let ids: Vec<i32> = switches.iter().map(|(_, id)| *id).collect();
        assert_eq!(ids.len(), 2);
        // Ids are different (one per monitor)
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn switch_to_absolute() {
        let mut s = make_switcher();
        s.handle(Command::SwitchTo { x: 2, y: 1 }).unwrap();
        assert_eq!(s.grid().position(), (2, 1));
        assert_eq!(s.grid().dimensions(), (3, 2));
    }

    #[test]
    fn move_window_and_go_records_move() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowAndGo(Direction::Right)).unwrap();
        let moves = s.wm.moves.borrow();
        assert_eq!(moves.len(), 1, "should have moved one window");
        assert_eq!(s.grid().position(), (1, 0));
    }

    #[test]
    fn prepare_move_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::PrepareMove { dx: 0.5, dy: 0.0 })
            .unwrap();
        assert_eq!(s.grid().position(), (0, 0), "grid should not move");
    }

    #[test]
    fn cancel_move_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::CancelMove).unwrap();
        assert_eq!(s.grid().position(), (0, 0));
    }

    #[test]
    fn commit_move_advances_grid() {
        let mut s = make_switcher();
        s.handle(Command::CommitMove(Direction::Down)).unwrap();
        assert_eq!(s.grid().position(), (0, 1));
    }

    #[test]
    fn full_command_sequence() {
        let mut s = make_switcher();
        s.handle(Command::Go(Direction::Right)).unwrap();
        s.handle(Command::Go(Direction::Down)).unwrap();
        s.handle(Command::Go(Direction::Left)).unwrap();
        s.handle(Command::Go(Direction::Up)).unwrap();
        assert_eq!(s.grid().position(), (0, 0));
        // Grid should have grown to 2x2
        assert_eq!(s.grid().dimensions(), (2, 2));
    }

    //  MoveWindowToMonitor tests 

    #[test]
    fn move_window_to_monitor_right() {
        let mut s = make_switcher();
        // RecorderWm: DP-1 at (0,0) 2560×1440, HDMI-A-1 at (2560,0) 1920×1080
        // active_window() returns monitor "DP-1"
        s.handle(Command::MoveWindowToMonitor(Direction::Right))
            .unwrap();
        let monitor_moves = s.wm.monitor_moves.borrow();
        assert_eq!(monitor_moves.len(), 1);
        assert_eq!(monitor_moves[0], "HDMI-A-1");
    }

    #[test]
    fn move_window_to_monitor_no_target_is_noop() {
        let mut s = make_switcher();
        // active_window() returns monitor "DP-1"; no monitor to the left of DP-1
        s.handle(Command::MoveWindowToMonitor(Direction::Left))
            .unwrap();
        let monitor_moves = s.wm.monitor_moves.borrow();
        assert_eq!(monitor_moves.len(), 0, "should be no-op");
    }

    #[test]
    fn move_window_to_monitor_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitor(Direction::Right))
            .unwrap();
        assert_eq!(s.grid().position(), (0, 0), "grid should not move");
    }

    //  MoveWindowToMonitorIndex tests 

    #[test]
    fn move_window_to_monitor_index_valid() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitorIndex(1)).unwrap();
        let monitor_moves = s.wm.monitor_moves.borrow();
        assert_eq!(monitor_moves.len(), 1);
        assert_eq!(monitor_moves[0], "HDMI-A-1");
    }

    #[test]
    fn move_window_to_monitor_index_zero() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitorIndex(0)).unwrap();
        let monitor_moves = s.wm.monitor_moves.borrow();
        assert_eq!(monitor_moves.len(), 1);
        assert_eq!(monitor_moves[0], "DP-1");
    }

    #[test]
    fn move_window_to_monitor_index_out_of_range() {
        let mut s = make_switcher();
        let result = s.handle(Command::MoveWindowToMonitorIndex(5));
        assert!(result.is_err());
    }

    #[test]
    fn move_window_to_monitor_index_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitorIndex(1)).unwrap();
        assert_eq!(s.grid().position(), (0, 0), "grid should not move");
    }
}

