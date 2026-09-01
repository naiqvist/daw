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

/// A one-dimensional horizontal scope with no content attached. Its
/// positions are ordinal addresses only — the mirror of [`FocusColumn`],
/// for the axis a track strip runs along.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusRow {
    len: usize,
    cursor: Option<usize>,
}

impl FocusRow {
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

    /// Grow or shrink to `len`, keeping the cursor addressable: a row
    /// that gains its first position gains a cursor, and one whose cursor
    /// fell off the end is clamped to the last rather than losing its
    /// place. The song owns how many tracks exist; this follows.
    pub fn resize(&mut self, len: usize) {
        self.len = len;
        self.cursor = match (len, self.cursor) {
            (0, _) => None,
            (_, None) => Some(0),
            (_, Some(cursor)) => Some(cursor.min(len - 1)),
        };
    }

    /// Address `index` directly. `false` means the row has no such
    /// position and the cursor did not move — a caller asking for a place
    /// that is not there gets told, not silently relocated.
    pub fn focus(&mut self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }
        self.cursor = Some(index);
        true
    }

    /// Left and right traverse the row. Up and down are not aliases for
    /// staying put: this shape has no neighbour above or below, so they
    /// refuse.
    pub fn step(&mut self, step: Step) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        let next = match step {
            Step::Left => cursor.saturating_sub(1),
            Step::Right => cursor.saturating_add(1).min(self.len - 1),
            Step::Up | Step::Down => cursor,
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

    /// Address a cell directly, clamped into the field. For a level whose
    /// cursor is owned by the surface drawn in it (the sequencer keeps its
    /// own step cursor) and mirrored here so ancestry stays truthful.
    pub fn set_cursor(&mut self, col: usize, row: usize) {
        self.col = col.min(self.cols - 1);
        self.row = row.min(self.rows - 1);
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

/// A field of cells whose WIDTH follows the model and whose height is
/// fixed: the session lattice, tracks across and scenes down. Unlike
/// [`FocusGrid`] it may have no columns at all — a song with no tracks has
/// nothing to address — so its cursor is optional the way [`FocusRow`]'s
/// is. The row count is fixed at construction because the rows are the
/// stage's own (a head row above the scenes), not the model's yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusLattice {
    cols: usize,
    rows: usize,
    cursor: Option<(usize, usize)>,
}

impl FocusLattice {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows: rows.max(1),
            cursor: (cols > 0).then_some((0, 0)),
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.cols == 0
    }

    /// The addressed cell as `(col, row)`, origin top-left; `None` while
    /// there are no columns to stand in.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    /// Grow or shrink the column count, keeping the cursor addressable
    /// exactly as [`FocusRow::resize`] does: a lattice that gains its
    /// first column gains a cursor, and a cursor that fell off the right
    /// edge is clamped rather than lost. The row is never touched.
    pub fn resize_cols(&mut self, cols: usize) {
        self.cols = cols;
        self.cursor = match (cols, self.cursor) {
            (0, _) => None,
            (_, None) => Some((0, 0)),
            (_, Some((col, row))) => Some((col.min(cols - 1), row)),
        };
    }

    /// Address column `col` directly, staying in the current row. `false`
    /// means there is no such column and nothing moved.
    pub fn focus_col(&mut self, col: usize) -> bool {
        if col >= self.cols {
            return false;
        }
        let row = self.cursor.map_or(0, |(_, row)| row);
        self.cursor = Some((col, row));
        true
    }

    /// Clamp, never wrap, on both axes. `false` means an edge absorbed the
    /// step, and the caller shows that.
    pub fn step(&mut self, step: Step) -> bool {
        let Some((col, row)) = self.cursor else {
            return false;
        };
        let next = match step {
            Step::Up => (col, row.saturating_sub(1)),
            Step::Down => (col, row.saturating_add(1).min(self.rows - 1)),
            Step::Left => (col.saturating_sub(1), row),
            Step::Right => (col.saturating_add(1).min(self.cols - 1), row),
        };
        self.cursor = Some(next);
        next != (col, row)
    }
}

/// A scope's shape reduced to a drawable miniature: a field of cells and
/// the one cell that was entered. A row is one cell tall and a column one
/// cell wide, so ancestry can be drawn without the breadcrumb learning
/// which shape it is looking at — a new scope shape gets a miniature, not
/// a new branch at every drawing site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Miniature {
    pub cols: usize,
    pub rows: usize,
    pub col: usize,
    pub row: usize,
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
    Row(FocusRow),
    Lattice(FocusLattice),
}

impl FocusScope {
    pub fn grid(cols: usize, rows: usize) -> Self {
        Self::Grid(FocusGrid::new(cols, rows))
    }

    pub fn column(len: usize) -> Self {
        Self::Column(FocusColumn::new(len))
    }

    pub fn row(len: usize) -> Self {
        Self::Row(FocusRow::new(len))
    }

    pub fn lattice(cols: usize, rows: usize) -> Self {
        Self::Lattice(FocusLattice::new(cols, rows))
    }

    /// Number of addressable positions. Zero is a first-class shape.
    pub fn len(&self) -> usize {
        match self {
            Self::Grid(grid) => grid.cols().saturating_mul(grid.rows()),
            Self::Column(column) => column.len(),
            Self::Row(row) => row.len(),
            Self::Lattice(lattice) => lattice.cols().saturating_mul(lattice.rows()),
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
            Self::Row(row) => row.cursor(),
            Self::Lattice(lattice) => lattice
                .cursor()
                .map(|(col, row)| row.saturating_mul(lattice.cols()).saturating_add(col)),
        }
    }

    pub fn step(&mut self, step: Step) -> bool {
        match self {
            Self::Grid(grid) => grid.step(step),
            Self::Column(column) => column.step(step),
            Self::Row(row) => row.step(step),
            Self::Lattice(lattice) => lattice.step(step),
        }
    }

    /// This scope as a field of cells. The one-dimensional shapes flatten
    /// onto the axis they actually traverse, so an ancestor is drawable
    /// whatever it is.
    pub fn miniature(&self) -> Miniature {
        match self {
            Self::Grid(grid) => {
                let (col, row) = grid.cursor();
                Miniature {
                    cols: grid.cols(),
                    rows: grid.rows(),
                    col,
                    row,
                }
            }
            Self::Column(column) => Miniature {
                cols: 1,
                rows: column.len(),
                col: 0,
                row: column.cursor().unwrap_or(0),
            },
            Self::Row(row) => Miniature {
                cols: row.len(),
                rows: 1,
                col: row.cursor().unwrap_or(0),
                row: 0,
            },
            Self::Lattice(lattice) => {
                let (col, row) = lattice.cursor().unwrap_or((0, 0));
                Miniature {
                    cols: lattice.cols(),
                    rows: lattice.rows(),
                    col,
                    row,
                }
            }
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

impl From<FocusRow> for FocusScope {
    fn from(row: FocusRow) -> Self {
        Self::Row(row)
    }
}

impl From<FocusLattice> for FocusScope {
    fn from(lattice: FocusLattice) -> Self {
        Self::Lattice(lattice)
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

    /// The active level, mutable, for a surface that owns its own cursor
    /// and reports it back.
    pub fn active_mut(&mut self) -> &mut FocusScope {
        self.levels.last_mut().expect("a stack is never empty")
    }

    /// Root first, active last.
    /// The root level, mutable. A scope standing for something the model
    /// owns has to be able to follow it when the model changes.
    pub fn root_mut(&mut self) -> &mut FocusScope {
        &mut self.levels[0]
    }

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
    fn a_row_that_grows_keeps_its_place_and_one_that_shrinks_is_clamped() {
        let mut row = FocusRow::new(2);
        row.step(Step::Right);
        row.resize(5);
        assert_eq!(row.cursor(), Some(1), "growing moved the cursor");
        row.resize(1);
        assert_eq!(row.cursor(), Some(0), "shrinking lost the cursor");
    }

    #[test]
    fn a_row_that_gains_its_first_position_gains_a_cursor() {
        let mut row = FocusRow::new(0);
        assert_eq!(row.cursor(), None);
        row.resize(3);
        assert_eq!(row.cursor(), Some(0));
        row.resize(0);
        assert_eq!(row.cursor(), None, "an emptied row kept a cursor");
    }

    #[test]
    fn addressing_a_position_the_row_does_not_have_is_refused() {
        let mut row = FocusRow::new(3);
        assert!(row.focus(2));
        assert_eq!(row.cursor(), Some(2));
        assert!(!row.focus(3), "the row accepted a position it lacks");
        assert_eq!(row.cursor(), Some(2), "a refused address moved the cursor");
    }

    #[test]
    fn every_shape_flattens_onto_the_axis_it_traverses() {
        let mut row = FocusScope::row(4);
        row.step(Step::Right);
        assert_eq!(
            row.miniature(),
            Miniature {
                cols: 4,
                rows: 1,
                col: 1,
                row: 0
            }
        );

        let mut column = FocusScope::column(4);
        column.step(Step::Down);
        assert_eq!(
            column.miniature(),
            Miniature {
                cols: 1,
                rows: 4,
                col: 0,
                row: 1
            }
        );

        let mut grid = FocusScope::grid(3, 2);
        grid.step(Step::Right);
        grid.step(Step::Down);
        assert_eq!(
            grid.miniature(),
            Miniature {
                cols: 3,
                rows: 2,
                col: 1,
                row: 1
            }
        );
    }

    #[test]
    fn a_row_traverses_sideways_and_clamps_at_both_ends() {
        let mut row = FocusRow::new(3);
        assert_eq!(row.cursor(), Some(0));
        assert!(!row.step(Step::Left));
        assert!(row.step(Step::Right));
        assert!(row.step(Step::Right));
        assert!(!row.step(Step::Right));
        assert_eq!(row.cursor(), Some(2));
    }

    #[test]
    fn a_row_refuses_the_axis_it_does_not_have() {
        let mut row = FocusRow::new(3);
        assert!(!row.step(Step::Up));
        assert!(!row.step(Step::Down));
        assert_eq!(row.cursor(), Some(0), "a refused step moved the cursor");
    }

    #[test]
    fn an_empty_row_holds_no_cursor_and_refuses_every_step() {
        let mut row = FocusRow::new(0);
        assert!(row.is_empty());
        assert_eq!(row.cursor(), None);
        for step in [Step::Up, Step::Down, Step::Left, Step::Right] {
            assert!(!row.step(step));
        }
    }

    #[test]
    fn a_row_and_a_column_refuse_opposite_axes() {
        let mut row = FocusRow::new(2);
        let mut column = FocusColumn::new(2);
        assert!(row.step(Step::Right) && !row.step(Step::Down));
        assert!(column.step(Step::Down) && !column.step(Step::Right));
    }

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

    #[test]
    fn a_lattice_with_no_columns_has_no_cursor_and_refuses_every_step() {
        let mut lattice = FocusLattice::new(0, 3);
        assert!(lattice.is_empty());
        assert_eq!(lattice.cursor(), None);
        for step in [Step::Up, Step::Down, Step::Left, Step::Right] {
            assert!(
                !lattice.step(step),
                "{step:?} moved a cursor that does not exist"
            );
        }
    }

    #[test]
    fn a_lattice_clamps_on_both_axes_and_reports_the_edge() {
        let mut lattice = FocusLattice::new(2, 3);
        assert!(!lattice.step(Step::Up), "the top row had a neighbour above");
        assert!(
            !lattice.step(Step::Left),
            "the first column had a neighbour left"
        );
        assert!(lattice.step(Step::Down));
        assert!(lattice.step(Step::Down));
        assert!(
            !lattice.step(Step::Down),
            "the last row had a neighbour below"
        );
        assert!(lattice.step(Step::Right));
        assert!(
            !lattice.step(Step::Right),
            "the last column had a neighbour right"
        );
        assert_eq!(lattice.cursor(), Some((1, 2)));
    }

    #[test]
    fn resizing_a_lattice_keeps_the_cursor_addressable_and_keeps_its_row() {
        let mut lattice = FocusLattice::new(3, 4);
        assert!(lattice.step(Step::Down));
        assert!(lattice.step(Step::Right));
        assert!(lattice.step(Step::Right));
        assert_eq!(lattice.cursor(), Some((2, 1)));
        lattice.resize_cols(1);
        assert_eq!(
            lattice.cursor(),
            Some((0, 1)),
            "shrinking lost the row or the cursor"
        );
        lattice.resize_cols(0);
        assert_eq!(lattice.cursor(), None);
        lattice.resize_cols(2);
        assert_eq!(
            lattice.cursor(),
            Some((0, 0)),
            "the first column did not gain a cursor"
        );
    }

    #[test]
    fn focusing_a_column_directly_stays_in_the_row() {
        let mut lattice = FocusLattice::new(3, 4);
        assert!(lattice.step(Step::Down));
        assert!(lattice.focus_col(2));
        assert_eq!(lattice.cursor(), Some((2, 1)));
        assert!(!lattice.focus_col(3), "a column past the end was addressed");
        assert_eq!(lattice.cursor(), Some((2, 1)));
    }

    #[test]
    fn a_lattice_miniature_is_its_own_shape() {
        let mut scope = FocusScope::lattice(3, 2);
        assert!(scope.step(Step::Down));
        assert!(scope.step(Step::Right));
        assert_eq!(
            scope.miniature(),
            Miniature {
                cols: 3,
                rows: 2,
                col: 1,
                row: 1
            }
        );
        assert_eq!(scope.cursor(), Some(4), "the ordinal is row-major");
    }
}
