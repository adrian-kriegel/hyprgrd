//! **hyprgrd** — a grid-based workspace switcher.
//!
//! Workspaces are arranged in a dynamic `cols × rows` grid.  Rows and columns
//! are created on demand as you navigate beyond the current bounds.  One
//! *virtual* workspace in hyprgrd spans all monitors: each monitor receives
//! its own window-manager workspace id, but they are switched in unison.
//!
//! # Architecture
//!
//! The crate is organised around two core traits:
//!
//! * [`traits::WindowManager`] — abstracts workspace switching and window
//!   movement so the grid logic is not coupled to any specific compositor.
//! * [`traits::CommandSource`] — abstracts the transport that delivers
//!   user-intent (a Unix socket, a gesture recogniser, …) so the main loop
//!   is not coupled to any specific IPC mechanism.
//!
//! Concrete implementations live in [`hyprland`] (Hyprland IPC) and
//! [`ipc`] (Unix-socket command listener).

pub mod command;
pub mod config;
pub mod grid;
pub mod hyprland;
pub mod ipc;
pub mod switcher;
pub mod traits;
pub mod visualizer;

