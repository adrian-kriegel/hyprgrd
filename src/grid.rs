//! Grid workspace layout.
//!
//! The [`Grid`] struct manages a dynamic `cols × rows` grid of workspaces.
//! Rows and columns are created on demand: moving right from the rightmost
//! column adds a new column, moving down from the bottom row adds a new row.
//!
//! Each grid cell stores a per-monitor workspace id via
//! [`WorkspaceMapping`], because one *virtual* workspace in hyprgrd can span
//! multiple physical monitors (each monitor gets its own Hyprland workspace).

use crate::command::Direction;
use std::collections::HashMap;

/// Maps a grid cell `(col, row)` to per-monitor workspace ids.
///
/// For example, if two monitors are present, position `(0, 0)` might map to
/// workspace 1 on monitor `"DP-1"` and workspace 2 on monitor `"HDMI-A-1"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMapping {
    /// `monitor_name -> hyprland_workspace_id`
    inner: HashMap<String, i32>,
}

impl WorkspaceMapping {
    /// Create a new mapping from an iterator of `(monitor_name, workspace_id)`.
    pub fn new(pairs: impl IntoIterator<Item = (String, i32)>) -> Self {
        Self {
            inner: pairs.into_iter().collect(),
        }
    }

    /// Look up the workspace id assigned to `monitor`.
    pub fn get(&self, monitor: &str) -> Option<i32> {
        self.inner.get(monitor).copied()
    }

    /// Return an iterator over `(monitor_name, workspace_id)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, i32)> {
        self.inner.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// Number of monitors in this mapping.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the mapping is empty (no monitors).
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// A dynamic grid of workspaces.
///
/// The grid starts at 1×1 and grows as the user navigates beyond its current
/// bounds.  Each cell holds an optional [`WorkspaceMapping`]; mappings are
/// created lazily when a cell is first visited.
///
/// The grid also tracks the *current* position `(col, row)` of the user.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Number of columns (width).
    cols: usize,
    /// Number of rows (height).
    rows: usize,
    /// Current column (0-indexed).
    col: usize,
    /// Current row (0-indexed).
    row: usize,
    /// `(col, row) -> WorkspaceMapping`.  Not every cell has a mapping yet.
    mappings: HashMap<(usize, usize), WorkspaceMapping>,
    /// Names of monitors that participate in synchronized switching.
    monitors: Vec<String>,
    /// Counter used to allocate the next workspace id.
    next_workspace_id: i32,
}

impl Grid {
    /// Create a new 1×1 grid positioned at `(0, 0)`.
    ///
    /// `monitors` lists the monitor names that should be switched in unison.
    /// The initial cell `(0, 0)` is immediately allocated workspace ids for
    /// every monitor.
    pub fn new(monitors: Vec<String>) -> Self {
        let mut grid = Self {
            cols: 1,
            rows: 1,
            col: 0,
            row: 0,
            mappings: HashMap::new(),
            monitors: monitors.clone(),
            next_workspace_id: 1,
        };
        grid.ensure_mapping(0, 0);
        grid
    }

    //  Accessors 

    /// Current grid position as `(col, row)`.
    pub fn position(&self) -> (usize, usize) {
        (self.col, self.row)
    }

    /// Grid dimensions as `(cols, rows)`.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Get the [`WorkspaceMapping`] for the current position.
    pub fn current_mapping(&self) -> Option<&WorkspaceMapping> {
        self.mappings.get(&(self.col, self.row))
    }

    /// Get the [`WorkspaceMapping`] for an arbitrary position, if it exists.
    pub fn mapping_at(&self, col: usize, row: usize) -> Option<&WorkspaceMapping> {
        self.mappings.get(&(col, row))
    }

    /// Whether a mapping exists at the given position (i.e. the cell has
    /// been visited).
    pub fn has_mapping(&self, col: usize, row: usize) -> bool {
        self.mappings.contains_key(&(col, row))
    }

    /// Return every `(col, row)` pair that has a workspace mapping.
    pub fn visited_cells(&self) -> Vec<(usize, usize)> {
        self.mappings.keys().copied().collect()
    }

    //  Navigation 

    /// Move one step in `direction`, growing the grid if necessary.
    ///
    /// Returns the new `(col, row)` position and a reference to the
    /// [`WorkspaceMapping`] at that position.
    pub fn go(&mut self, direction: Direction) -> (usize, usize) {
        match direction {
            Direction::Left => {
                if self.col > 0 {
                    self.col -= 1;
                }
            }
            Direction::Right => {
                self.col += 1;
                if self.col >= self.cols {
                    self.cols = self.col + 1;
                }
            }
            Direction::Up => {
                if self.row > 0 {
                    self.row -= 1;
                }
            }
            Direction::Down => {
                self.row += 1;
                if self.row >= self.rows {
                    self.rows = self.row + 1;
                }
            }
        }
        self.ensure_mapping(self.col, self.row);
        (self.col, self.row)
    }

    /// Jump to an absolute position, growing the grid if needed.
    ///
    /// Returns the clamped position (col and row are lower-bounded at 0 but
    /// have no upper bound — the grid simply grows).
    pub fn switch_to(&mut self, col: usize, row: usize) -> (usize, usize) {
        if col >= self.cols {
            self.cols = col + 1;
        }
        if row >= self.rows {
            self.rows = row + 1;
        }
        self.col = col;
        self.row = row;
        self.ensure_mapping(col, row);
        (self.col, self.row)
    }

    //  Internal 

    /// Ensure a [`WorkspaceMapping`] exists for `(col, row)`.
    ///
    /// Allocates sequential workspace ids for each monitor if the cell has
    /// not been visited before.
    fn ensure_mapping(&mut self, col: usize, row: usize) {
        if self.mappings.contains_key(&(col, row)) {
            return;
        }
        let pairs: Vec<(String, i32)> = self
            .monitors
            .iter()
            .map(|name| {
                let id = self.next_workspace_id;
                self.next_workspace_id += 1;
                (name.clone(), id)
            })
            .collect();
        self.mappings.insert((col, row), WorkspaceMapping::new(pairs));
    }
}

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;

    fn monitors() -> Vec<String> {
        vec!["DP-1".into(), "HDMI-A-1".into()]
    }

    #[test]
    fn new_grid_starts_1x1_at_origin() {
        let g = Grid::new(monitors());
        assert_eq!(g.position(), (0, 0));
        assert_eq!(g.dimensions(), (1, 1));
    }

    #[test]
    fn initial_mapping_covers_all_monitors() {
        let g = Grid::new(monitors());
        let m = g.current_mapping().expect("should have mapping at (0,0)");
        assert_eq!(m.len(), 2);
        assert!(m.get("DP-1").is_some());
        assert!(m.get("HDMI-A-1").is_some());
    }

    #[test]
    fn go_right_grows_columns() {
        let mut g = Grid::new(monitors());
        let pos = g.go(Direction::Right);
        assert_eq!(pos, (1, 0));
        assert_eq!(g.dimensions(), (2, 1));
    }

    #[test]
    fn go_down_grows_rows() {
        let mut g = Grid::new(monitors());
        let pos = g.go(Direction::Down);
        assert_eq!(pos, (0, 1));
        assert_eq!(g.dimensions(), (1, 2));
    }

    #[test]
    fn go_left_at_origin_stays() {
        let mut g = Grid::new(monitors());
        let pos = g.go(Direction::Left);
        assert_eq!(pos, (0, 0));
    }

    #[test]
    fn go_up_at_origin_stays() {
        let mut g = Grid::new(monitors());
        let pos = g.go(Direction::Up);
        assert_eq!(pos, (0, 0));
    }

    #[test]
    fn switch_to_beyond_bounds_grows_grid() {
        let mut g = Grid::new(monitors());
        let pos = g.switch_to(3, 2);
        assert_eq!(pos, (3, 2));
        assert_eq!(g.dimensions(), (4, 3));
    }

    #[test]
    fn workspace_ids_are_unique_across_cells() {
        let mut g = Grid::new(monitors());
        g.go(Direction::Right);
        g.go(Direction::Down);

        // Collect all workspace ids
        let mut ids: Vec<i32> = Vec::new();
        for col in 0..g.dimensions().0 {
            for row in 0..g.dimensions().1 {
                if let Some(m) = g.mapping_at(col, row) {
                    for (_, id) in m.iter() {
                        ids.push(id);
                    }
                }
            }
        }
        let unique: std::collections::HashSet<i32> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "all workspace ids must be unique");
    }

    #[test]
    fn revisiting_cell_keeps_same_mapping() {
        let mut g = Grid::new(monitors());
        g.go(Direction::Right);
        let first = g.current_mapping().cloned();
        g.go(Direction::Left);
        g.go(Direction::Right);
        let second = g.current_mapping().cloned();
        assert_eq!(first, second, "mapping should be stable");
    }

    #[test]
    fn single_monitor_grid() {
        let mut g = Grid::new(vec!["eDP-1".into()]);
        g.go(Direction::Right);
        let m = g.current_mapping().unwrap();
        assert_eq!(m.len(), 1);
        assert!(m.get("eDP-1").is_some());
    }

    #[test]
    fn complex_navigation_sequence() {
        let mut g = Grid::new(monitors());
        // Build a 3x3 grid
        g.go(Direction::Right); // (1,0)
        g.go(Direction::Right); // (2,0)
        g.go(Direction::Down);  // (2,1)
        g.go(Direction::Down);  // (2,2)
        g.go(Direction::Left);  // (1,2)
        g.go(Direction::Left);  // (0,2)
        g.go(Direction::Up);    // (0,1)
        assert_eq!(g.position(), (0, 1));
        assert_eq!(g.dimensions(), (3, 3));
    }
}

