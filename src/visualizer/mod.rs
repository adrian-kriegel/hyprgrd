//! Visualizer implementations for the workspace grid overlay.
//!
//! When the `visualizer-gtk` feature is enabled, the
//! [`gtk::run_main_loop`] function takes over the main thread and
//! drives both command processing and overlay rendering through the
//! GLib main loop.

#[cfg(feature = "visualizer-gtk")]
pub mod gtk;
