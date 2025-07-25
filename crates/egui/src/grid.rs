use emath::GuiRounding as _;

use crate::{
    Align2, Color32, Context, Id, InnerResponse, NumExt as _, Painter, Rect, Region, Style, Ui,
    UiBuilder, Vec2, vec2,
};

#[cfg(debug_assertions)]
use crate::Stroke;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct State {
    col_widths: Vec<f32>,
    row_heights: Vec<f32>,
}

impl State {
    pub fn load(ctx: &Context, id: impl Into<Id>) -> Option<Self> {
        ctx.data_mut(|d| d.get_temp(id))
    }

    pub fn store(self, ctx: &Context, id: impl Into<Id>) {
        // We don't persist Grids, because
        // A) there are potentially a lot of them, using up a lot of space (and therefore serialization time)
        // B) if the code changes, the grid _should_ change, and not remember old sizes
        ctx.data_mut(|d| d.insert_temp(id, self));
    }

    fn set_min_col_width(&mut self, col: usize, width: f32) {
        self.col_widths
            .resize(self.col_widths.len().max(col + 1), 0.0);
        self.col_widths[col] = self.col_widths[col].max(width);
    }

    fn set_min_row_height(&mut self, row: usize, height: f32) {
        self.row_heights
            .resize(self.row_heights.len().max(row + 1), 0.0);
        self.row_heights[row] = self.row_heights[row].max(height);
    }

    fn col_width(&self, col: usize) -> Option<f32> {
        self.col_widths.get(col).copied()
    }

    fn row_height(&self, row: usize) -> Option<f32> {
        self.row_heights.get(row).copied()
    }

    fn full_width(&self, x_spacing: f32) -> f32 {
        self.col_widths.iter().sum::<f32>()
            + (self.col_widths.len().at_least(1) - 1) as f32 * x_spacing
    }
}

// ----------------------------------------------------------------------------

// type alias for boxed function to determine row color during grid generation
type ColorPickerFn = Box<dyn Send + Sync + Fn(usize, &Style) -> Option<Color32>>;

pub(crate) struct GridLayout {
    ctx: Context,
    style: std::sync::Arc<Style>,
    id: Id,

    /// First frame (no previous know state).
    is_first_frame: bool,

    /// State previous frame (if any).
    /// This can be used to predict future sizes of cells.
    prev_state: State,

    /// State accumulated during the current frame.
    curr_state: State,
    initial_available: Rect,

    // Options:
    num_columns: Option<usize>,
    spacing: Vec2,
    min_cell_size: Vec2,
    max_cell_size: Vec2,
    color_picker: Option<ColorPickerFn>,

    // Cursor:
    col: usize,
    row: usize,
}

impl GridLayout {
    pub(crate) fn new(ui: &Ui, id: impl Into<Id>, prev_state: Option<State>) -> Self {
        let id = id.into();
        let is_first_frame = prev_state.is_none();
        let prev_state = prev_state.unwrap_or_default();

        // TODO(emilk): respect current layout

        let initial_available = ui.placer().max_rect().intersect(ui.cursor());
        debug_assert!(
            initial_available.min.x.is_finite(),
            "Grid not yet available for right-to-left layouts"
        );

        ui.ctx().check_for_id_clash(id, initial_available, "Grid");

        Self {
            ctx: ui.ctx().clone(),
            style: ui.style().clone(),
            id,
            is_first_frame,
            prev_state,
            curr_state: State::default(),
            initial_available,

            num_columns: None,
            spacing: ui.spacing().item_spacing,
            min_cell_size: ui.spacing().interact_size,
            max_cell_size: Vec2::INFINITY,
            color_picker: None,

            col: 0,
            row: 0,
        }
    }
}

impl GridLayout {
    fn prev_col_width(&self, col: usize) -> f32 {
        self.prev_state
            .col_width(col)
            .unwrap_or(self.min_cell_size.x)
    }

    fn prev_row_height(&self, row: usize) -> f32 {
        self.prev_state
            .row_height(row)
            .unwrap_or(self.min_cell_size.y)
    }

    pub(crate) fn wrap_text(&self) -> bool {
        self.max_cell_size.x.is_finite()
    }

    pub(crate) fn available_rect(&self, region: &Region) -> Rect {
        let is_last_column = Some(self.col + 1) == self.num_columns;

        let width = if is_last_column {
            // The first frame we don't really know the widths of the previous columns,
            // so returning a big available width here can cause trouble.
            if self.is_first_frame {
                self.curr_state
                    .col_width(self.col)
                    .unwrap_or(self.min_cell_size.x)
            } else {
                (self.initial_available.right() - region.cursor.left())
                    .at_most(self.max_cell_size.x)
            }
        } else if self.max_cell_size.x.is_finite() {
            // TODO(emilk): should probably heed `prev_state` here too
            self.max_cell_size.x
        } else {
            // If we want to allow width-filling widgets like [`Separator`] in one of the first cells
            // then we need to make sure they don't spill out of the first cell:
            self.prev_state
                .col_width(self.col)
                .or_else(|| self.curr_state.col_width(self.col))
                .unwrap_or(self.min_cell_size.x)
        };

        // If something above was wider, we can be wider:
        let width = width.max(self.curr_state.col_width(self.col).unwrap_or(0.0));

        let available = region.max_rect.intersect(region.cursor);

        let height = region.max_rect.max.y - available.top();
        let height = height
            .at_least(self.min_cell_size.y)
            .at_most(self.max_cell_size.y);

        Rect::from_min_size(available.min, vec2(width, height))
    }

    pub(crate) fn next_cell(&self, cursor: Rect, child_size: Vec2) -> Rect {
        let width = self.prev_state.col_width(self.col).unwrap_or(0.0);
        let height = self.prev_row_height(self.row);
        let size = child_size.max(vec2(width, height));
        Rect::from_min_size(cursor.min, size).round_ui()
    }

    #[expect(clippy::unused_self)]
    pub(crate) fn align_size_within_rect(&self, size: Vec2, frame: Rect) -> Rect {
        // TODO(emilk): allow this alignment to be customized
        Align2::LEFT_CENTER
            .align_size_within_rect(size, frame)
            .round_ui()
    }

    pub(crate) fn justify_and_align(&self, frame: Rect, size: Vec2) -> Rect {
        self.align_size_within_rect(size, frame)
    }

    pub(crate) fn advance(&mut self, cursor: &mut Rect, _frame_rect: Rect, widget_rect: Rect) {
        #[cfg(debug_assertions)]
        {
            let debug_expand_width = self.style.debug.show_expand_width;
            let debug_expand_height = self.style.debug.show_expand_height;
            if debug_expand_width || debug_expand_height {
                let rect = widget_rect;
                let too_wide = rect.width() > self.prev_col_width(self.col);
                let too_high = rect.height() > self.prev_row_height(self.row);

                if (debug_expand_width && too_wide) || (debug_expand_height && too_high) {
                    let painter = self.ctx.debug_painter();
                    painter.rect_stroke(
                        rect,
                        0.0,
                        (1.0, Color32::LIGHT_BLUE),
                        crate::StrokeKind::Inside,
                    );

                    let stroke = Stroke::new(2.5, Color32::from_rgb(200, 0, 0));
                    let paint_line_seg = |a, b| painter.line_segment([a, b], stroke);

                    if debug_expand_width && too_wide {
                        paint_line_seg(rect.left_top(), rect.left_bottom());
                        paint_line_seg(rect.left_center(), rect.right_center());
                        paint_line_seg(rect.right_top(), rect.right_bottom());
                    }
                }
            }
        }

        self.curr_state
            .set_min_col_width(self.col, widget_rect.width().max(self.min_cell_size.x));
        self.curr_state
            .set_min_row_height(self.row, widget_rect.height().max(self.min_cell_size.y));

        cursor.min.x += self.prev_col_width(self.col) + self.spacing.x;
        self.col += 1;
    }

    fn paint_row(&self, cursor: &Rect, painter: &Painter) {
        // handle row color painting based on color-picker function
        let Some(color_picker) = self.color_picker.as_ref() else {
            return;
        };
        let Some(row_color) = color_picker(self.row, &self.style) else {
            return;
        };
        let Some(height) = self.prev_state.row_height(self.row) else {
            return;
        };
        // Paint background for coming row:
        let size = Vec2::new(self.prev_state.full_width(self.spacing.x), height);
        let rect = Rect::from_min_size(cursor.min, size);
        let rect = rect.expand2(0.5 * self.spacing.y * Vec2::Y);
        let rect = rect.expand2(2.0 * Vec2::X); // HACK: just looks better with some spacing on the sides

        painter.rect_filled(rect, 2.0, row_color);
    }

    pub(crate) fn end_row(&mut self, cursor: &mut Rect, painter: &Painter) {
        cursor.min.x = self.initial_available.min.x;
        cursor.min.y += self.spacing.y;
        cursor.min.y += self
            .curr_state
            .row_height(self.row)
            .unwrap_or(self.min_cell_size.y);

        self.col = 0;
        self.row += 1;

        self.paint_row(cursor, painter);
    }

    pub(crate) fn save(&self) {
        // We need to always save state on the first frame, otherwise request_discard
        // would be called repeatedly (see #5132)
        if self.curr_state != self.prev_state || self.is_first_frame {
            self.curr_state.clone().store(&self.ctx, self.id);
            self.ctx.request_repaint();
        }
    }
}

// ----------------------------------------------------------------------------

/// A simple grid layout.
///
/// The cells are always laid out left to right, top-down.
/// The contents of each cell will be aligned to the left and center.
///
/// If you want to add multiple widgets to a cell you need to group them with
/// [`Ui::horizontal`], [`Ui::vertical`] etc.
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// egui::Grid::new("some_unique_id").show(ui, |ui| {
///     ui.label("First row, first column");
///     ui.label("First row, second column");
///     ui.end_row();
///
///     ui.label("Second row, first column");
///     ui.label("Second row, second column");
///     ui.label("Second row, third column");
///     ui.end_row();
///
///     ui.horizontal(|ui| { ui.label("Same"); ui.label("cell"); });
///     ui.label("Third row, second column");
///     ui.end_row();
/// });
/// # });
/// ```
#[must_use = "You should call .show()"]
pub struct Grid {
    id_salt: Id,
    num_columns: Option<usize>,
    min_col_width: Option<f32>,
    min_row_height: Option<f32>,
    max_cell_size: Vec2,
    spacing: Option<Vec2>,
    start_row: usize,
    color_picker: Option<ColorPickerFn>,
}

impl Grid {
    /// Create a new [`Grid`] with a locally unique identifier.
    pub fn new(id_salt: impl std::hash::Hash) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            num_columns: None,
            min_col_width: None,
            min_row_height: None,
            max_cell_size: Vec2::INFINITY,
            spacing: None,
            start_row: 0,
            color_picker: None,
        }
    }

    /// Setting this will allow for dynamic coloring of rows of the grid object
    #[inline]
    pub fn with_row_color<F>(mut self, color_picker: F) -> Self
    where
        F: Send + Sync + Fn(usize, &Style) -> Option<Color32> + 'static,
    {
        self.color_picker = Some(Box::new(color_picker));
        self
    }

    /// Setting this will allow the last column to expand to take up the rest of the space of the parent [`Ui`].
    #[inline]
    pub fn num_columns(mut self, num_columns: usize) -> Self {
        self.num_columns = Some(num_columns);
        self
    }

    /// If `true`, add a subtle background color to every other row.
    ///
    /// This can make a table easier to read.
    /// Default is whatever is in [`crate::Visuals::striped`].
    pub fn striped(self, striped: bool) -> Self {
        if striped {
            self.with_row_color(striped_row_color)
        } else {
            // Explicitly set the row color to nothing.
            // Needed so that when the style.visuals.striped value is checked later on,
            // it is clear that the user does not want stripes on this specific Grid.
            self.with_row_color(|_row: usize, _style: &Style| None)
        }
    }

    /// Set minimum width of each column.
    /// Default: [`crate::style::Spacing::interact_size`]`.x`.
    #[inline]
    pub fn min_col_width(mut self, min_col_width: f32) -> Self {
        self.min_col_width = Some(min_col_width);
        self
    }

    /// Set minimum height of each row.
    /// Default: [`crate::style::Spacing::interact_size`]`.y`.
    #[inline]
    pub fn min_row_height(mut self, min_row_height: f32) -> Self {
        self.min_row_height = Some(min_row_height);
        self
    }

    /// Set soft maximum width (wrapping width) of each column.
    #[inline]
    pub fn max_col_width(mut self, max_col_width: f32) -> Self {
        self.max_cell_size.x = max_col_width;
        self
    }

    /// Set spacing between columns/rows.
    /// Default: [`crate::style::Spacing::item_spacing`].
    #[inline]
    pub fn spacing(mut self, spacing: impl Into<Vec2>) -> Self {
        self.spacing = Some(spacing.into());
        self
    }

    /// Change which row number the grid starts on.
    /// This can be useful when you have a large [`crate::Grid`] inside of [`crate::ScrollArea::show_rows`].
    #[inline]
    pub fn start_row(mut self, start_row: usize) -> Self {
        self.start_row = start_row;
        self
    }
}

impl Grid {
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.show_dyn(ui, Box::new(add_contents))
    }

    fn show_dyn<'c, R>(
        self,
        ui: &mut Ui,
        add_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
    ) -> InnerResponse<R> {
        let Self {
            id_salt,
            num_columns,
            min_col_width,
            min_row_height,
            max_cell_size,
            spacing,
            start_row,
            mut color_picker,
        } = self;
        let min_col_width = min_col_width.unwrap_or_else(|| ui.spacing().interact_size.x);
        let min_row_height = min_row_height.unwrap_or_else(|| ui.spacing().interact_size.y);
        let spacing = spacing.unwrap_or_else(|| ui.spacing().item_spacing);
        if color_picker.is_none() && ui.visuals().striped {
            color_picker = Some(Box::new(striped_row_color));
        }

        let id = ui.make_persistent_id(id_salt);
        let prev_state = State::load(ui.ctx(), id);

        // Each grid cell is aligned LEFT_CENTER.
        // If somebody wants to wrap more things inside a cell,
        // then we should pick a default layout that matches that alignment,
        // which we do here:
        let max_rect = ui.cursor().intersect(ui.max_rect());

        let mut ui_builder = UiBuilder::new().max_rect(max_rect);
        if prev_state.is_none() {
            // The initial frame will be glitchy, because we don't know the sizes of things to come.

            if ui.is_visible() {
                // Try to cover up the glitchy initial frame:
                ui.ctx().request_discard("new Grid");
            }

            // Hide the ui this frame, and make things as narrow as possible:
            ui_builder = ui_builder.sizing_pass().invisible();
        }

        ui.scope_builder(ui_builder, |ui| {
            ui.horizontal(|ui| {
                let is_color = color_picker.is_some();
                let grid = GridLayout {
                    num_columns,
                    color_picker,
                    min_cell_size: vec2(min_col_width, min_row_height),
                    max_cell_size,
                    spacing,
                    row: start_row,
                    ..GridLayout::new(ui, id, prev_state)
                };

                // paint first incoming row
                if is_color {
                    let cursor = ui.cursor();
                    let painter = ui.painter();
                    grid.paint_row(&cursor, painter);
                }

                ui.set_grid(grid);
                let r = add_contents(ui);
                ui.save_grid();
                r
            })
            .inner
        })
    }
}

fn striped_row_color(row: usize, style: &Style) -> Option<Color32> {
    if row % 2 == 1 {
        return Some(style.visuals.faint_bg_color);
    }
    None
}
