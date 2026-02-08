//! IPC listener that accepts commands over a Unix socket.
//!
//! External tools (scripts, key-bind helpers, etc.) can connect to the
//! socket and send newline-delimited JSON commands.

pub mod listener;



