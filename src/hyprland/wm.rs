//! [`WindowManager`] implementation backed by Hyprland IPC.
//!
//! Communicates directly with Hyprland through its Unix socket at
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`,
//! avoiding any shell command invocation or third-party crate for socket
//! discovery.

use crate::command::{MonitorInfo, WindowInfo};
use crate::traits::WindowManager;
use serde::Deserialize;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// Hyprland-backed window manager.
///
/// All communication happens over Hyprland's IPC socket
/// (`$XDG_RUNTIME_DIR/hypr/<instance>/.socket.sock`).  No child processes
/// are spawned.
pub struct HyprlandWm;

/// Errors that can occur when talking to Hyprland.
#[derive(Debug, thiserror::Error)]
#[error("hyprland IPC error: {0}")]
pub struct HyprlandWmError(String);

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

//  Direct Hyprland IPC helpers 

/// Resolve the Hyprland command socket path.
///
/// Hyprland ≥ 0.40 stores its sockets at
/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`.
fn socket_path() -> Result<PathBuf, HyprlandWmError> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map_err(|_| HyprlandWmError("XDG_RUNTIME_DIR not set".into()))?;
    let his = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| HyprlandWmError("HYPRLAND_INSTANCE_SIGNATURE not set".into()))?;
    Ok(PathBuf::from(format!(
        "{}/hypr/{}/.socket.sock",
        runtime_dir, his
    )))
}

/// Send a raw command to the Hyprland command socket and return the
/// response as a string.
fn ipc_request(command: &str) -> Result<String, HyprlandWmError> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| HyprlandWmError(format!("connect to {}: {}", path.display(), e)))?;

    stream
        .write_all(command.as_bytes())
        .map_err(|e| HyprlandWmError(format!("write: {}", e)))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| HyprlandWmError(format!("read: {}", e)))?;

    String::from_utf8(response).map_err(|e| HyprlandWmError(format!("utf-8: {}", e)))
}

/// Send a JSON data query (`j/<command>`) and return the raw JSON string.
fn ipc_json(data_command: &str) -> Result<String, HyprlandWmError> {
    ipc_request(&format!("j/{}", data_command))
}

/// Send a dispatch command and check for `"ok"`.
fn ipc_dispatch(args: &str) -> Result<(), HyprlandWmError> {
    let response = ipc_request(&format!("/dispatch {}", args))?;
    if response.trim() == "ok" {
        Ok(())
    } else {
        Err(HyprlandWmError(format!("dispatch error: {}", response)))
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
}

/// Subset of the JSON object returned by `j/activewindow`.
#[derive(Deserialize)]
struct ActiveWindowJson {
    address: String,
    title: String,
    monitor: i64,
}

/// Resolve a Hyprland monitor numeric id to its name by querying `j/monitors`.
fn monitor_name_by_id(id: i64) -> Result<String, HyprlandWmError> {
    let json = ipc_json("monitors")?;
    let monitors: Vec<MonitorJson> =
        serde_json::from_str(&json).map_err(|e| HyprlandWmError(format!("parse: {}", e)))?;
    monitors
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.clone())
        .ok_or_else(|| HyprlandWmError(format!("unknown monitor id: {}", id)))
}

//  WindowManager implementation 

impl WindowManager for HyprlandWm {
    type Error = HyprlandWmError;

    fn monitors(&self) -> Result<Vec<MonitorInfo>, Self::Error> {
        let json = ipc_json("monitors")?;
        let monitors: Vec<MonitorJson> =
            serde_json::from_str(&json).map_err(|e| HyprlandWmError(format!("parse: {}", e)))?;
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

    fn switch_workspace(&self, monitor: &str, workspace_id: i32) -> Result<(), Self::Error> {
        // Hyprland dispatches are global — we focus the target monitor first,
        // then switch.
        ipc_dispatch(&format!("focusmonitor {}", monitor))?;
        ipc_dispatch(&format!("workspace {}", workspace_id))?;
        Ok(())
    }

    fn move_window_to_workspace(&self, workspace_id: i32) -> Result<(), Self::Error> {
        ipc_dispatch(&format!("movetoworkspace {}", workspace_id))
    }

    fn move_window_to_monitor(&self, monitor: &str) -> Result<(), Self::Error> {
        ipc_dispatch(&format!("movewindow mon:{}", monitor))
    }

    fn active_window(&self) -> Result<Option<WindowInfo>, Self::Error> {
        let json = ipc_json("activewindow")?;
        // Hyprland returns an empty object `{}` when no window is focused.
        if json.trim() == "{}" {
            return Ok(None);
        }
        let w: ActiveWindowJson =
            serde_json::from_str(&json).map_err(|e| HyprlandWmError(format!("parse: {}", e)))?;
        let monitor_name = monitor_name_by_id(w.monitor)?;
        Ok(Some(WindowInfo {
            address: w.address,
            title: w.title,
            monitor: monitor_name,
        }))
    }
}
