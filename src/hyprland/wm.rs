//! [`WindowManager`] implementation backed by Hyprland IPC.
//!
//! Communicates directly with Hyprland through its Unix socket at
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`,
//! avoiding any shell command invocation or third-party crate for socket
//! discovery.

use crate::command::{MonitorInfo, WindowInfo};
use crate::common::GridPosition;
use crate::traits::WindowManager;
use serde::Deserialize;

use crate::hyprland::ipc::{IPCError, ipc_dispatch, ipc_json};

/// Hyprland-backed window manager.
///
/// All communication happens over Hyprland's IPC socket
/// (`$XDG_RUNTIME_DIR/hypr/<instance>/.socket.sock`).  No child processes
/// are spawned.
pub struct HyprlandWm;

impl Default for HyprlandWm {
    fn default() -> Self {
        Self
    }
}


impl HyprlandWm {
    /// Create a new handle.
    ///
    /// No connection is opened eagerly; each method call opens a short-lived
    /// IPC request.
    pub fn new() -> Self {
        Self
    }
}

//  Minimal serde structs for the JSON we care about 

/// Subset of the JSON object returned by `j/monitors`.
#[derive(Deserialize)]
struct MonitorJson {
    id: i64,
    name: String,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    focused: bool,
}

/// Subset of the JSON object returned by `j/activewindow`.
#[derive(Deserialize)]
struct ActiveWindowJson {
    address: String,
    title: String,
    monitor: i64,
}

/// Resolve a Hyprland monitor numeric id to its name by querying `j/monitors`.
fn monitor_name_by_id(id: i64) -> Result<String, IPCError> {
    let json = ipc_json("monitors")?;
    let monitors: Vec<MonitorJson> =
        serde_json::from_str(&json).map_err(|e| IPCError(format!("parse: {}", e)))?;
    monitors
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.clone())
        .ok_or_else(|| IPCError(format!("unknown monitor id: {}", id)))
}

/// Generate a unique name for a workspace from the monitor name and grid position
fn workspace_name(monitor: &str, position: GridPosition) -> String {
    format!("{}-{}-{}", monitor, position.col, position.row)
}

//  WindowManager implementation 

impl WindowManager for HyprlandWm {
    type Error = IPCError;

    fn monitors(&self) -> Result<Vec<MonitorInfo>, Self::Error> {
        let json = ipc_json("monitors")?;
        let monitors: Vec<MonitorJson> =
            serde_json::from_str(&json).map_err(|e| IPCError(format!("parse: {}", e)))?;
        Ok(monitors
            .into_iter()
            .map(|m| MonitorInfo {
                name: m.name,
                width: m.width,
                height: m.height,
                x: m.x,
                y: m.y,
            })
            .collect())
    }

    fn switch_workspace(&self, monitor: &str, position: GridPosition) -> Result<(), Self::Error> {
        let workspace = workspace_name(monitor, position);

        // Hyprland dispatches are global. We need to focus the target monitor first,
        // then switch to the named workspace scoped to that monitor.
        ipc_dispatch(&format!("focusmonitor {}", monitor))?;

        // Named workspace format: <monitor>-<col>-<row>
        ipc_dispatch(&format!(
            "focusworkspaceoncurrentmonitor name:{}",
            workspace
        ))?;

        Ok(())
    }

    fn move_window_to_workspace(
        &self,
        monitor: &str,
        position: GridPosition,
    ) -> Result<(), Self::Error> {
        let workspace = workspace_name(monitor, position);

        ipc_dispatch(&format!("movetoworkspace name:{}", workspace))
    }

    fn move_window_to_monitor(&self, monitor: &str) -> Result<(), Self::Error> {
        ipc_dispatch(&format!("movewindow mon:{}", monitor))
    }

    fn active_monitor(&self) -> Result<Option<String>, Self::Error> {
        let json = ipc_json("monitors")?;
        let monitors: Vec<MonitorJson> = serde_json::from_str(&json)
            .map_err(|e| IPCError(format!("parse: {}", e)))?;
        Ok(monitors
            .into_iter()
            .find(|m| m.focused)
            .map(|m| m.name))
    }

    fn active_window(&self) -> Result<Option<WindowInfo>, Self::Error> {
        let json = ipc_json("activewindow")?;
        // Hyprland returns an empty object `{}` when no window is focused.
        if json.trim() == "{}" {
            return Ok(None);
        }
        let w: ActiveWindowJson =
            serde_json::from_str(&json).map_err(|e| IPCError(format!("parse: {}", e)))?;
        let monitor_name = monitor_name_by_id(w.monitor)?;
        Ok(Some(WindowInfo {
            address: w.address,
            title: w.title,
            monitor: monitor_name,
        }))
    }
}
