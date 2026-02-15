//! Unix-socket [`CommandSource`] implementation.
//!
//! Binds a Unix stream socket and accepts one connection at a time.
//! Each line received is parsed as a JSON-encoded [`Command`].
//!
//! # Wire format
//!
//! Every message is a single line of JSON followed by `\n`:
//!
//! ```json
//! {"Go":"Right"}
//! {"SwitchTo":{"x":2,"y":1}}
//! {"PrepareMove":{"dx":0.5,"dy":-0.3}}
//! "CancelMove"
//! {"CommitMove":"Down"}
//! {"MoveWindowAndGo":"Left"}
//! ```

use crate::command::Command;
use crate::traits::CommandSource;
use log::{debug, error, info};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// A [`CommandSource`] that listens on a Unix stream socket for
/// JSON-encoded commands.
///
/// Each accepted connection can send multiple newline-delimited JSON
/// commands.  When the connection closes, the listener waits for the
/// next one.
pub struct UnixSocketListener {
    path: PathBuf,
}

/// Errors produced by the Unix socket listener.
#[derive(Debug, thiserror::Error)]
pub enum UnixSocketError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

impl UnixSocketListener {
    /// Create a new listener bound to `path`.
    ///
    /// The socket file is created when [`run`](CommandSource::run) is called
    /// and removed when the source shuts down.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// The filesystem path of the socket.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl CommandSource for UnixSocketListener {
    type Error = UnixSocketError;

    /// Bind the socket and start accepting connections.
    ///
    /// This method **blocks** indefinitely.  Run it on a dedicated thread.
    fn run(&mut self, sink: mpsc::Sender<Command>) -> Result<(), Self::Error> {
        // Remove stale socket if present.
        let _ = std::fs::remove_file(&self.path);

        let listener = UnixListener::bind(&self.path)?;
        info!("listening on {}", self.path.display());

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    debug!("client connected");
                    let reader = BufReader::new(stream);
                    for line in reader.lines() {
                        match line {
                            Ok(ref text) if text.trim().is_empty() => continue,
                            Ok(text) => match serde_json::from_str::<Command>(&text) {
                                Ok(cmd) => {
                                    debug!("received {:?}", cmd);
                                    if sink.send(cmd).is_err() {
                                        info!("sink closed, shutting down");
                                        return Ok(());
                                    }
                                }
                                Err(e) => {
                                    error!("bad command: {} — {}", text, e);
                                }
                            },
                            Err(e) => {
                                error!("read error: {}", e);
                                break;
                            }
                        }
                    }
                    debug!("client disconnected");
                }
                Err(e) => {
                    error!("accept error: {}", e);
                }
            }
        }
        Ok(())
    }
}

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Direction;
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Monotonic counter to generate unique socket paths per test.
    static TEST_ID: AtomicU32 = AtomicU32::new(0);

    /// Helper: create a unique temporary socket path for each test.
    fn tmp_socket_path() -> PathBuf {
        let id = TEST_ID.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir();
        dir.join(format!(
            "hyprgrd-test-{}-{}.sock",
            std::process::id(),
            id
        ))
    }

    #[test]
    fn round_trip_commands_over_socket() {
        let path = tmp_socket_path();
        let path_clone = path.clone();

        let (tx, rx) = mpsc::channel();

        // Run listener in a background thread.
        let _handle = std::thread::spawn(move || {
            let mut listener = UnixSocketListener::new(&path_clone);
            let _ = listener.run(tx);
        });

        // Give the listener a moment to bind.
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Connect and send commands.
        {
            let mut stream = UnixStream::connect(&path).expect("connect");
            writeln!(stream, r#"{{"Go":"Right"}}"#).unwrap();
            writeln!(stream, r#"{{"SwitchTo":{{"x":2,"y":1}}}}"#).unwrap();
            writeln!(stream, r#""CancelMove""#).unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
        }

        // Collect commands (give the listener a moment to process).
        std::thread::sleep(std::time::Duration::from_millis(150));
        let cmds: Vec<Command> = rx.try_iter().collect();

        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0], Command::Go(Direction::Right));
        assert_eq!(cmds[1], Command::SwitchTo(crate::command::SwitchToTarget { x: 2, y: 1 }));
        assert_eq!(cmds[2], Command::CancelMove);

        // Clean up.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn malformed_json_does_not_crash() {
        let path = tmp_socket_path();
        let path2 = path.clone();
        let (tx, rx) = mpsc::channel();

        let _handle = std::thread::spawn(move || {
            let mut listener = UnixSocketListener::new(&path2);
            let _ = listener.run(tx);
        });

        std::thread::sleep(std::time::Duration::from_millis(150));

        {
            let mut stream = UnixStream::connect(&path).expect("connect");
            writeln!(stream, "not json at all").unwrap();
            writeln!(stream, r#"{{"Go":"Right"}}"#).unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(150));
        let cmds: Vec<Command> = rx.try_iter().collect();
        // Only the valid command should have arrived.
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], Command::Go(Direction::Right));

        let _ = std::fs::remove_file(&path);
    }
}

