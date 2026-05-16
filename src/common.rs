
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

    fn parse_non_negative(s: &str, field: &str) -> Result<usize, String> {
        s.trim()
            .parse()
            .map_err(|_| format!("{field} must be a non-negative integer"))
    }

    /// Parse non-negative column and row values from wire text.
    pub fn from_col_row_str(col: &str, row: &str) -> Result<Self, String> {
        Ok(Self::from_coords(
            Self::parse_non_negative(col, "col")?,
            Self::parse_non_negative(row, "row")?,
        ))
    }

    /// Parse `"col row"` from the wire format.
    pub fn from_col_row_string(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(format!("SwitchTo: expected \"col row\", got {s:?}"));
        }
        Self::from_col_row_str(parts[0], parts[1])
    }
}