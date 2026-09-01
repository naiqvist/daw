//! The calibration grid: focus with nothing to focus ON.
//!
//! Before the stage displays any of the song, it displays FOCUS ITSELF —
//! a grid of meaningless squares and a cursor moved by the keyboard. The
//! squares carry no model data on purpose: the only thing on trial is the
//! sync marker, whether the eye can find it instantly, track it under key
//! repeat, and never lose it.
//!
//! Two rules the cursor commits to, both load-bearing for everything that
//! comes after:
//!
//! - **Clamp, never wrap.** A wrap is a spontaneous teleport of the
//!   conditioning context. At an edge the keystroke is absorbed, and the
//!   absorption is REPORTED — a refused move is information too.
//! - **The signal is a state change, never a motion.** The focused square
//!   inverts; nothing resizes, nothing reflows, the ground never shakes.

/// One directional step in the stage grammar. A scope decides which of
/// these directions connect two of its positions; an unsupported direction
/// is an honest refusal, never a disguised no-op.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Up,
    Down,
    Left,
    Right,
}

/// A one-dimensional vertical scope with no content attached. Its positions
/// are ordinal addresses only; a future track list can supply meaning without
/// changing the focus rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusColumn {
    len: usize,
    cursor: Option<usize>,
}

impl FocusColumn {
    pub fn new(len: usize) -> Self {
        Self {
            len,
            cursor: (len > 0).then_some(0),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Up and down traverse the column. Left and right are not aliases for
    /// staying put: this shape has no sideways neighbour, so they refuse.
    pub fn step(&mut self, step: Step) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        let next = match step {
            Step::Up => cursor.saturating_sub(1),
            Step::Down => cursor.saturating_add(1).min(self.len - 1),
            Step::Left | Step::Right => cursor,
        };
        self.cursor = Some(next);
        next != cursor
    }
}

/// A fixed field of squares and the one square the keyboard addresses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusGrid {
    cols: usize,
    rows: usize,
    col: usize,
    row: usize,
}

impl FocusGrid {
    /// A grid is never empty: focus must always have somewhere to stand.
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
            col: 0,
            row: 0,
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The addressed square as `(col, row)`, origin top-left.
    pub fn cursor(&self) -> (usize, usize) {
        (self.col, self.row)
    }

    /// Apply one step. `true` means focus moved; `false` means the edge
    /// absorbed it — the caller should SHOW that refusal, not swallow it.
    pub fn step(&mut self, step: Step) -> bool {
        let (col, row) = (self.col, self.row);
        match step {
            Step::Up => self.row = self.row.saturating_sub(1),
            Step::Down => self.row = (self.row + 1).min(self.rows - 1),
            Step::Left => self.col = self.col.saturating_sub(1),
            Step::Right => self.col = (self.col + 1).min(self.cols - 1),
        }
        (self.col, self.row) != (col, row)
    }
}

/// The content-free shapes a focus level may currently have.
///
/// This enum is the abstraction boundary: the stack knows only these common
/// operations, while drawing remains with the caller that knows which shape
/// it put in. Adding a new shape later means adding a focus variant, not
/// teaching the stack about tracks, time, cards, or scrolling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FocusScope {
    Grid(FocusGrid),
    Column(FocusColumn),
}

impl FocusScope {
    pub fn grid(cols: usize, rows: usize) -> Self {
        Self::Grid(FocusGrid::new(cols, rows))
    }

    pub fn column(len: usize) -> Self {
        Self::Column(FocusColumn::new(len))
    }

    /// Number of addressable positions. Zero is a first-class shape.
    pub fn len(&self) -> usize {
        match self {
            Self::Grid(grid) => grid.cols().saturating_mul(grid.rows()),
            Self::Column(column) => column.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The current address as an ordinal in this scope's own position set.
    /// Shape-specific geometry remains available on the implementation and
    /// deliberately does not leak into this common contract.
    pub fn cursor(&self) -> Option<usize> {
        match self {
            Self::Grid(grid) => {
                let (col, row) = grid.cursor();
                Some(row.saturating_mul(grid.cols()).saturating_add(col))
            }
            Self::Column(column) => column.cursor(),
        }
    }

    pub fn step(&mut self, step: Step) -> bool {
        match self {
            Self::Grid(grid) => grid.step(step),
            Self::Column(column) => column.step(step),
        }
    }
}

impl From<FocusGrid> for FocusScope {
    fn from(grid: FocusGrid) -> Self {
        Self::Grid(grid)
    }
}

impl From<FocusColumn> for FocusScope {
    fn from(column: FocusColumn) -> Self {
        Self::Column(column)
    }
}

/// A path of scopes. The top is the active conditioning context; ancestors
/// remain in place with their cursors intact, so ascent lands exactly where
/// descent left regardless of either scope's shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusStack {
    levels: Vec<FocusScope>,
    max_depth: usize,
}

impl FocusStack {
    pub fn new(root: impl Into<FocusScope>, max_depth: usize) -> Self {
        Self {
            levels: vec![root.into()],
            max_depth: max_depth.max(1),
        }
    }

    /// The scope the keyboard currently addresses.
    pub fn active(&self) -> &FocusScope {
        self.levels.last().expect("a stack is never empty")
    }

    /// Root first, active last.
    pub fn levels(&self) -> &[FocusScope] {
        &self.levels
    }

    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// Step within the active level.
    pub fn step(&mut self, step: Step) -> bool {
        self.levels
            .last_mut()
            .expect("a stack is never empty")
            .step(step)
    }

    /// Descend into the child supplied by the caller. The stack owns
    /// hierarchy, not the policy that decides what shape lives below the
    /// current position.
    pub fn enter(&mut self, child: impl Into<FocusScope>) -> bool {
        if self.levels.len() >= self.max_depth {
            return false;
        }
        self.levels.push(child.into());
        true
    }

    /// Ascend to the parent, landing on the position that was entered.
    /// Refused at the root: there is no further out.
    pub fn escape(&mut self) -> bool {
        if self.levels.len() == 1 {
            return false;
        }
        self.levels.pop();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_starts_at_the_origin() {
        let grid = FocusGrid::new(8, 8);
        assert_eq!(grid.cursor(), (0, 0));
    }

    #[test]
    fn steps_move_one_square_at_a_time() {
        let mut grid = FocusGrid::new(8, 8);
        assert!(grid.step(Step::Right));
        assert!(grid.step(Step::Down));
        assert_eq!(grid.cursor(), (1, 1));
        assert!(grid.step(Step::Left));
        assert!(grid.step(Step::Up));
        assert_eq!(grid.cursor(), (0, 0));
    }

    #[test]
    fn every_edge_clamps_and_reports_the_refusal() {
        let mut grid = FocusGrid::new(3, 2);
        assert!(!grid.step(Step::Up));
        assert!(!grid.step(Step::Left));
        assert_eq!(grid.cursor(), (0, 0));

        assert!(grid.step(Step::Right));
        assert!(grid.step(Step::Right));
        assert!(!grid.step(Step::Right));
        assert!(grid.step(Step::Down));
        assert!(!grid.step(Step::Down));
        assert_eq!(grid.cursor(), (2, 1));
    }

    #[test]
    fn a_degenerate_grid_still_holds_focus() {
        let mut grid = FocusGrid::new(0, 0);
        assert_eq!(grid.cursor(), (0, 0));
        assert!(!grid.step(Step::Up));
        assert!(!grid.step(Step::Down));
        assert!(!grid.step(Step::Left));
        assert!(!grid.step(Step::Right));
    }

    #[test]
    fn a_column_moves_vertically_and_refuses_sideways_honestly() {
        let mut scope = FocusScope::column(3);
        assert_eq!(scope.cursor(), Some(0));
        assert!(!scope.step(Step::Up));
        assert!(!scope.step(Step::Left));
        assert!(!scope.step(Step::Right));
        assert!(scope.step(Step::Down));
        assert_eq!(scope.cursor(), Some(1));
        assert!(scope.step(Step::Down));
        assert_eq!(scope.cursor(), Some(2));
        assert!(!scope.step(Step::Down));
    }

    #[test]
    fn an_empty_scope_has_no_cursor_and_refuses_every_direction() {
        let mut scope = FocusScope::column(0);
        assert!(scope.is_empty());
        assert_eq!(scope.cursor(), None);
        for step in [Step::Up, Step::Down, Step::Left, Step::Right] {
            assert!(!scope.step(step));
        }
    }

    #[test]
    fn different_scope_shapes_stack_without_moving_each_other() {
        let mut stack = FocusStack::new(FocusScope::column(3), 3);
        assert!(stack.step(Step::Down));
        assert_eq!(stack.active().cursor(), Some(1));

        assert!(stack.enter(FocusScope::grid(2, 2)));
        assert!(stack.step(Step::Right));
        assert!(stack.step(Step::Down));
        assert_eq!(stack.active().cursor(), Some(3));
        assert_eq!(stack.levels()[0].cursor(), Some(1));

        assert!(stack.escape());
        assert_eq!(stack.active().cursor(), Some(1));
    }
}
