use log::debug;

use crate::event::Event;
use crate::hyprland::ipc::{IPCError, event_socket_path};
use crate::traits::EventSource;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

pub struct HyprlandEventListener;

impl HyprlandEventListener {
    pub fn new() -> Self {
        Self
    }
}

fn is_monitor_change_event(line: &str) -> bool {
    let Some((name, _data)) = line.trim_end().split_once(">>") else {
        return false;
    };

    matches!(
        name,
        "monitoradded" |
        "monitoraddedv2" | 
        "monitorremoved" |
        "monitorremovedv2"
    )
}

impl EventSource for HyprlandEventListener {
    type Error = IPCError;

    fn run(&mut self, sink: mpsc::Sender<Event>) -> Result<(), Self::Error> {
        let path = event_socket_path()?;
        let stream = UnixStream::connect(&path)
            .map_err(|e| IPCError(format!("connect to {}: {}", path.display(), e)))?;

        let reader = BufReader::new(stream);

        for line in reader.lines() {
            
            let line = line.map_err(
                |_| IPCError(String::from("Failed to read from hyprland socket.")))?;
            if is_monitor_change_event(&line) {
                if sink.send(Event::MonitorsChanged).is_err() {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}
