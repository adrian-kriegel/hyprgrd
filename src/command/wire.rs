//! Wire representations of [`Command`] variants received from the plugin / IPC.
//!
//! Each type deserializes verbatim JSON from the transport and converts into the
//! validated domain type via [`TryFrom`].

use serde::Deserialize;
use std::convert::TryFrom;

use super::{Command, Direction, MonitorIndex, SwitchTo};

/// Error while converting a wire command into a domain [`Command`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CommandParseError(pub String);

/// Raw direction string from the plugin (e.g. `"right"`, `"  up-left  "`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct WireDirection(pub String);

/// Raw `"col row"` argument for [`Command::SwitchTo`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct WireSwitchTo(pub String);

/// Raw monitor index string for [`Command::MoveWindowToMonitorIndex`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct WireMonitorIndex(pub String);

/// Mirrors [`Command`] on the wire; fields that need validation use wire newtypes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum WireCommand {
    SwitchTo(WireSwitchTo),
    Go(WireDirection),
    MoveWindowAndGo(WireDirection),
    MoveWindowToMonitor(WireDirection),
    MoveWindowToMonitorIndex(WireMonitorIndex),
    PrepareMove { dx: f64, dy: f64 },
    CancelMove,
    CommitMove(WireDirection),
    ToggleVisualizer,
    SwipeBegin { fingers: u32 },
    SwipeUpdate { fingers: u32, dx: f64, dy: f64 },
    SwipeEnd,
}

fn parse_direction(s: &str) -> Option<Direction> {
    let normalized: String = s
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    match normalized.as_str() {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        "upleft" | "up-left" => Some(Direction::UpLeft),
        "upright" | "up-right" => Some(Direction::UpRight),
        "downleft" | "down-left" => Some(Direction::DownLeft),
        "downright" | "down-right" => Some(Direction::DownRight),
        _ => None,
    }
}

impl TryFrom<WireDirection> for Direction {
    type Error = CommandParseError;

    fn try_from(w: WireDirection) -> Result<Self, Self::Error> {
        parse_direction(&w.0).ok_or_else(|| {
            CommandParseError(format!("invalid direction: {:?}", w.0))
        })
    }
}

impl TryFrom<WireSwitchTo> for SwitchTo {
    type Error = CommandParseError;

    fn try_from(w: WireSwitchTo) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = w.0.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(CommandParseError(format!(
                "SwitchTo: expected \"col row\", got {:?}",
                w.0
            )));
        }
        let col: usize = parts[0]
            .parse()
            .map_err(|_| CommandParseError("SwitchTo: col must be a non-negative integer".into()))?;
        let row: usize = parts[1]
            .parse()
            .map_err(|_| CommandParseError("SwitchTo: row must be a non-negative integer".into()))?;
        Ok(SwitchTo::new(col, row))
    }
}

impl TryFrom<WireMonitorIndex> for MonitorIndex {
    type Error = CommandParseError;

    fn try_from(w: WireMonitorIndex) -> Result<Self, Self::Error> {
        let index: usize = w
            .0
            .trim()
            .parse()
            .map_err(|_| {
                CommandParseError(
                    "MoveWindowToMonitorIndex: expected non-negative integer".into(),
                )
            })?;
        Ok(MonitorIndex(index))
    }
}

impl TryFrom<WireCommand> for Command {
    type Error = CommandParseError;

    fn try_from(w: WireCommand) -> Result<Self, Self::Error> {
        Ok(match w {
            WireCommand::SwitchTo(arg) => Command::SwitchTo(arg.try_into()?),
            WireCommand::Go(dir) => Command::Go(dir.try_into()?),
            WireCommand::MoveWindowAndGo(dir) => Command::MoveWindowAndGo(dir.try_into()?),
            WireCommand::MoveWindowToMonitor(dir) => Command::MoveWindowToMonitor(dir.try_into()?),
            WireCommand::MoveWindowToMonitorIndex(idx) => {
                Command::MoveWindowToMonitorIndex(idx.try_into()?)
            }
            WireCommand::PrepareMove { dx, dy } => Command::PrepareMove { dx, dy },
            WireCommand::CancelMove => Command::CancelMove,
            WireCommand::CommitMove(dir) => Command::CommitMove(dir.try_into()?),
            WireCommand::ToggleVisualizer => Command::ToggleVisualizer,
            WireCommand::SwipeBegin { fingers } => Command::SwipeBegin { fingers },
            WireCommand::SwipeUpdate { fingers, dx, dy } => {
                Command::SwipeUpdate { fingers, dx, dy }
            }
            WireCommand::SwipeEnd => Command::SwipeEnd,
        })
    }
}

impl WireCommand {
    /// Deserialize a line of JSON and convert to a domain [`Command`].
    pub fn parse_line(line: &str) -> Result<Command, CommandParseError> {
        let wire: WireCommand = serde_json::from_str(line)
            .map_err(|e| CommandParseError(format!("json: {e}")))?;
        wire.try_into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::GridPosition;

    #[test]
    fn wire_direction_parses_case_insensitive() {
        let dir: Direction = serde_json::from_str::<WireDirection>(r#""RIGHT""#)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(dir, Direction::Right);
    }

    #[test]
    fn wire_direction_parses_diagonal() {
        let dir: Direction = serde_json::from_str::<WireDirection>(r#""up-left""#)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(dir, Direction::UpLeft);
    }

    #[test]
    fn wire_switch_to_parses_col_row() {
        let target: SwitchTo = serde_json::from_str::<WireSwitchTo>(r#""2 1""#)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(target, SwitchTo::new(2, 1));
        assert_eq!(target.grid_position(), GridPosition::from_coords(2, 1));
    }

    #[test]
    fn wire_switch_to_rejects_invalid() {
        let wire: WireSwitchTo = serde_json::from_str(r#""abc""#).unwrap();
        assert!(SwitchTo::try_from(wire).is_err());
    }

    #[test]
    fn wire_monitor_index_parses() {
        let idx: MonitorIndex = serde_json::from_str::<WireMonitorIndex>(r#""3""#)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(idx, MonitorIndex(3));
    }

    #[test]
    fn wire_command_round_trip() {
        let cmd = WireCommand::parse_line(r#"{"Go":"right"}"#).unwrap();
        assert_eq!(cmd, Command::Go(Direction::Right));

        let cmd = WireCommand::parse_line(r#"{"SwitchTo":"2 1"}"#).unwrap();
        assert_eq!(cmd, Command::SwitchTo(SwitchTo::new(2, 1)));
    }
}
