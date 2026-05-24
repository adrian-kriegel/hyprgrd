//! Core traits that decouple hyprgrd from any specific window manager or
//! transport mechanism.
//!
//! Every concrete backend (Hyprland, a Unix-socket listener, a test harness,
//! …) implements one of these traits.  The [`GridSwitcher`](crate::switcher::GridSwitcher)
//! only depends on these abstractions.

use crate::{
    command::{MonitorInfo, WindowInfo},
    common::GridPosition,
    event::Event,
};
use std::sync::mpsc;

/// Payload for visualizer events that show the overlay (ShowAuto, ToggleManual).
/// Includes grid state and monitor info so the visualizer can position the
/// overlay without querying the switcher.
#[derive(Debug, Clone)]
pub struct VisualizerShowPayload {
    pub state: VisualizerState,
    /// Name of the currently focused monitor (for layer-shell positioning).
    pub active_monitor_name: Option<String>,
    /// Monitor list from the window manager (to resolve GDK monitor by position).
    pub monitors: Vec<MonitorInfo>,
}

/// Abstraction over a window manager that can switch workspaces and move
/// windows.
///
/// An implementation might talk to Hyprland via IPC, or it might be a
/// no-op stub used in tests.
pub trait WindowManager {
    /// The error type produced by this window manager.
    type Error: std::error::Error + Send + 'static;

    /// Return the list of monitors the window manager knows about.
    fn monitors(&self) -> Result<Vec<MonitorInfo>, Self::Error>;

    /// Switch `monitor` to `workspace_id`.
    ///
    /// The workspace id has been allocated by [`Grid`](crate::grid::Grid)
    /// and is an opaque integer meaningful to the window manager.
    fn switch_workspace(&self, monitor: &str, workspace_id: GridPosition) -> Result<(), Self::Error>;

    /// Move the currently focused window to `workspace_id` **and** switch
    /// the active monitor to that workspace so the user follows the window.
    fn move_window_to_workspace(&self, monitor: &str, workspace_id: GridPosition) -> Result<(), Self::Error>;

    /// Move the currently focused window to the given monitor.
    ///
    /// The window lands on whatever workspace that monitor is currently
    /// displaying.  The focus may or may not follow, depending on the
    /// backend.
    fn move_window_to_monitor(&self, monitor: &str) -> Result<(), Self::Error>;

    /// Return the name of the currently focused monitor, or `None` if no
    /// monitor is focused.
    ///
    /// On Hyprland this typically corresponds to the monitor whose JSON
    /// description has `focused: true`.
    fn active_monitor(&self) -> Result<Option<String>, Self::Error>;

    /// Return information about the currently focused window, or `None` if
    /// no window is focused.
    fn active_window(&self) -> Result<Option<WindowInfo>, Self::Error>;
}

//  Visualizer 

/// A snapshot of the grid state that a [`Visualizer`] needs in order to
/// render.
///
/// Constructed via [`VisualizerState::new`].
#[derive(Debug, Clone)]
pub struct VisualizerState {
    /// Total columns in the grid.
    pub cols: usize,
    /// Total rows in the grid.
    pub rows: usize,
    /// Current grid cell.
    pub position: GridPosition,
    /// Gesture offset on the X axis, normalised to `[-1.0, 1.0]`.
    /// `0.0` when no gesture is active.
    pub offset_x: f64,
    /// Gesture offset on the Y axis, normalised to `[-1.0, 1.0]`.
    /// `0.0` when no gesture is active.
    pub offset_y: f64,
    /// When sliding with the touchpad, the cell that would be switched to on
    /// release. Set only once the gesture has reached the commit threshold.
    pub target_cell: Option<GridPosition>,
}

impl VisualizerState {
    /// Build a state snapshot from grid dimensions, position, and gesture offsets.
    pub fn new(
        cols: usize,
        rows: usize,
        position: GridPosition,
        offset_x: f64,
        offset_y: f64,
    ) -> Self {
        Self {
            cols,
            rows,
            position,
            offset_x,
            offset_y,
            target_cell: None,
        }
    }
}

/// Events sent from the [`GridSwitcher`](crate::switcher::GridSwitcher) to an
/// external visualizer over an [`mpsc`](std::sync::mpsc) channel.
///
/// The switcher holds an `Option<mpsc::Sender<VisualizerEvent>>`.  Any
/// listener — the GTK overlay, a debug logger, etc. — can receive these
/// events independently without being owned by the switcher.
///
/// The visualizer is responsible for maintaining its own visibility state
/// machine (Hidden / ManuallyShown / AutomaticallyShown) based on these
/// events. The switcher does **not** track that state; it only describes
/// *why* the overlay should be shown or hidden.
#[derive(Debug, Clone)]
pub enum VisualizerEvent {
    /// Show (or update) the overlay as an **automatically** shown overlay,
    /// e.g. in response to a navigation or gesture event.
    ///
    /// The visualizer should treat this as `AutomaticallyShown` and apply
    /// its normal fade-out behaviour when a subsequent [`Hide`] arrives.
    ShowAuto(VisualizerShowPayload),

    /// Toggle a **manually** shown overlay for the given grid state.
    ///
    /// The visualizer should switch between:
    ///
    /// - `Hidden` / `AutomaticallyShown` → `ManuallyShown` (show instantly)
    /// - `ManuallyShown` → `Hidden` (hide instantly, no fade)
    ToggleManual(VisualizerShowPayload),

    /// Request that the overlay be hidden.
    ///
    /// The exact behaviour depends on the visualizer's current state:
    ///
    /// - `ManuallyShown` → hide **instantly** (no fade)
    /// - `AutomaticallyShown` → go through linger + fade-out
    /// - `Hidden` → no-op
    Hide,
}

//  Event Source 

/// A source of [`Event`]s.
///
/// Implementations listen on some transport — a Unix socket, Hyprland's
/// IPC event stream, an in-memory channel, … — and forward parsed events
/// into the provided [`mpsc::Sender`].
///
/// The trait is deliberately transport-agnostic: the switcher does not know
/// (or care) whether events come from a socket, a gesture recognizer, or
/// a test harness.
///
/// # Contract
///
/// * [`run`](EventSource::run) **blocks** until the source is exhausted or
///   an unrecoverable error occurs.
/// * Implementations must be [`Send`] so they can run on a dedicated thread.
pub trait EventSource: Send {
    /// The error type produced by this source.
    type Error: std::error::Error + Send + 'static;

    /// Blocks until the source is exhausted or an unrecoverable error occurs.
    fn run(&mut self, sink: mpsc::Sender<Event>) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Direction, MonitorInfo, WindowInfo};
    use std::sync::mpsc;

    //  Mock WindowManager 

    /// A test double that records every call made to it.
    #[derive(Debug, Default)]
    struct MockWm {
        switch_log: std::cell::RefCell<Vec<(String, GridPosition)>>,
        move_log: std::cell::RefCell<Vec<GridPosition>>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("mock error")]
    struct MockError;

    impl WindowManager for MockWm {
        type Error = MockError;

        fn monitors(&self) -> Result<Vec<MonitorInfo>, MockError> {
            Ok(vec![MonitorInfo {
                name: "MOCK-1".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
            }])
        }

        fn switch_workspace(&self, monitor: &str, ws: GridPosition) -> Result<(), MockError> {
            self.switch_log
                .borrow_mut()
                .push((monitor.to_string(), ws));
            Ok(())
        }

        fn move_window_to_workspace(&self, _: &str, ws: GridPosition) -> Result<(), MockError> {
            self.move_log.borrow_mut().push(ws);
            Ok(())
        }

        fn move_window_to_monitor(&self, _monitor: &str) -> Result<(), MockError> {
            Ok(())
        }

        fn active_monitor(&self) -> Result<Option<String>, MockError> {
            Ok(Some("MOCK-1".into()))
        }

        fn active_window(&self) -> Result<Option<WindowInfo>, MockError> {
            Ok(Some(WindowInfo {
                address: "0xdead".into(),
                title: "mock".into(),
                monitor: "MOCK-1".into(),
            }))
        }
    }

    #[test]
    fn mock_wm_records_switches() {
        let wm = MockWm::default();
        let pos = GridPosition::from_coords(4, 2);
        wm.switch_workspace("MOCK-1", pos).unwrap();
        assert_eq!(wm.switch_log.borrow().len(), 1);
        assert_eq!(wm.switch_log.borrow()[0], ("MOCK-1".into(), pos));
    }

    //  Mock EventSource 

    /// A test double that emits a fixed sequence of events.
    struct MockSource {
        events: Vec<Event>,
    }

    impl EventSource for MockSource {
        type Error = MockError;

        fn run(&mut self, sink: mpsc::Sender<Event>) -> Result<(), MockError> {
            for event in self.events.drain(..) {
                let _ = sink.send(event);
            }
            Ok(())
        }
    }

    #[test]
    fn mock_source_emits_events() {
        let mut src = MockSource {
            events: vec![
                Event::Command(Command::Go(Direction::Right)),
                Event::Command(Command::SwitchTo(crate::command::SwitchTo::new(2, 1))),
            ],
        };
        let (tx, rx) = mpsc::channel();
        src.run(tx).unwrap();
        let events: Vec<Event> = rx.try_iter().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], Event::Command(Command::Go(Direction::Right)));
        assert_eq!(
            events[1],
            Event::Command(Command::SwitchTo(crate::command::SwitchTo::new(2, 1)))
        );
    }
}

