//! Mock visualizer demo — a 3×3 grid with a cursor that **slides**
//! between cells on a clockwise path.  Uses the same CSS-styleable
//! `.grid-cursor` widget and ease-out-cubic animation as the real
//! visualizer.
//!
//! Run with:
//!     cargo run --bin hyprgrd-test-overlay
//!
//! Press Ctrl-C to quit.

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::LayerShell;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

const COLS: usize = 3;
const ROWS: usize = 3;

const CELL_SIZE: i32 = 24;
const CELL_MARGIN: i32 = 3;
const CELL_PITCH: i32 = CELL_SIZE + 2 * CELL_MARGIN;

/// Milliseconds between each step.
const STEP_MS: u64 = 800;
/// Animation duration for the cursor slide.
const ANIM_MS: u64 = 200;

const CSS: &str = r#"
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

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

fn cell_px(index: usize) -> f64 {
    index as f64 * CELL_PITCH as f64 + CELL_MARGIN as f64
}

/// Clockwise walk around the grid edge.
fn walk_path() -> Vec<(usize, usize)> {
    let mut path = Vec::new();
    for c in 0..COLS {
        path.push((c, 0));
    }
    for r in 1..ROWS {
        path.push((COLS - 1, r));
    }
    for c in (0..COLS - 1).rev() {
        path.push((c, ROWS - 1));
    }
    for r in (1..ROWS - 1).rev() {
        path.push((0, r));
    }
    path
}

struct Anim {
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
    start: Instant,
}

fn main() {
    gtk4::init().expect("Failed to initialise GTK4");

    //  CSS 
    let provider = gtk4::CssProvider::new();
    #[allow(deprecated)]
    provider.load_from_data(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    //  Layer-shell window 
    let window = gtk4::Window::new();
    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);
    window.set_namespace("hyprgrd-test");
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
    window.set_decorated(false);
    window.remove_css_class("background");

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("grid-overlay");
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    window.set_child(Some(&container));

    //  Grid inside an Overlay 
    let overlay = gtk4::Overlay::new();

    let grid = gtk4::Grid::new();
    grid.add_css_class("grid");
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);

    let mut cells: Vec<gtk4::Box> = Vec::with_capacity(COLS * ROWS);
    for row in 0..ROWS {
        for col in 0..COLS {
            let cell = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            cell.add_css_class("grid-cell");
            cell.set_size_request(CELL_SIZE, CELL_SIZE);
            grid.attach(&cell, col as i32, row as i32, 1, 1);
            cells.push(cell);
        }
    }
    overlay.set_child(Some(&grid));

    //  Cursor 
    let cursor = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    cursor.add_css_class("grid-cursor");
    cursor.set_size_request(CELL_SIZE, CELL_SIZE);
    cursor.set_halign(gtk4::Align::Start);
    cursor.set_valign(gtk4::Align::Start);
    cursor.set_can_target(false);
    overlay.add_overlay(&cursor);
    overlay.set_measure_overlay(&cursor, false);

    container.append(&overlay);

    //  Animation state 
    let path = walk_path();
    let step = Rc::new(Cell::new(0usize));
    let visited: Rc<Cell<u64>> = Rc::new(Cell::new(1)); // cell 0 visited
    let anim: Rc<RefCell<Option<Anim>>> = Rc::new(RefCell::new(None));
    let cur_x: Rc<Cell<f64>> = Rc::new(Cell::new(cell_px(0)));
    let cur_y: Rc<Cell<f64>> = Rc::new(Cell::new(cell_px(0)));

    // Initial cursor position.
    cursor.set_margin_start(cell_px(0).round() as i32);
    cursor.set_margin_top(cell_px(0).round() as i32);
    window.present();

    //  Step timer: move to next cell every STEP_MS 
    {
        let path = path.clone();
        let step = step.clone();
        let visited = visited.clone();
        let anim = anim.clone();
        let cur_x = cur_x.clone();
        let cur_y = cur_y.clone();
        let cells = cells.clone();

        glib::timeout_add_local(Duration::from_millis(STEP_MS), move || {
            let i = (step.get() + 1) % path.len();
            step.set(i);

            let (col, row) = path[i];
            let mut vis = visited.get();
            vis |= 1 << (row * COLS + col);
            visited.set(vis);

            // Update CSS classes on cells.
            for r in 0..ROWS {
                for c in 0..COLS {
                    let cell = &cells[r * COLS + c];
                    let is_visited = vis & (1 << (r * COLS + c)) != 0;
                    if is_visited {
                        cell.add_css_class("visited");
                    } else {
                        cell.remove_css_class("visited");
                    }
                }
            }

            // Start cursor animation.
            *anim.borrow_mut() = Some(Anim {
                from_x: cur_x.get(),
                from_y: cur_y.get(),
                to_x: cell_px(col),
                to_y: cell_px(row),
                start: Instant::now(),
            });

            glib::ControlFlow::Continue
        });
    }

    //  Render tick: advance animation at ~60 fps 
    {
        let anim = anim.clone();
        let cursor = cursor.clone();

        glib::timeout_add_local(Duration::from_millis(16), move || {
            let mut anim_ref = anim.borrow_mut();
            if let Some(ref a) = *anim_ref {
                let t = (a.start.elapsed().as_secs_f64()
                    / Duration::from_millis(ANIM_MS).as_secs_f64())
                .min(1.0);
                let e = ease_out_cubic(t);

                let x = a.from_x + (a.to_x - a.from_x) * e;
                let y = a.from_y + (a.to_y - a.from_y) * e;

                cursor.set_margin_start(x.round() as i32);
                cursor.set_margin_top(y.round() as i32);
                cur_x.set(x);
                cur_y.set(y);

                if t >= 1.0 {
                    *anim_ref = None;
                }
            }
            glib::ControlFlow::Continue
        });
    }

    eprintln!("Mock visualizer running — cursor slides between cells.");
    eprintln!("Press Ctrl-C to quit.");

    let main_loop = glib::MainLoop::new(None, false);
    main_loop.run();
}
