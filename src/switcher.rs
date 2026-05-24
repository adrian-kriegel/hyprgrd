//! The main orchestrator that ties the grid, window manager, and command
//! sources together.
//!
//! [`GridSwitcher`] owns the [`Grid`] and reacts to [`Command`]s by updating
//! the grid state and issuing calls to the [`WindowManager`] trait.

use crate::command::{
    find_monitor_in_direction, Command, Direction, MonitorIndex, SwitchTo,
};
use crate::common::GridPosition;
use crate::event::Event;
use crate::grid::Grid;
use crate::hyprland::gestures::{dominant_direction, normalised_swipe_offset, GestureConfig};
use crate::traits::{VisualizerEvent, VisualizerShowPayload, VisualizerState, WindowManager};
use log::{debug, info, warn};
use std::collections::HashMap;
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

/// Per-monitor position in the shared grid.
#[derive(Debug, Clone)]
pub struct MonitorGridPosition {
    pub name: String,
    pub grid_position: GridPosition,
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
    monitor_positions: Vec<MonitorGridPosition>,
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
        let monitor_positions = monitors
            .into_iter()
            .map(|name| MonitorGridPosition {
                name,
                grid_position: GridPosition::ORIGIN,
            })
            .collect();

        Self {
            wm,
            grid: Grid::new(),
            monitor_positions,
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
    /// The switcher will send:
    ///
    /// - [`VisualizerEvent::ShowAuto`] during navigation / gesture commands
    ///   (`Go`, `SwitchTo`, `PrepareMove`, swipe updates, etc.)
    /// - [`VisualizerEvent::ToggleManual`] when handling
    ///   [`Command::ToggleVisualizer`]
    /// - [`VisualizerEvent::Hide`] when a navigation / gesture path finishes
    ///   (`CommitMove` / `CancelMove`) or after a discrete move flashes the
    ///   overlay.
    ///
    /// The receiver end can be owned by any independent listener — the GTK
    /// overlay, a debug logger, etc. The visualizer is responsible for
    /// interpreting these events and maintaining its own visibility state.
    pub fn set_visualizer(&mut self, tx: mpsc::Sender<VisualizerEvent>) {
        self.vis_tx = Some(tx);
    }

    /// Return a shared reference to the underlying grid.
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Current grid position. All monitors share this position.
    pub fn position(&self) -> GridPosition {
        self.monitor_positions
            .first()
            .map(|p| p.grid_position)
            .unwrap_or(GridPosition::ORIGIN)
    }

    /// Return the name of the currently focused monitor, or `None` if no
    /// monitor is focused.
    ///
    /// This delegates to the underlying [`WindowManager::active_monitor`].
    pub fn active_monitor(&self) -> Result<String, SwitcherError> {
        self.wm
            .active_monitor()
            .map_err(|e| SwitcherError::WindowManager(e.to_string()))?
            .ok_or_else(|| SwitcherError::WindowManager("no active monitor".to_string()))
    }

    /// Return the list of monitors the window manager knows about.
    ///
    /// This delegates to the underlying [`WindowManager::monitors`].
    pub fn monitors(&self) -> Result<Vec<crate::command::MonitorInfo>, SwitcherError> {
        self.wm
            .monitors()
            .map_err(|e| SwitcherError::WindowManager(e.to_string()))
    }

    /// Process a single [`Event`].
    pub fn handle_event(&mut self, event: Event) -> Result<(), SwitcherError> {
        match event {
            Event::Command(cmd) => self.handle(cmd),
            Event::MonitorsChanged => self.refresh_monitors(),
        }
    }

    pub fn refresh_monitors(&mut self) -> Result<(), SwitcherError> {
        let monitors = self
            .wm
            .monitors()
            .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
        
        self.reconcile_monitor_positions(monitors.into_iter().map(|m| m.name));

        if self.monitor_positions.is_empty() {
            return Ok(());
        }

        self.apply_current_workspace()?;

        Ok(())
    }

    fn reconcile_monitor_positions<I>(&mut self, monitor_names: I)
    where
        I: IntoIterator<Item = String>,
    {
        let fallback_pos = self.position();

        let old_by_name: HashMap<String, MonitorGridPosition> = self
            .monitor_positions
            .drain(..)
            .map(|p| (p.name.clone(), p))
            .collect();

        self.monitor_positions = monitor_names
            .into_iter()
            .map(|name| {
                old_by_name
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| MonitorGridPosition {
                        name,
                        grid_position: fallback_pos,
                    })
            })
            .collect();
    }

    /// Process a single [`Command`].
    ///
    /// Returns `Ok(())` on success.  If the window manager fails, the grid
    /// state has **already** been updated (the switcher is optimistic); a
    /// retry or recovery strategy is left to the caller.
    pub fn handle(&mut self, cmd: Command) -> Result<(), SwitcherError> {
        match cmd {
            Command::SwitchTo(target) => {
                let pos = target.grid_position();
                info!("switch to ({}, {})", pos.col, pos.row);
                self.grid.grow_to_contain(pos);
                for mp in &mut self.monitor_positions {
                    mp.grid_position = pos;
                }
                // Always apply so re-sending current cell or clicking it can repair desync.
                if let Err(e) = self.apply_current_workspace() {
                    warn!("switch failed (will still finalize visualizer): {}", e);
                }
                // Always flash and hide the visualizer so the UI finalizes.
                self.show_visualizer(0.0, 0.0);
                self.hide_visualizer();
            }

            Command::Go(dir) => {
                info!("go {}", dir);
                self.go(dir)?;
            }

            Command::MoveWindowAndGo(dir) => {
                info!("move window and go {}", dir);
                self.move_and_go(dir)?
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

            Command::MoveWindowToMonitorIndex(MonitorIndex(idx)) => {
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
                debug!("cancel move — switch to current workspace");
                return self.handle(Command::SwitchTo(SwitchTo::from_grid_position(
                    self.position(),
                )));
            }

            Command::CommitMove(dir) => {
                info!("commit move {}", dir);
                // Gesture-based plain move reuses the same path as `Go`.
                self.go(dir)?;
            }

            Command::ToggleVisualizer => {
                debug!("toggle visualizer");
                self.toggle_manual_visualizer();
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
                let state = if let Some(ref mut swipe) = self.active_swipe {
                    swipe.dx += dx;
                    swipe.dy += dy;
                    let cfg = &self.gesture_config;
                    let (norm_dx, norm_dy) = normalised_swipe_offset(
                        swipe.dx,
                        swipe.dy,
                        cfg.sensitivity,
                        cfg.natural_swiping,
                    );
                    let fingers = swipe.fingers;
                    let commit_while = cfg.commit_while_dragging_threshold.and_then(|t| {
                        dominant_direction(norm_dx, norm_dy, t)
                            .map(|dir| (dir, fingers == cfg.move_fingers))
                    });
                    Some((norm_dx, norm_dy, commit_while))
                } else {
                    None
                };
                if let Some((norm_dx, norm_dy, commit_while)) = state {
                    debug!("swipe update: dx={:.2} dy={:.2}", norm_dx, norm_dy);
                    self.show_visualizer(norm_dx, norm_dy);
                    if let Some((dir, move_window)) = commit_while {
                        if let Err(e) = self.execute_swipe_commit(dir, move_window) {
                            warn!("swipe commit while dragging: {}", e);
                        }
                        self.active_swipe = None;
                    }
                }
            }

            Command::SwipeEnd => {
                if let Some(swipe) = self.active_swipe.take() {
                    let cfg = &self.gesture_config;
                    let (norm_dx, norm_dy) = normalised_swipe_offset(
                        swipe.dx,
                        swipe.dy,
                        cfg.sensitivity,
                        cfg.natural_swiping,
                    );

                    match dominant_direction(norm_dx, norm_dy, cfg.commit_threshold) {
                        Some(dir) => {
                            let move_window = swipe.fingers == cfg.move_fingers;
                            self.execute_swipe_commit(dir, move_window)?;
                        }
                        None => {
                            debug!("swipe cancel (below threshold) — switch to current workspace");
                            self.handle(Command::SwitchTo(SwitchTo::from_grid_position(
                                self.position(),
                            )))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    //  Visualizer helpers 

    /// Show the visualizer with the current grid state (plus gesture offsets)
    /// as an **automatically** shown overlay.
    fn show_visualizer(&mut self, offset_x: f64, offset_y: f64) {
        if let Some(tx) = &self.vis_tx {
            let payload = self.visualizer_show_payload(offset_x, offset_y);
            let _ = tx.send(VisualizerEvent::ShowAuto(payload));
        }
    }

    /// Toggle the visualizer in **manual** mode for the current grid state.
    fn toggle_manual_visualizer(&mut self) {
        if let Some(tx) = &self.vis_tx {
            let payload = self.visualizer_show_payload(0.0, 0.0);
            let _ = tx.send(VisualizerEvent::ToggleManual(payload));
        }
    }

    /// Build a `VisualizerShowPayload` with state and monitor info.
    fn visualizer_show_payload(&self, offset_x: f64, offset_y: f64) -> VisualizerShowPayload {
        let state = self.visualizer_state(offset_x, offset_y);
        let active_monitor_name = self
            .wm
            .active_monitor()
            .ok()
            .flatten();
        let monitors = self
            .wm
            .monitors()
            .unwrap_or_default();
        VisualizerShowPayload {
            state,
            active_monitor_name,
            monitors,
        }
    }

    /// Build a `VisualizerState` from the current grid and position (for tests
    /// and visualizer integration).
    ///
    /// When gesture offsets are present, `target_cell` is set only once the
    /// offset reaches the commit threshold (same as "release to switch").
    pub fn visualizer_state(&self, offset_x: f64, offset_y: f64) -> VisualizerState {
        let (cols, rows) = self.grid.dimensions();
        let position = self.position();
        let target_cell = dominant_direction(
            offset_x,
            offset_y,
            self.gesture_config.commit_threshold,
        )
        .map(|dir| Grid::get_abs_from(dir, position));
        VisualizerState {
            target_cell,
            ..VisualizerState::new(cols, rows, position, offset_x, offset_y)
        }
    }

    /// Request that the visualizer be hidden.
    ///
    /// Sends a [`VisualizerEvent::Hide`] to the visualizer channel, if one is
    /// attached. The visualizer uses its own internal state to decide whether
    /// to hide instantly (manual) or via linger + fade (automatic).
    fn hide_visualizer(&mut self) {
        if let Some(tx) = &self.vis_tx {
            let _ = tx.send(VisualizerEvent::Hide);
        }
    }

    /// Tell the window manager to switch every monitor to the workspace positions
    /// derived from the current grid cell.
    fn apply_current_workspace(&self) -> Result<(), SwitcherError> {
        let position = self.position();

        let active = self.
            active_monitor()?;

        // TODO: still required ? 
        let mut entries: Vec<(&str, GridPosition)> = self
            .monitor_positions
            .iter()
            .enumerate()
            .map(|(_, p)| {
                (
                    p.name.as_str(),
                    position,
                )
            })
            .collect();

        // Move current monitor to the end to make sure it is switched last.
        if let Some(pos) = entries.iter().position(|(mon, _)| *mon == active.as_str()) {
            let entry = entries.remove(pos);
            entries.push(entry);
        }

        for (monitor, pos) in &entries {
            self.wm
                .switch_workspace(monitor, *pos)
                .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;
        }

        Ok(())
    }

    /// Execute a swipe commit in the given direction (plain go or move window and go).
    fn execute_swipe_commit(
        &mut self,
        dir: Direction,
        move_window: bool,
    ) -> Result<(), SwitcherError> {
        if move_window {
            info!("swipe commit: move window and go {}", dir);
            self.move_and_go(dir)
        } else {
            info!("swipe commit: go {}", dir);
            self.go(dir)
        }
    }

    /// Core logic for a discrete workspace move in a direction.
    fn go(&mut self, dir: Direction) -> Result<(), SwitcherError> {
        let pos = Grid::get_abs_from(dir, self.position());
        self.grid.grow_to_contain(pos);
        for mp in &mut self.monitor_positions {
            mp.grid_position = pos;
        }
        self.show_visualizer(0.0, 0.0);
        self.hide_visualizer();
        self.apply_current_workspace()?;
        Ok(())
    }

    fn move_and_go(&mut self, dir : Direction) -> Result<(), SwitcherError> {
        let target_pos = Grid::get_abs_from(dir, self.position());

        // Figure out which monitor is currently focused so we move
        // the window to the workspace slice for that monitor.
        let active = self
            .active_monitor()?;

        self.wm
            .move_window_to_workspace(&active, target_pos)
            .map_err(|e| SwitcherError::WindowManager(e.to_string()))?;

        self.go(dir)
    }

}

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Direction, MonitorInfo, WindowInfo};
    use crate::traits::VisualizerEvent;
    use std::cell::RefCell;
    use std::sync::mpsc;

    /// Record-keeping mock window manager.
    #[derive(Debug, Default)]
    struct RecorderWm {
        switches: RefCell<Vec<(String, GridPosition)>>,
        moves: RefCell<Vec<GridPosition>>,
        monitor_moves: RefCell<Vec<String>>,
        /// Tracks which monitor is currently "focused" in the mock, i.e. where
        /// the mouse cursor would be. We model Hyprland's behaviour where
        /// `switch_workspace` focuses the target monitor.
        focused_monitor: RefCell<Option<String>>,
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

        fn switch_workspace(&self, mon: &str, pos: GridPosition) -> Result<(), RecorderErr> {
            self.switches.borrow_mut().push((mon.into(), pos));
            // Simulate Hyprland: switching a workspace on a monitor also
            // focuses that monitor.
            *self.focused_monitor.borrow_mut() = Some(mon.into());
            Ok(())
        }

        fn move_window_to_workspace(&self, _ : &str, pos: GridPosition) -> Result<(), RecorderErr> {
            self.moves.borrow_mut().push(pos);
            Ok(())
        }

        fn move_window_to_monitor(&self, monitor: &str) -> Result<(), RecorderErr> {
            self.monitor_moves.borrow_mut().push(monitor.into());
            Ok(())
        }

        fn active_monitor(&self) -> Result<Option<String>, RecorderErr> {
            Ok(self
                .focused_monitor
                .borrow()
                .clone()
                .or_else(|| Some("DP-1".into())))
        }

        fn active_window(&self) -> Result<Option<WindowInfo>, RecorderErr> {
            // The active window is always reported on the currently focused
            // monitor. Default to "DP-1" if nothing has focused yet.
            let monitor = self
                .focused_monitor
                .borrow()
                .clone()
                .unwrap_or_else(|| "DP-1".to_string());
            Ok(Some(WindowInfo {
                address: "0xbeef".into(),
                title: "test".into(),
                monitor,
            }))
        }
    }

    /// Window manager that enforces that `active_window` is queried before
    /// `move_window_to_workspace`. This models the requirement that
    /// `MoveWindowAndGo` must take the currently focused monitor into account
    /// when deciding which workspace to move the window to.
    #[derive(Debug, Default)]
    struct OrderCheckingWm {
        switches: RefCell<Vec<(String, GridPosition)>>,
        moves: RefCell<Vec<GridPosition>>,
        active_monitor_queried_before_move: RefCell<bool>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("order-checking error")]
    struct OrderCheckingErr;

    impl WindowManager for OrderCheckingWm {
        type Error = OrderCheckingErr;

        fn monitors(&self) -> Result<Vec<MonitorInfo>, OrderCheckingErr> {
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

        fn switch_workspace(&self, mon: &str, pos: GridPosition) -> Result<(), OrderCheckingErr> {
            self.switches.borrow_mut().push((mon.into(), pos));
            Ok(())
        }

        fn move_window_to_workspace(&self, _ : &str, pos: GridPosition) -> Result<(), OrderCheckingErr> {
            // Fail if we have not yet queried the active monitor; `MoveWindowAndGo`
            // must know which monitor is focused before choosing a workspace position.
            if !*self.active_monitor_queried_before_move.borrow() {
                return Err(OrderCheckingErr);
            }
            self.moves.borrow_mut().push(pos);
            Ok(())
        }

        fn move_window_to_monitor(&self, _monitor: &str) -> Result<(), OrderCheckingErr> {
            Ok(())
        }

        fn active_monitor(&self) -> Result<Option<String>, OrderCheckingErr> {
            *self.active_monitor_queried_before_move.borrow_mut() = true;
            Ok(Some("DP-1".into()))
        }

        fn active_window(&self) -> Result<Option<WindowInfo>, OrderCheckingErr> {
            Ok(Some(WindowInfo {
                address: "0xbeef".into(),
                title: "order-check".into(),
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
        // They should have the same position
        let positions: Vec<GridPosition> = switches.iter().map(|(_, pos)| *pos).collect();
        assert_eq!(positions.len(), 2);
        // positions per monitor are the same
        assert_eq!(positions[0], positions[1]);
    }

    #[test]
    fn go_keeps_focused_monitor_on_original_display() {
        let mut s = make_switcher();
        // Initially, the active window is on DP-1.
        let initial = s.wm.active_window().unwrap().unwrap();
        assert_eq!(initial.monitor, "DP-1");
        // Moving in the grid with a plain Go command switches workspaces on
        // multiple monitors.
        s.handle(Command::Go(Direction::Right)).unwrap();
        // After switching, the "focused" monitor (where the cursor is) should
        // still be the original one.
        let after = s.wm.active_window().unwrap().unwrap();
        assert_eq!(after.monitor, "DP-1");
    }

    /// Same as `go_keeps_focused_monitor_on_original_display`, but starting
    /// from the secondary monitor. This mirrors the report that the cursor can
    /// jump to a different monitor even on plain `Go` commands.
    #[test]
    fn go_from_secondary_monitor_keeps_focus_on_that_monitor() {
        let mut s = make_switcher();
        // Pretend the user currently has the cursor on HDMI-A-1.
        *s.wm.focused_monitor.borrow_mut() = Some("HDMI-A-1".into());
        let initial = s.wm.active_window().unwrap().unwrap();
        assert_eq!(initial.monitor, "HDMI-A-1");

        s.handle(Command::Go(Direction::Right)).unwrap();

        // After switching workspaces, focus should remain on HDMI-A-1.
        let after = s.wm.active_window().unwrap().unwrap();
        assert_eq!(after.monitor, "HDMI-A-1");
    }

    /// When using the "movego" command (`MoveWindowAndGo`), we both move the
    /// focused window and switch workspaces on all monitors. This test asserts
    /// that the "focused" monitor (our proxy for where the mouse cursor is)
    /// stays on the same display as before the move, mirroring the bug
    /// described in the README where the cursor sometimes jumps to a different
    /// monitor when switching.
    #[test]
    fn move_window_and_go_keeps_focused_monitor_on_original_display() {
        let mut s = make_switcher();
        // Initially, the active window is on DP-1.
        let initial = s.wm.active_window().unwrap().unwrap();
        assert_eq!(initial.monitor, "DP-1");

        // Perform a move+switch operation.
        s.handle(Command::MoveWindowAndGo(Direction::Right))
            .unwrap();

        // After the operation, the focused monitor should remain DP-1.
        let after = s.wm.active_window().unwrap().unwrap();
        assert_eq!(after.monitor, "DP-1");
    }

    //  Visualizer integration

    /// Attach a visualizer channel to the switcher and collect all events it emits
    /// while handling `cmd`.
    fn collect_vis_events(cmd: Command) -> Vec<VisualizerEvent> {
        let mut s = make_switcher();
        let (tx, rx) = mpsc::channel();
        s.set_visualizer(tx);
        s.handle(cmd).unwrap();
        rx.try_iter().collect()
    }

    #[test]
    fn go_emits_show_and_hide_events() {
        let events = collect_vis_events(Command::Go(Direction::Right));
        assert!(
            matches!(events.as_slice(), [VisualizerEvent::ShowAuto(_), VisualizerEvent::Hide]),
            "Go should emit a Show followed by Hide, got: {events:#?}"
        );
    }

    #[test]
    fn move_window_and_go_emits_show_and_hide_events() {
        let events = collect_vis_events(Command::MoveWindowAndGo(Direction::Right));
        assert!(
            matches!(events.as_slice(), [VisualizerEvent::ShowAuto(_), VisualizerEvent::Hide]),
            "MoveWindowAndGo should emit a Show followed by Hide, got: {events:#?}"
        );
    }

    #[test]
    fn toggle_visualizer_pins_and_unpins_overlay() {
        let mut s = make_switcher();
        let (tx, rx) = mpsc::channel();
        s.set_visualizer(tx);

        // First toggle: should emit a ToggleManual with the current grid state.
        s.handle(Command::ToggleVisualizer).unwrap();
        let events1: Vec<VisualizerEvent> = rx.try_iter().collect();
        assert!(
            matches!(events1.as_slice(), [VisualizerEvent::ToggleManual(_)]),
            "first ToggleVisualizer should emit a single ToggleManual, got: {events1:#?}"
        );

        // Second toggle: should emit another ToggleManual (visualizer will turn this
        // into an instant hide based on its own internal state).
        s.handle(Command::ToggleVisualizer).unwrap();
        let events2: Vec<VisualizerEvent> = rx.try_iter().collect();
        assert!(
            matches!(events2.as_slice(), [VisualizerEvent::ToggleManual(_)]),
            "second ToggleVisualizer should emit a single ToggleManual, got: {events2:#?}"
        );
    }

    // The visualizer owns the high-level visibility state machine (Hidden /
    // ManuallyShown / AutomaticallyShown). The switcher only verifies that it
    // sends the appropriate event *types*; detailed behaviour (fade vs
    // instant hide) is tested on the visualizer side.

    #[test]
    fn switch_to_absolute() {
        let mut s = make_switcher();
        s.handle(Command::SwitchTo(SwitchTo::new(2, 1))).unwrap();
        assert_eq!(s.position(), GridPosition { col: 2, row: 1 });
        assert_eq!(s.grid().dimensions(), (3, 2));
    }

    #[test]
    fn move_window_and_go_records_move() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowAndGo(Direction::Right)).unwrap();
        let moves = s.wm.moves.borrow();
        assert_eq!(moves.len(), 1, "should have moved one window");
        assert_eq!(s.position(), GridPosition { col: 1, row: 0 });
    }

    /// `MoveWindowAndGo` must determine the active monitor *before* deciding
    /// which workspace id to move the window to. If it queries the active
    /// monitor only after moving, the chosen workspace may belong to a
    /// different monitor, causing the window to jump displays (as described in
    /// the README TODOs).
    #[test]
    fn move_window_and_go_queries_active_before_moving_window() {
        let wm = OrderCheckingWm::default();
        let mut s = GridSwitcher::new(wm, vec!["DP-1".into(), "HDMI-A-1".into()]);
        // Desired contract: `MoveWindowAndGo` must consult the active monitor
        // *before* moving the window, so this call should succeed when the
        // implementation is correct. With the fixed code it succeeds; if a
        // future refactor moves the window first (calling `move_window_to_workspace`
        // before `active_monitor`), this test will fail and catch the
        // regression.
        s.handle(Command::MoveWindowAndGo(Direction::Right))
            .expect("MoveWindowAndGo must query active_window before moving");
    }

    #[test]
    fn prepare_move_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::PrepareMove { dx: 0.5, dy: 0.0 })
            .unwrap();
        assert_eq!(s.position(), GridPosition::ORIGIN, "grid should not move");
    }

    #[test]
    fn cancel_move_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::CancelMove).unwrap();
        assert_eq!(s.position(), GridPosition::ORIGIN);
    }

    #[test]
    fn commit_move_advances_grid() {
        let mut s = make_switcher();
        s.handle(Command::CommitMove(Direction::Down)).unwrap();
        assert_eq!(s.position(), GridPosition { col: 0, row: 1 });
    }

    #[test]
    fn full_command_sequence() {
        let mut s = make_switcher();
        s.handle(Command::Go(Direction::Right)).unwrap();
        s.handle(Command::Go(Direction::Down)).unwrap();
        s.handle(Command::Go(Direction::Left)).unwrap();
        s.handle(Command::Go(Direction::Up)).unwrap();
        assert_eq!(s.position(), GridPosition::ORIGIN);
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
        assert_eq!(s.position(), GridPosition::ORIGIN, "grid should not move");
    }

    //  MoveWindowToMonitorIndex tests 

    #[test]
    fn move_window_to_monitor_index_valid() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitorIndex(MonitorIndex(1))).unwrap();
        let monitor_moves = s.wm.monitor_moves.borrow();
        assert_eq!(monitor_moves.len(), 1);
        assert_eq!(monitor_moves[0], "HDMI-A-1");
    }

    #[test]
    fn move_window_to_monitor_index_zero() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitorIndex(MonitorIndex(0))).unwrap();
        let monitor_moves = s.wm.monitor_moves.borrow();
        assert_eq!(monitor_moves.len(), 1);
        assert_eq!(monitor_moves[0], "DP-1");
    }

    #[test]
    fn move_window_to_monitor_index_out_of_range() {
        let mut s = make_switcher();
        let result = s.handle(Command::MoveWindowToMonitorIndex(MonitorIndex(5)));
        assert!(result.is_err());
    }

    #[test]
    fn move_window_to_monitor_index_does_not_change_grid() {
        let mut s = make_switcher();
        s.handle(Command::MoveWindowToMonitorIndex(MonitorIndex(1))).unwrap();
        assert_eq!(s.position(), GridPosition::ORIGIN, "grid should not move");
    }
}

