
/// A cell in the workspace grid (0-indexed column and row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPosition {
    pub col: usize,
    pub row: usize,
}

impl GridPosition {
    pub const ORIGIN: Self = Self { col: 0, row: 0 };

    /// Construct from validated non-negative grid coordinates.
    pub fn from_coords(col: usize, row: usize) -> Self {
        Self { col, row }
    }
}
