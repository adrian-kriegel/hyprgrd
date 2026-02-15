//! Entry point for the **hyprgrd** daemon.
//!
//! Spawns all configured [`CommandSource`](hyprgrd::traits::CommandSource)s
//! on background threads and processes incoming commands on the main thread.
//!
//! When the `visualizer-gtk` feature is enabled the main thread runs the
//! GLib main loop (GTK4 requires it) and polls the command channel from
//! there.  Without the feature, a simple blocking loop is used instead.

use hyprgrd::command::Command;
use hyprgrd::config::Config;
use hyprgrd::hyprland::wm::HyprlandWm;
use hyprgrd::ipc::listener::UnixSocketListener;
use hyprgrd::switcher::GridSwitcher;
use hyprgrd::traits::{CommandSource, WindowManager};
use log::{error, info};
use std::sync::mpsc;

/// Default socket path for the command listener.
fn default_socket_path() -> String {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    format!("{}/hyprgrd.sock", runtime)
}

/// Resolve the config directory (`$XDG_CONFIG_HOME/hyprgrd`).
fn config_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{}/.config", home)
    });
    std::path::PathBuf::from(base).join("hyprgrd")
}

/// Try to load the config from `$XDG_CONFIG_HOME/hyprgrd/config.json`,
/// falling back to compiled-in defaults.
fn load_config() -> Config {
    let path = config_dir().join("config.json");
    match Config::load(&path) {
        Ok(cfg) => {
            info!("loaded config from {}", path.display());
            cfg
        }
        Err(e) => {
            info!("no config file ({}), using defaults", e);
            Config::default()
        }
    }
}

/// Resolve the CSS stylesheet path.
#[cfg(feature = "visualizer-gtk")]
fn css_path() -> std::path::PathBuf {
    config_dir().join("style.css")
}

//  No-op window manager (--debug-visualizer-only) 

#[cfg(feature = "visualizer-gtk")]
mod noop_wm {
    use hyprgrd::command::{MonitorInfo, WindowInfo};
    use hyprgrd::traits::WindowManager;

    pub struct NoopWm;

    #[derive(Debug, thiserror::Error)]
    #[error("noop")]
    pub struct NoopWmError;

    impl WindowManager for NoopWm {
        type Error = NoopWmError;

        fn monitors(&self) -> Result<Vec<MonitorInfo>, NoopWmError> {
            Ok(vec![MonitorInfo {
                name: "DEBUG-1".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
            }])
        }

        fn switch_workspace(&self, _: &str, _: i32) -> Result<(), NoopWmError> {
            Ok(())
        }

        fn move_window_to_workspace(&self, _: i32) -> Result<(), NoopWmError> {
            Ok(())
        }

        fn move_window_to_monitor(&self, _: &str) -> Result<(), NoopWmError> {
            Ok(())
        }

        fn active_monitor(&self) -> Result<Option<String>, NoopWmError> {
            Ok(Some("DEBUG-1".into()))
        }

        fn active_window(&self) -> Result<Option<WindowInfo>, NoopWmError> {
            Ok(None)
        }
    }
}

#[cfg(feature = "visualizer-gtk")]
use noop_wm::NoopWm;

//  Main 

fn main() {
    env_logger::init();

    let debug_visualizer = std::env::args().any(|a| a == "--debug-visualizer-only");

    if debug_visualizer {
        run_debug_visualizer();
    } else {
        run_daemon();
    }
}

/// Normal daemon mode.
fn run_daemon() {
    let config = load_config();

    let wm = HyprlandWm::new();
    let monitors = match wm.monitors() {
        Ok(m) => {
            info!("found {} monitor(s)", m.len());
            m.iter().map(|m| m.name.clone()).collect::<Vec<_>>()
        }
        Err(e) => {
            error!("failed to query monitors: {}", e);
            std::process::exit(1);
        }
    };

    let mut switcher = GridSwitcher::new(wm, monitors);
    switcher.set_gesture_config(config.gestures.clone());

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    #[cfg(feature = "visualizer-gtk")]
    let cmd_tx_for_visualizer = cmd_tx.clone();
    spawn_command_sources(cmd_tx);

    #[cfg(feature = "visualizer-gtk")]
    start_event_loop(switcher, cmd_rx, cmd_tx_for_visualizer, config);
    #[cfg(not(feature = "visualizer-gtk"))]
    start_event_loop(switcher, cmd_rx, config);
}

/// Debug-visualizer-only mode.
fn run_debug_visualizer() {
    #[cfg(not(feature = "visualizer-gtk"))]
    {
        error!("--debug-visualizer-only requires the `visualizer-gtk` feature");
        std::process::exit(1);
    }

    #[cfg(feature = "visualizer-gtk")]
    {
        let config = load_config();

        info!("running in debug-visualizer-only mode (no workspace switching)");

        let monitors = vec!["DEBUG-1".into()];
        let mut switcher = GridSwitcher::new(NoopWm, monitors);
        switcher.set_gesture_config(config.gestures.clone());

        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let cmd_tx_for_visualizer = cmd_tx.clone();
        spawn_command_sources(cmd_tx);

        start_event_loop(switcher, cmd_rx, cmd_tx_for_visualizer, config);
    }
}

//  Event loops 

#[cfg(feature = "visualizer-gtk")]
fn start_event_loop<W: WindowManager + 'static>(
    mut switcher: GridSwitcher<W>,
    cmd_rx: mpsc::Receiver<Command>,
    cmd_tx_for_visualizer: mpsc::Sender<Command>,
    config: Config,
) {
    let (vis_tx, vis_rx) = mpsc::channel();
    switcher.set_visualizer(vis_tx);

    let initial_state = switcher.visualizer_state(0.0, 0.0);

    use std::cell::RefCell;
    let switcher = std::rc::Rc::new(RefCell::new(switcher));
    let dispatch = Box::new(move |cmd: Command| {
        if let Err(e) = switcher.borrow_mut().handle(cmd) {
            error!(target: "hyprgrd::switcher", "command error: {}", e);
        }
    });

    hyprgrd::visualizer::gtk::run_main_loop(
        cmd_rx,
        vis_rx,
        cmd_tx_for_visualizer,
        dispatch,
        initial_state,
        Some(css_path()),
        config.visualizer,
    );
}

#[cfg(not(feature = "visualizer-gtk"))]
fn start_event_loop<W: WindowManager>(
    mut switcher: GridSwitcher<W>,
    cmd_rx: mpsc::Receiver<Command>,
    _config: Config,
) {
    info!("hyprgrd running");
    for cmd in cmd_rx {
        if let Err(e) = switcher.handle(cmd) {
            error!("command error: {}", e);
        }
    }
    info!("all command sources closed, exiting");
}

//  Helpers 

fn spawn_command_sources(tx: mpsc::Sender<Command>) {
    {
        let tx = tx.clone();
        let path = default_socket_path();
        std::thread::spawn(move || {
            let mut source = UnixSocketListener::new(&path);
            if let Err(e) = source.run(tx) {
                error!("socket listener error: {}", e);
            }
        });
    }

    // Swipe gestures arrive over the same Unix socket, sent by the
    // hyprgrd Hyprland plugin (SwipeBegin / SwipeUpdate / SwipeEnd).
    // No separate gesture thread is needed.

    drop(tx);
}
