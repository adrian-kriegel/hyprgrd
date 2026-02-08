//! Hyprland-specific implementations.
//!
//! This module provides concrete backends for the
//! [`WindowManager`](crate::traits::WindowManager) and
//! [`CommandSource`](crate::traits::CommandSource) traits, powered by
//! Hyprland's IPC sockets.
//!
//! Nothing outside this module should reference Hyprland directly.

pub mod gestures;
pub mod wm;



