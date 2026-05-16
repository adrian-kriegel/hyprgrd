//! Grid workspace layout.
//!
//! The [`Grid`] struct manages a dynamic `cols × rows` grid of workspaces.
//! Rows and columns are created on demand when navigating beyond current
//! bounds.  The grid is **stateless** with respect to position: it only tracks
//! dimensions.  The current `(col, row)` is stored by the [`GridSwitcher`]
//! in per-monitor position entries.
//!
//! Mapping a cell to concrete workspace ids for individual monitors is handled
//! by higher-level orchestration code (see `switcher.rs`).

use crate::{command::Direction, common::GridPosition};

/// A dynamic grid of workspaces.
///
/// The grid starts at 1×1 and grows as navigation moves beyond its bounds.
/// It tracks only dimensions `(cols, rows)`; position state lives in the
/// switcher.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Number of columns (width).
    cols: usize,
    /// Number of rows (height).
    rows: usize,
}

impl Grid {
    /// Create a new 1×1 grid.
    pub fn new() -> Self {
        Self { cols: 1, rows: 1 }
    }

    /// Grid dimensions as `(cols, rows)`.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Compute the absolute target position when moving one step in `direction`
    /// from `pos`.
    ///
    /// Pure: no mutation. Left/up at edge stay in place; right/down extend
    /// by one column/row.
    pub fn get_abs_from(direction: Direction, pos: GridPosition) -> GridPosition {
        let (mut c, mut r) = (pos.col, pos.row);
        match direction {
            Direction::Left => {
                if c > 0 {
                    c -= 1;
                }
            }
            Direction::Right => c += 1,
            Direction::Up => {
                if r > 0 {
                    r -= 1;
                }
            }
            Direction::Down => r += 1,
            Direction::UpLeft => {
                if c > 0 {
                    c -= 1;
                }
                if r > 0 {
                    r -= 1;
                }
            }
            Direction::UpRight => {
                c += 1;
                if r > 0 {
                    r -= 1;
                }
            }
            Direction::DownLeft => {
                if c > 0 {
                    c -= 1;
                }
                r += 1;
            }
            Direction::DownRight => {
                c += 1;
                r += 1;
            }
        }
        GridPosition { col: c, row: r }
    }

    /// Grow the grid to contain `pos` if needed.
    pub fn grow_to_contain(&mut self, pos: GridPosition) {
        if pos.col >= self.cols {
            self.cols = pos.col + 1;
        }
        if pos.row >= self.rows {
            self.rows = pos.row + 1;
        }
    }
}

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grid_starts_1x1() {
        let g = Grid::new();
        assert_eq!(g.dimensions(), (1, 1));
    }

    #[test]
    fn get_abs_from_right() {
        let target = Grid::get_abs_from(Direction::Right, GridPosition::ORIGIN);
        assert_eq!(target, GridPosition { col: 1, row: 0 });
    }

    #[test]
    fn get_abs_from_down() {
        let target = Grid::get_abs_from(Direction::Down, GridPosition::ORIGIN);
        assert_eq!(target, GridPosition { col: 0, row: 1 });
    }

    #[test]
    fn get_abs_from_left_at_origin_stays() {
        let target = Grid::get_abs_from(Direction::Left, GridPosition::ORIGIN);
        assert_eq!(target, GridPosition::ORIGIN);
    }

    #[test]
    fn get_abs_from_up_at_origin_stays() {
        let target = Grid::get_abs_from(Direction::Up, GridPosition::ORIGIN);
        assert_eq!(target, GridPosition::ORIGIN);
    }

    #[test]
    fn grow_to_contain_expands_dimensions() {
        let mut g = Grid::new();
        g.grow_to_contain(GridPosition { col: 3, row: 2 });
        assert_eq!(g.dimensions(), (4, 3));
    }

    #[test]
    fn grow_to_contain_idempotent_for_existing_cell() {
        let mut g = Grid::new();
        g.grow_to_contain(GridPosition::ORIGIN);
        assert_eq!(g.dimensions(), (1, 1));
    }
}

