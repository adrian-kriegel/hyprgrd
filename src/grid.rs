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

use crate::command::Direction;

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
    /// from `(col, row)`.
    ///
    /// Pure: no mutation. Left/up at edge stay in place; right/down extend
    /// by one column/row.
    pub fn get_abs_from(
        direction: Direction,
        col: usize,
        row: usize,
    ) -> (usize, usize) {
        let (mut c, mut r) = (col, row);
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
        (c, r)
    }

    /// Grow the grid to contain `(col, row)` if needed.
    pub fn grow_to_contain(&mut self, col: usize, row: usize) {
        if col >= self.cols {
            self.cols = col + 1;
        }
        if row >= self.rows {
            self.rows = row + 1;
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
        let target = Grid::get_abs_from(Direction::Right, 0, 0);
        assert_eq!(target, (1, 0));
    }

    #[test]
    fn get_abs_from_down() {
        let target = Grid::get_abs_from(Direction::Down, 0, 0);
        assert_eq!(target, (0, 1));
    }

    #[test]
    fn get_abs_from_left_at_origin_stays() {
        let target = Grid::get_abs_from(Direction::Left, 0, 0);
        assert_eq!(target, (0, 0));
    }

    #[test]
    fn get_abs_from_up_at_origin_stays() {
        let target = Grid::get_abs_from(Direction::Up, 0, 0);
        assert_eq!(target, (0, 0));
    }

    #[test]
    fn grow_to_contain_expands_dimensions() {
        let mut g = Grid::new();
        g.grow_to_contain(3, 2);
        assert_eq!(g.dimensions(), (4, 3));
    }

    #[test]
    fn grow_to_contain_idempotent_for_existing_cell() {
        let mut g = Grid::new();
        g.grow_to_contain(0, 0);
        assert_eq!(g.dimensions(), (1, 1));
    }
}

