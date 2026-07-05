//! GTK4 + layer-shell visualizer that runs on the **main thread**.
//!
//! # Widget tree
//!
//! ```text
//! window                         (layer-shell, transparent)
//! └ .grid-overlay              (dark rounded box)
//!     └ gtk4::Overlay
//!         ├ .grid              (GtkGrid, main child)
//!         │   ├ .grid-cell     (dim base colour)
//!         │   └ …
//!         └ .grid-cursor       (overlay child, animated position)
//! ```
//!
//! # CSS selectors
//!
//! | Selector               | Targets                                       |
//! |------------------------|-----------------------------------------------|
//! | `window`               | The overlay window (keep transparent)          |
//! | `.grid-overlay`        | Container around the grid                      |
//! | `.grid`                | The `GtkGrid`                                  |
//! | `.grid-cell`           | Every cell                                     |
//! | `.grid-cell.active`    | Cell under the cursor (for user CSS hooks)     |
//! | `.grid-cell.target`    | Cell that will be switched to on release (gesture past threshold) |
//! | `.grid-cell.tobecreated.target` | Target cell for new row/column to be created    |
//! | `.grid-cursor`         | The sliding selector highlight                 |
//!
//! The `.grid-cursor` appearance is fully CSS-configurable.  Movement is
//! code-driven; timing is controlled by [`VisualizerConfig`].

use crate::command::{Command, MonitorInfo, SwitchTo};
use crate::common::GridPosition;
use crate::config::VisualizerConfig;
use crate::event::Event;
use crate::traits::{VisualizerEvent, VisualizerState};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::LayerShell;
use log::{debug, error, info, warn};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

//  Layout constants (must match the default CSS) 

const CELL_SIZE: i32 = 24;
const CELL_MARGIN: i32 = 3;
const CELL_PITCH: i32 = CELL_SIZE + 2 * CELL_MARGIN; // 30
/// Overlay padding (space between screen edge and grid overlay).
const OVERLAY_PADDING: i32 = 12;

/// Snap the gesture offset vector to the nearest multiple of 45° for display.
/// Preserves magnitude so the cursor travel feels consistent.
fn snap_offset_to_45_deg(offset_x: f64, offset_y: f64) -> (f64, f64) {
    let mag = (offset_x * offset_x + offset_y * offset_y).sqrt();
    if mag < 1e-10 {
        return (0.0, 0.0);
    }
    let angle = offset_y.atan2(offset_x);
    const FRAC_PI_4: f64 = std::f64::consts::FRAC_PI_4;
    let snapped = (angle / FRAC_PI_4).round() * FRAC_PI_4;
    let ox = mag * snapped.cos();
    let oy = mag * snapped.sin();
    (ox, oy)
}

/// Blend between actual offset and 45°-snapped offset. Alpha follows cubic-bezier(0.25, 0.1, 0.25, 1):
/// alpha = 1 at p-inf norm 0 (full user), alpha = 0 at p-inf norm 1 (one workspace, full snap).
fn interpolated_offset(offset_x: f64, offset_y: f64) -> (f64, f64) {
    let snapped = snap_offset_to_45_deg(offset_x, offset_y);
    let u = offset_x.abs().max(offset_y.abs()).min(1.0) as f32;
    if u < 1e-6 {
        return (offset_x, offset_y);
    }
    // ease_scalar(0, 1, u) = y(t) where x(t)=u; we want alpha = 1 - y
    let alpha = 1.0 - crate::bezier::ease_scalar(0.0, 1.0, u) as f64;
    let display_ox = alpha * offset_x + (1.0 - alpha) * snapped.0;
    let display_oy = alpha * offset_y + (1.0 - alpha) * snapped.1;
    (display_ox, display_oy)
}

//  Default CSS 

const DEFAULT_CSS: &str = r#"
window,
window.background {
    background-color: transparent;
    background: none;
}

.grid-overlay {
    background-color: rgba(0, 0, 0, 0.75);
    border-radius: 16px;
}

.grid {
    padding: 0;
}

.grid-cell {
    min-width: 24px;
    min-height: 24px;
    margin: 3px;
    border-radius: 6px;
    background-color: rgba(255, 255, 255, 0.08);
    transition: background-color 150ms ease-in-out;
}

.grid-cell.target {
    background-color: rgba(255, 255, 255, 0.22);
}

.grid-cursor {
    background-color: rgba(255, 255, 255, 0.9);
    border-radius: 6px;
}

.grid-overlay.mode-manual {
    cursor: pointer;
}

.grid-overlay.mode-manual .grid-cell {
    cursor: pointer;
}
"#;

//  Easing 

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

fn cell_px(index: usize) -> f64 {
    index as f64 * CELL_PITCH as f64 + CELL_MARGIN as f64
}

//  Overlay visibility state machine 

/// Tracks the show → linger → fade-out → hidden lifecycle.
enum Visibility {
    /// Overlay is hidden (`window.set_visible(false)`).
    Hidden,
    /// Overlay is fully opaque and actively showing content.
    Visible,
    /// Waiting before the fade-out starts.
    Lingering(Instant),
    /// Opacity is being animated from 1 → 0.
    Fading(Instant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShownKind {
    Hidden,
    ManuallyShown,
    AutomaticallyShown,
}

//  Cursor animation 

struct CursorAnim {
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
    start: Instant,
}

//  Persistent overlay grid 

struct OverlayGrid {
    grid_widget: gtk4::Grid,
    cursor: gtk4::Box,
    cells: Vec<gtk4::Box>,
    cols: usize,
    rows: usize,

    cur_x: f64,
    cur_y: f64,
    anim: Option<CursorAnim>,
    cursor_anim_dur: Duration,
    initialised: bool,
    on_cell_click: Option<Rc<dyn Fn(GridPosition)>>,
}

impl OverlayGrid {
    fn new(
        container: &gtk4::Box,
        config: &VisualizerConfig,
        on_cell_click: Option<Rc<dyn Fn(GridPosition)>>,
    ) -> Self {
        let overlay = gtk4::Overlay::new();

        let grid_widget = gtk4::Grid::new();
        grid_widget.add_css_class("grid");
        grid_widget.set_row_spacing(0);
        grid_widget.set_column_spacing(0);
        overlay.set_child(Some(&grid_widget));

        let cursor = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        cursor.add_css_class("grid-cursor");
        cursor.set_size_request(CELL_SIZE, CELL_SIZE);
        cursor.set_halign(gtk4::Align::Start);
        cursor.set_valign(gtk4::Align::Start);
        cursor.set_can_target(false);
        overlay.add_overlay(&cursor);
        overlay.set_measure_overlay(&cursor, false);

        overlay.set_margin_start(OVERLAY_PADDING);
        overlay.set_margin_end(OVERLAY_PADDING);
        overlay.set_margin_top(OVERLAY_PADDING);
        overlay.set_margin_bottom(OVERLAY_PADDING);
        container.append(&overlay);

        Self {
            grid_widget,
            cursor,
            cells: Vec::new(),
            cols: 0,
            rows: 0,
            cur_x: 0.0,
            cur_y: 0.0,
            anim: None,
            cursor_anim_dur: Duration::from_millis(config.cursor_animation_ms),
            initialised: false,
            on_cell_click,
        }
    }

    fn update(&mut self, state: &VisualizerState) {
        let is_gesture = state.offset_x != 0.0 || state.offset_y != 0.0;
        let effective_target = state
            .target_cell
            .unwrap_or(state.position);
        let display_cols = if is_gesture {
            state.cols.max(effective_target.col + 1)
        } else {
            state.cols
        };
        let display_rows = if is_gesture {
            state.rows.max(effective_target.row + 1)
        } else {
            state.rows
        };

        let dims_changed = display_cols != self.cols || display_rows != self.rows;
        if dims_changed {
            self.rebuild_cells(display_cols, display_rows, state.cols, state.rows);
        }
        self.apply_classes(state);

        let base_x = cell_px(state.position.col);
        let base_y = cell_px(state.position.row);
        let (display_ox, display_oy) = interpolated_offset(state.offset_x, state.offset_y);
        let mut target_x = base_x + display_ox * CELL_PITCH as f64;
        let mut target_y = base_y + display_oy * CELL_PITCH as f64;
        let min_px = CELL_MARGIN as f64;
        target_x = target_x.max(min_px);
        target_y = target_y.max(min_px);

        if !self.initialised {
            self.snap(target_x, target_y);
            self.initialised = true;
        } else if is_gesture {
            self.snap(target_x, target_y);
        } else {
            // For discrete moves, the state carries an `origin` — the cell the
            // switcher was on *before* this move.  If our cached cursor position
            // doesn't match, snap to the origin first so the animation always
            // starts from the correct cell.
            let origin_x = cell_px(state.origin.col);
            let origin_y = cell_px(state.origin.row);
            let (cx, cy) = (self.cur_x, self.cur_y);
            if (origin_x - cx).abs() > 0.5 || (origin_y - cy).abs() > 0.5 {
                self.snap(origin_x, origin_y);
            }
            let (ctx, cty) = self.current_target();
            if (target_x - ctx).abs() > 0.5 || (target_y - cty).abs() > 0.5 {
                self.animate_to(target_x, target_y);
            }
        }
    }

    fn tick(&mut self) {
        if let Some(ref anim) = self.anim {
            let dur = self.cursor_anim_dur.as_secs_f64();
            let t = if dur > 0.0 {
                (anim.start.elapsed().as_secs_f64() / dur).min(1.0)
            } else {
                1.0
            };
            let e = ease_out_cubic(t);

            self.cur_x = anim.from_x + (anim.to_x - anim.from_x) * e;
            self.cur_y = anim.from_y + (anim.to_y - anim.from_y) * e;
            self.apply_cursor_pos();

            if t >= 1.0 {
                self.anim = None;
            }
        }
    }

    //  internals 

    fn snap(&mut self, x: f64, y: f64) {
        self.cur_x = x;
        self.cur_y = y;
        self.anim = None;
        self.apply_cursor_pos();
    }

    fn animate_to(&mut self, x: f64, y: f64) {
        self.anim = Some(CursorAnim {
            from_x: self.cur_x,
            from_y: self.cur_y,
            to_x: x,
            to_y: y,
            start: Instant::now(),
        });
    }

    fn current_target(&self) -> (f64, f64) {
        match &self.anim {
            Some(a) => (a.to_x, a.to_y),
            None => (self.cur_x, self.cur_y),
        }
    }

    fn apply_cursor_pos(&self) {
        self.cursor.set_margin_start(self.cur_x.round() as i32);
        self.cursor.set_margin_top(self.cur_y.round() as i32);
    }

    fn rebuild_cells(
        &mut self,
        display_cols: usize,
        display_rows: usize,
        _base_cols: usize,
        _base_rows: usize,
    ) {
        for cell in self.cells.drain(..) {
            self.grid_widget.remove(&cell);
        }
        self.cols = display_cols;
        self.rows = display_rows;
        self.cells.reserve(display_cols * display_rows);

        for row in 0..display_rows {
            for col in 0..display_cols {
                let cell = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
                cell.add_css_class("grid-cell");
                cell.set_size_request(CELL_SIZE, CELL_SIZE);
                self.grid_widget
                    .attach(&cell, col as i32, row as i32, 1, 1);

                if let Some(ref on_click) = self.on_cell_click {
                    let gesture = gtk4::GestureClick::new();
                    gesture.set_button(1); // left mouse button
                    let cb: Rc<dyn Fn(GridPosition)> = Rc::clone(on_click);
                    gesture.connect_released(move |_, _, _, _| {
                        cb(GridPosition { col, row });
                    });
                    cell.add_controller(gesture);
                }

                self.cells.push(cell);
            }
        }
    }

    fn apply_classes(&self, state: &VisualizerState) {
        let is_gesture = state.offset_x != 0.0 || state.offset_y != 0.0;
        let effective_target = state
            .target_cell
            .unwrap_or(state.position);

        let is_tobecreated_cell = is_gesture
            && (effective_target.col >= state.cols || effective_target.row >= state.rows);

        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.cells[row * self.cols + col];
                let is_active = col == state.position.col && row == state.position.row;
                let is_target = is_gesture
                    && col == effective_target.col
                    && row == effective_target.row;
                let is_tobecreated = is_tobecreated_cell
                    && col == effective_target.col
                    && row == effective_target.row;

                if is_active {
                    cell.add_css_class("active");
                } else {
                    cell.remove_css_class("active");
                }
                if is_target {
                    cell.add_css_class("target");
                } else {
                    cell.remove_css_class("target");
                }
                if is_tobecreated {
                    cell.add_css_class("tobecreated");
                } else {
                    cell.remove_css_class("tobecreated");
                }
            }
        }
    }
}

/// Resolve the GDK monitor for the given active monitor name and WM monitor list.
/// Matches by position (x, y) since monitor names from the WM (e.g., "DP-1")
/// may not match GDK monitor identifiers.
///
/// Returns `None` (rather than panicking) on any failure — missing display,
/// mid-iteration monitor list mutation, etc. — so that monitor hot-plug or
/// DPMS events on the GTK main thread cannot bring the process down.
fn get_active_monitor(
    active_monitor_name: Option<&str>,
    monitors: &[MonitorInfo],
) -> Option<gdk::Monitor> {
    let active_name = active_monitor_name?;
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => {
            warn!(target: "hyprgrd::visualizer", "no GDK display while resolving monitor");
            return None;
        }
    };

    let active_monitor_info = monitors.iter().find(|m| m.name == active_name)?;
    display
        .monitors()
        .iter::<gdk::Monitor>()
        .find_map(|res| {
            let m = match res {
                Ok(m) => m,
                Err(e) => {
                    warn!(
                        target: "hyprgrd::visualizer",
                        "monitor list mutated during iteration: {}", e
                    );
                    return None;
                }
            };
            let geometry = m.geometry();
            if geometry.x() == active_monitor_info.x && geometry.y() == active_monitor_info.y {
                Some(m)
            } else {
                None
            }
        })
}

//  Public API 

/// Run the GTK4 main loop on the **current** (main) thread.
///
/// The visualizer is decoupled from the switcher: it receives events via
/// `vis_rx`, dispatches commands (e.g. from cell clicks) via `cmd_tx`, and
/// forwards incoming commands via `dispatch`. It never holds a reference
/// to the switcher.
pub fn run_main_loop(
    event_rx: mpsc::Receiver<Event>,
    vis_rx: mpsc::Receiver<VisualizerEvent>,
    event_tx: mpsc::Sender<Event>,
    dispatch: Box<dyn FnMut(Event)>,
    initial_state: VisualizerState,
    css_path: Option<PathBuf>,
    vis_config: VisualizerConfig,
) {
    let linger_dur = Duration::from_millis(vis_config.linger_ms);
    let fade_dur = Duration::from_millis(vis_config.fade_out_ms);

    gtk4::init().expect("failed to initialise GTK4");
    info!("GTK4 initialised on main thread");

    load_css(&css_path);

    // Per-monitor overlay cache.
    //
    // We keep one layer-shell window per monitor (lazily created on first
    // use) because wlr-layer-shell forbids changing the output of a mapped
    // surface — the previous "single window, set_monitor on every show"
    // approach corrupted the wl_surface and crashed the GL renderer.
    // Each window calls `set_monitor()` exactly once, before its first
    // `present()`, and is then reused for the lifetime of the process.
    let shown_kind = Rc::new(Cell::new(ShownKind::Hidden));
    let current_shown: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let on_cell_click: Rc<dyn Fn(GridPosition)> = {
        let event_tx = event_tx.clone();
        let shown_kind = Rc::clone(&shown_kind);
        Rc::new(move |pos| {
            if shown_kind.get() != ShownKind::ManuallyShown {
                return;
            }
            let event = Event::Command(Command::SwitchTo(SwitchTo::from_grid_position(pos)));
            if let Err(e) = event_tx.send(event) {
                error!(target: "hyprgrd::visualizer", "failed to dispatch cell click: {}", e);
            }
        })
    };

    let overlays: Rc<RefCell<HashMap<String, MonitorOverlay>>> =
        Rc::new(RefCell::new(HashMap::new()));

    info!(
        "visualizer ready (cursor {}ms, linger {}ms, fade {}ms, CSS: {}); initial state {}x{} at ({}, {})",
        vis_config.cursor_animation_ms,
        vis_config.linger_ms,
        vis_config.fade_out_ms,
        css_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<built-in>".into()),
        initial_state.cols,
        initial_state.rows,
        initial_state.position.col,
        initial_state.position.row,
    );

    //  Main event loop (~60 fps)
    let dispatch_cell = Rc::new(RefCell::new(dispatch));
    let shown_kind_for_loop = Rc::clone(&shown_kind);
    let dispatch_for_loop = Rc::clone(&dispatch_cell);
    let overlays_for_loop = Rc::clone(&overlays);
    let current_shown_for_loop = Rc::clone(&current_shown);
    let on_cell_click_for_loop = Rc::clone(&on_cell_click);
    let vis_config_for_loop = vis_config.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        // 1. Drain events and forward to the switcher via the dispatch callback.
        let mut disconnected = false;
        loop {
            match event_rx.try_recv() {
                Ok(event) => {
                    debug!("event: {:?}", event);
                    if matches!(&event, Event::MonitorsChanged) {
                        reset_overlay_cache(
                            &overlays_for_loop,
                            &current_shown_for_loop,
                            &shown_kind_for_loop,
                        );

                        // Drop visualizer events generated against the old monitor topology.
                        while vis_rx.try_recv().is_ok() {}
                    }
                    dispatch_for_loop.borrow_mut()(event);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        // 2. Drain visualizer events.
        while let Ok(event) = vis_rx.try_recv() {
            match event {
                VisualizerEvent::ShowAuto(payload) => {
                    let state = payload.state.clone();
                    debug!(
                        target: "hyprgrd::visualizer",
                        "SHOW_AUTO {}x{} pos=({},{}) off=({:.2},{:.2})",
                        state.cols,
                        state.rows,
                        state.position.col,
                        state.position.row,
                        state.offset_x, state.offset_y
                    );
                    show_on_target_monitor(
                        &overlays_for_loop,
                        &current_shown_for_loop,
                        &on_cell_click_for_loop,
                        &vis_config_for_loop,
                        &payload.active_monitor_name,
                        &payload.monitors,
                        &state,
                        ShowMode::Auto,
                    );
                    shown_kind_for_loop.set(ShownKind::AutomaticallyShown);
                }
                VisualizerEvent::ToggleManual(payload) => {
                    let state = payload.state.clone();
                    match shown_kind_for_loop.get() {
                        ShownKind::ManuallyShown => {
                            debug!(target: "hyprgrd::visualizer", "TOGGLE_MANUAL → hide (instant)");
                            hide_currently_shown(
                                &overlays_for_loop,
                                &current_shown_for_loop,
                                /*instant=*/ true,
                                /*linger=*/ Duration::ZERO,
                            );
                            shown_kind_for_loop.set(ShownKind::Hidden);
                        }
                        ShownKind::Hidden | ShownKind::AutomaticallyShown => {
                            debug!(
                                target: "hyprgrd::visualizer",
                                "TOGGLE_MANUAL → show {}x{} pos=({},{})",
                                state.cols,
                                state.rows,
                                state.position.col,
                                state.position.row
                            );
                            show_on_target_monitor(
                                &overlays_for_loop,
                                &current_shown_for_loop,
                                &on_cell_click_for_loop,
                                &vis_config_for_loop,
                                &payload.active_monitor_name,
                                &payload.monitors,
                                &state,
                                ShowMode::Manual,
                            );
                            shown_kind_for_loop.set(ShownKind::ManuallyShown);
                        }
                    }
                }
                VisualizerEvent::Hide => {
                    match shown_kind_for_loop.get() {
                        ShownKind::Hidden => {
                            debug!(target: "hyprgrd::visualizer", "HIDE (no-op, already hidden)");
                        }
                        ShownKind::ManuallyShown => {
                            debug!(target: "hyprgrd::visualizer", "HIDE (manual, instant)");
                            hide_currently_shown(
                                &overlays_for_loop,
                                &current_shown_for_loop,
                                /*instant=*/ true,
                                Duration::ZERO,
                            );
                            shown_kind_for_loop.set(ShownKind::Hidden);
                        }
                        ShownKind::AutomaticallyShown => {
                            debug!(
                                target: "hyprgrd::visualizer",
                                "HIDE (automatic, linger {}ms + fade {}ms)",
                                linger_dur.as_millis(),
                                fade_dur.as_millis()
                            );
                            hide_currently_shown(
                                &overlays_for_loop,
                                &current_shown_for_loop,
                                /*instant=*/ false,
                                linger_dur,
                            );
                            // shown_kind stays AutomaticallyShown until the
                            // fade finishes; the per-overlay state machine
                            // below will reset it.
                        }
                    }
                }
            }
        }

        // 3. Advance per-monitor cursor animation + visibility state machines.
        let mut overlays_mut = overlays_for_loop.borrow_mut();
        let mut clear_current_shown = false;
        for (key, overlay) in overlays_mut.iter_mut() {
            overlay.overlay_grid.tick();

            match overlay.visibility {
                Visibility::Hidden | Visibility::Visible => {}
                Visibility::Lingering(since) => {
                    if since.elapsed() >= linger_dur {
                        if fade_dur.is_zero() {
                            overlay.window.set_visible(false);
                            overlay.container.set_opacity(1.0);
                            overlay.visibility = Visibility::Hidden;
                            debug!(target: "hyprgrd::visualizer", "[{}] hidden after linger", key);
                            if current_shown_for_loop.borrow().as_deref() == Some(key.as_str()) {
                                clear_current_shown = true;
                                shown_kind_for_loop.set(ShownKind::Hidden);
                            }
                        } else {
                            overlay.visibility = Visibility::Fading(Instant::now());
                        }
                    }
                }
                Visibility::Fading(since) => {
                    let t = (since.elapsed().as_secs_f64() / fade_dur.as_secs_f64()).min(1.0);
                    overlay.container.set_opacity(1.0 - t);
                    if t >= 1.0 {
                        overlay.window.set_visible(false);
                        overlay.container.set_opacity(1.0);
                        overlay.visibility = Visibility::Hidden;
                        debug!(target: "hyprgrd::visualizer", "[{}] hidden after fade", key);
                        if current_shown_for_loop.borrow().as_deref() == Some(key.as_str()) {
                            clear_current_shown = true;
                            shown_kind_for_loop.set(ShownKind::Hidden);
                        }
                    }
                }
            }
        }
        drop(overlays_mut);
        if clear_current_shown {
            *current_shown_for_loop.borrow_mut() = None;
        }

        if disconnected {
            info!("all sources closed — exiting");
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });

    info!("entering GLib main loop");
    let main_loop = glib::MainLoop::new(None, false);
    main_loop.run();
    info!("GLib main loop exited");
}

//  Per-monitor overlay state 

/// One layer-shell overlay window, bound to a single GDK monitor for life.
///
/// Created lazily on the first show event that targets that monitor.
/// The output is set via `set_monitor()` exactly once, *before* the first
/// `present()` call, and never changed again — matching the wlr-layer-shell
/// rule that the output of a mapped layer surface is fixed at first commit.
struct MonitorOverlay {
    window: gtk4::Window,
    container: gtk4::Box,
    overlay_grid: OverlayGrid,
    visibility: Visibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShowMode {
    Auto,
    Manual,
}

/// Derive the per-monitor cache key. Prefer the WM-provided name (e.g.
/// `DP-1`) since that's what every show event gives us; fall back to a
/// geometry-based key when the monitor is unknown so we still keep one
/// window per output rather than churning a single anonymous one.
fn monitor_cache_key(active_name: Option<&str>, monitor: Option<&gdk::Monitor>) -> String {
    if let Some(n) = active_name {
        return n.to_string();
    }
    if let Some(m) = monitor {
        let g = m.geometry();
        return format!("@{},{}", g.x(), g.y());
    }
    "<compositor-default>".to_string()
}

#[allow(clippy::too_many_arguments)]
fn show_on_target_monitor(
    overlays: &Rc<RefCell<HashMap<String, MonitorOverlay>>>,
    current_shown: &Rc<RefCell<Option<String>>>,
    on_cell_click: &Rc<dyn Fn(GridPosition)>,
    vis_config: &VisualizerConfig,
    active_monitor_name: &Option<String>,
    monitors: &[MonitorInfo],
    state: &VisualizerState,
    mode: ShowMode,
) {
    let monitor = get_active_monitor(active_monitor_name.as_deref(), monitors);
    let key = monitor_cache_key(active_monitor_name.as_deref(), monitor.as_ref());

    // If a different monitor is currently shown, hide it first.
    let prev = current_shown.borrow().clone();
    if let Some(ref prev_key) = prev {
        if prev_key != &key {
            if let Some(prev_overlay) = overlays.borrow_mut().get_mut(prev_key) {
                debug!(
                    target: "hyprgrd::visualizer",
                    "[{}] hiding (switching active monitor → {})", prev_key, key
                );
                prev_overlay.window.set_visible(false);
                prev_overlay.container.set_opacity(1.0);
                prev_overlay.visibility = Visibility::Hidden;
            }
        }
    }

    let mut overlays_mut = overlays.borrow_mut();
    let overlay = overlays_mut.entry(key.clone()).or_insert_with(|| {
        info!(
            target: "hyprgrd::visualizer",
            "[{}] creating new layer-shell window", key
        );
        build_monitor_overlay(monitor.as_ref(), &key, on_cell_click, vis_config)
    });

    overlay.overlay_grid.update(state);
    overlay.container.set_opacity(1.0);
    match mode {
        ShowMode::Auto => {
            overlay.container.remove_css_class("mode-manual");
            overlay.container.add_css_class("mode-auto");
            overlay.window.set_cursor_from_name(None::<&str>);
        }
        ShowMode::Manual => {
            overlay.container.remove_css_class("mode-auto");
            overlay.container.add_css_class("mode-manual");
            overlay.window.set_cursor_from_name(Some("pointer"));
        }
    }
    debug!(target: "hyprgrd::visualizer", "[{}] set_visible(true) + present()", key);
    overlay.window.set_visible(true);
    overlay.window.present();
    overlay.visibility = Visibility::Visible;

    *current_shown.borrow_mut() = Some(key);
}

fn build_monitor_overlay(
    monitor: Option<&gdk::Monitor>,
    key: &str,
    on_cell_click: &Rc<dyn Fn(GridPosition)>,
    vis_config: &VisualizerConfig,
) -> MonitorOverlay {
    let window = gtk4::Window::new();
    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);
    window.set_namespace("hyprgrd");
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
    window.set_decorated(false);
    window.remove_css_class("background");

    // CRITICAL: set_monitor MUST happen before the first present() (which
    // commits the wl_surface). After that point wlr-layer-shell freezes
    // the output and any further set_monitor call is a protocol violation.
    if let Some(m) = monitor {
        let g = m.geometry();
        info!(
            target: "hyprgrd::visualizer",
            "[{}] set_monitor before first map → ({}, {})",
            key, g.x(), g.y()
        );
        window.set_monitor(m);
    } else {
        info!(
            target: "hyprgrd::visualizer",
            "[{}] no resolved GDK monitor — compositor will pick the output",
            key
        );
    }

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("grid-overlay");
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    window.set_child(Some(&container));

    let overlay_grid = OverlayGrid::new(&container, vis_config, Some(Rc::clone(on_cell_click)));

    MonitorOverlay {
        window,
        container,
        overlay_grid,
        visibility: Visibility::Hidden,
    }
}

fn hide_currently_shown(
    overlays: &Rc<RefCell<HashMap<String, MonitorOverlay>>>,
    current_shown: &Rc<RefCell<Option<String>>>,
    instant: bool,
    linger_dur: Duration,
) {
    let key = match current_shown.borrow().clone() {
        Some(k) => k,
        None => return,
    };
    let mut overlays_mut = overlays.borrow_mut();
    let Some(overlay) = overlays_mut.get_mut(&key) else {
        return;
    };
    if instant {
        overlay.window.set_visible(false);
        overlay.container.set_opacity(1.0);
        overlay.container.remove_css_class("mode-auto");
        overlay.container.remove_css_class("mode-manual");
        overlay.window.set_cursor_from_name(None::<&str>);
        overlay.visibility = Visibility::Hidden;
        drop(overlays_mut);
        *current_shown.borrow_mut() = None;
    } else {
        overlay.visibility = Visibility::Lingering(Instant::now());
        let _ = linger_dur; // duration applied by the per-overlay state machine
    }
}


/// Drop cached GTK/layer-shell windows after a monitor topology change.
///
/// Cached layer-shell windows can outlive the GDK output
/// objects they were bound to after hotplug, DPMS, dock/undock, or
/// suspend/resume.
fn reset_overlay_cache(
    overlays: &Rc<RefCell<HashMap<String, MonitorOverlay>>>,
    current_shown: &Rc<RefCell<Option<String>>>,
    shown_kind: &Rc<Cell<ShownKind>>,
) {
    let mut overlays_mut = overlays.borrow_mut();
    let count = overlays_mut.len();

    if count > 0 {
        info!(
            target: "hyprgrd::visualizer",
            "monitor topology changed; dropping {} cached layer-shell overlay window(s)",
            count
        );
    }

    for (_key, overlay) in overlays_mut.drain() {
        overlay.window.set_visible(false);
    }
    drop(overlays_mut);

    *current_shown.borrow_mut() = None;
    shown_kind.set(ShownKind::Hidden);
}

//  CSS loading 

fn load_css(css_path: &Option<PathBuf>) {
    let provider = gtk4::CssProvider::new();

    let css_content = match css_path.as_ref().filter(|p| p.exists()) {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(content) => {
                info!("user CSS: {} ({} bytes)", p.display(), content.len());
                content
            }
            Err(e) => {
                warn!("CSS read failed ({}): {} — using built-in", p.display(), e);
                DEFAULT_CSS.to_string()
            }
        },
        None => {
            info!("no user CSS — using built-in default");
            DEFAULT_CSS.to_string()
        }
    };

    #[allow(deprecated)]
    provider.load_from_data(&css_content);

    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        info!("CSS registered on display");
    } else {
        warn!("no GDK display — CSS will not be applied");
    }
}
