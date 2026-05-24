
use std::{io::{Read, Write}, os::unix::net::UnixStream, path::PathBuf};


#[derive(Debug, thiserror::Error)]
#[error("hyprland IPC error: {0}")]
pub struct IPCError(pub String);

fn hypr_socket_path(socket_name: &str) -> Result<PathBuf, IPCError> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map_err(|_| IPCError("XDG_RUNTIME_DIR not set".into()))?;
    let his = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| IPCError("HYPRLAND_INSTANCE_SIGNATURE not set".into()))?;

    Ok(PathBuf::from(runtime_dir)
        .join("hypr")
        .join(his)
        .join(socket_name))
}

pub fn socket_path() -> Result<PathBuf, IPCError> {
    hypr_socket_path(".socket.sock")
}

pub fn event_socket_path() -> Result<PathBuf, IPCError> {
    hypr_socket_path(".socket2.sock")
}

/// Send a raw command to the Hyprland command socket and return the
/// response as a string.
pub fn ipc_request(command: &str) -> Result<String, IPCError> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| IPCError(format!("connect to {}: {}", path.display(), e)))?;

    stream
        .write_all(command.as_bytes())
        .map_err(|e| IPCError(format!("write: {}", e)))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| IPCError(format!("read: {}", e)))?;

    String::from_utf8(response).map_err(|e| IPCError(format!("utf-8: {}", e)))
}

/// Send a JSON data query (`j/<command>`) and return the raw JSON string.
pub fn ipc_json(data_command: &str) -> Result<String, IPCError> {
    ipc_request(&format!("j/{}", data_command))
}

/// Send a dispatch command and check for `"ok"`.
pub fn ipc_dispatch(args: &str) -> Result<(), IPCError> {
    let response = ipc_request(&format!("/dispatch {}", args))?;
    if response.trim() == "ok" {
        Ok(())
    } else {
        Err(IPCError(format!("dispatch error: {}", response)))
    }
}
