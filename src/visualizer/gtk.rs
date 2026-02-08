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
//!         │   ├ .grid-cell.visited
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
//! | `.grid-cell.visited`   | Previously visited cells                       |
//! | `.grid-cursor`         | The sliding selector highlight                 |
//!
//! The `.grid-cursor` appearance is fully CSS-configurable.  Movement is
//! code-driven; timing is controlled by [`VisualizerConfig`].

use crate::command::Command;
use crate::config::VisualizerConfig;
use crate::switcher::GridSwitcher;
use crate::traits::{VisualizerEvent, VisualizerState, WindowManager};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::LayerShell;
use log::{debug, error, info, warn};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

//  Layout constants (must match the default CSS) 

const CELL_SIZE: i32 = 24;
const CELL_MARGIN: i32 = 3;
const CELL_PITCH: i32 = CELL_SIZE + 2 * CELL_MARGIN; // 30

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
    padding: 12px;
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

.grid-cell.visited {
    background-color: rgba(255, 255, 255, 0.18);
}

.grid-cursor {
    background-color: rgba(255, 255, 255, 0.9);
    border-radius: 6px;
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
}

impl OverlayGrid {
    fn new(container: &gtk4::Box, config: &VisualizerConfig) -> Self {
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
        }
    }

    fn update(&mut self, state: &VisualizerState) {
        let dims_changed = state.cols != self.cols || state.rows != self.rows;
        if dims_changed {
            self.rebuild_cells(state.cols, state.rows);
        }
        self.apply_classes(state);

        let base_x = cell_px(state.col);
        let base_y = cell_px(state.row);
        let target_x = base_x + state.offset_x * CELL_PITCH as f64;
        let target_y = base_y + state.offset_y * CELL_PITCH as f64;

        let is_gesture = state.offset_x != 0.0 || state.offset_y != 0.0;

        if !self.initialised {
            self.snap(target_x, target_y);
            self.initialised = true;
        } else if is_gesture {
            self.snap(target_x, target_y);
        } else {
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

    fn rebuild_cells(&mut self, cols: usize, rows: usize) {
        for cell in self.cells.drain(..) {
            self.grid_widget.remove(&cell);
        }
        self.cols = cols;
        self.rows = rows;
        self.cells.reserve(cols * rows);

        for row in 0..rows {
            for col in 0..cols {
                let cell = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
                cell.add_css_class("grid-cell");
                cell.set_size_request(CELL_SIZE, CELL_SIZE);
                self.grid_widget
                    .attach(&cell, col as i32, row as i32, 1, 1);
                self.cells.push(cell);
            }
        }
    }

    fn apply_classes(&self, state: &VisualizerState) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.cells[row * self.cols + col];
                let is_active = col == state.col && row == state.row;
                let is_visited = !is_active && state.visited.contains(&(col, row));

                if is_active {
                    cell.add_css_class("active");
                } else {
                    cell.remove_css_class("active");
                }
                if is_visited {
                    cell.add_css_class("visited");
                } else {
                    cell.remove_css_class("visited");
                }
            }
        }
    }
}

//  Public API 

/// Run the GTK4 main loop on the **current** (main) thread.
pub fn run_main_loop<W: WindowManager + 'static>(
    mut switcher: GridSwitcher<W>,
    cmd_rx: mpsc::Receiver<Command>,
    css_path: Option<PathBuf>,
    vis_config: VisualizerConfig,
) {
    let linger_dur = Duration::from_millis(vis_config.linger_ms);
    let fade_dur = Duration::from_millis(vis_config.fade_out_ms);

    gtk4::init().expect("failed to initialise GTK4");
    info!("GTK4 initialised on main thread");

    load_css(&css_path);

    //  Layer-shell overlay window 
    let window = gtk4::Window::new();
    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);
    window.set_namespace("hyprgrd");
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
    window.set_decorated(false);
    window.remove_css_class("background");

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("grid-overlay");
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    window.set_child(Some(&container));

    //  Persistent overlay grid 
    let mut overlay_grid = OverlayGrid::new(&container, &vis_config);

    //  Initial render + present (maps the Wayland surface) 
    let initial_state = VisualizerState::from_grid(switcher.grid(), 0.0, 0.0);
    overlay_grid.update(&initial_state);
    window.present();
    window.set_visible(false);
    info!(
        "overlay mapped (hidden): {}x{} at ({}, {})",
        initial_state.cols, initial_state.rows, initial_state.col, initial_state.row
    );

    //  Visualizer channel 
    let (vis_tx, vis_rx) = mpsc::channel::<VisualizerEvent>();
    switcher.set_visualizer(vis_tx);

    info!(
        "visualizer ready (cursor {}ms, linger {}ms, fade {}ms, CSS: {})",
        vis_config.cursor_animation_ms,
        vis_config.linger_ms,
        vis_config.fade_out_ms,
        css_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<built-in>".into()),
    );

    //  Visibility state machine 
    let mut visibility = Visibility::Hidden;

    //  Main event loop (~60 fps) 
    glib::timeout_add_local(Duration::from_millis(16), move || {
        // 1. Drain commands.
        let mut disconnected = false;
        loop {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    debug!("command: {:?}", cmd);
                    if let Err(e) = switcher.handle(cmd) {
                        error!("command error: {}", e);
                    }
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
                VisualizerEvent::Show(state) => {
                    debug!(
                        "SHOW {}x{} pos=({},{}) off=({:.2},{:.2})",
                        state.cols, state.rows, state.col, state.row,
                        state.offset_x, state.offset_y
                    );
                    overlay_grid.update(&state);
                    // Cancel any linger / fade and become fully visible.
                    container.set_opacity(1.0);
                    window.set_visible(true);
                    window.present();
                    visibility = Visibility::Visible;
                }
                VisualizerEvent::Hide => {
                    debug!("HIDE (linger {}ms + fade {}ms)", linger_dur.as_millis(), fade_dur.as_millis());
                    visibility = Visibility::Lingering(Instant::now());
                }
            }
        }

        // 3. Advance cursor animation.
        overlay_grid.tick();

        // 4. Advance visibility state machine.
        match visibility {
            Visibility::Hidden | Visibility::Visible => {}
            Visibility::Lingering(since) => {
                if since.elapsed() >= linger_dur {
                    if fade_dur.is_zero() {
                        // Instant hide, no fade.
                        window.set_visible(false);
                        container.set_opacity(1.0);
                        visibility = Visibility::Hidden;
                    } else {
                        visibility = Visibility::Fading(Instant::now());
                    }
                }
            }
            Visibility::Fading(since) => {
                let t = (since.elapsed().as_secs_f64() / fade_dur.as_secs_f64()).min(1.0);
                container.set_opacity(1.0 - t);
                if t >= 1.0 {
                    window.set_visible(false);
                    container.set_opacity(1.0); // reset for next show
                    visibility = Visibility::Hidden;
                }
            }
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
