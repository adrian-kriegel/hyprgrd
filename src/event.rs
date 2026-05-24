
use crate::command::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Command(Command),
    MonitorsChanged,
}