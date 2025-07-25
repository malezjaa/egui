use std::sync::Arc;

use emath::{Rect, TSTransform};
use epaint::{
    StrokeKind,
    text::{Galley, LayoutJob, cursor::CCursor},
};

use crate::{
    Align, Align2, Color32, Context, CursorIcon, Event, EventFilter, FontSelection, Id, ImeEvent,
    Key, KeyboardShortcut, Margin, Modifiers, NumExt as _, Response, Sense, Shape, TextBuffer,
    TextStyle, TextWrapMode, Ui, Vec2, Widget, WidgetInfo, WidgetText, WidgetWithState, epaint,
    os::OperatingSystem,
    output::OutputEvent,
    response, text_selection,
    text_selection::{CCursorRange, text_cursor_state::cursor_rect, visuals::paint_text_selection},
    vec2,
};

use super::{TextEditOutput, TextEditState};

type LayouterFn<'t> = &'t mut dyn FnMut(&Ui, &dyn TextBuffer, f32) -> Arc<Galley>;

/// A text region that the user can edit the contents of.
///
/// See also [`Ui::text_edit_singleline`] and [`Ui::text_edit_multiline`].
///
/// Example:
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// # let mut my_string = String::new();
/// let response = ui.add(egui::TextEdit::singleline(&mut my_string));
/// if response.changed() {
///     // …
/// }
/// if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
///     // …
/// }
/// # });
/// ```
///
/// To fill an [`Ui`] with a [`TextEdit`] use [`Ui::add_sized`]:
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// # let mut my_string = String::new();
/// ui.add_sized(ui.available_size(), egui::TextEdit::multiline(&mut my_string));
/// # });
/// ```
///
///
/// You can also use [`TextEdit`] to show text that can be selected, but not edited.
/// To do so, pass in a `&mut` reference to a `&str`, for instance:
///
/// ```
/// fn selectable_text(ui: &mut egui::Ui, mut text: &str) {
///     ui.add(egui::TextEdit::multiline(&mut text));
/// }
/// ```
///
/// ## Advanced usage
/// See [`TextEdit::show`].
///
/// ## Other
/// The background color of a [`crate::TextEdit`] is [`crate::Visuals::text_edit_bg_color`] or can be set with [`crate::TextEdit::background_color`].
#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
pub struct TextEdit<'t> {
    text: &'t mut dyn TextBuffer,
    hint_text: WidgetText,
    hint_text_font: Option<FontSelection>,
    id: Option<Id>,
    id_salt: Option<Id>,
    font_selection: FontSelection,
    text_color: Option<Color32>,
    layouter: Option<LayouterFn<'t>>,
    password: bool,
    frame: bool,
    margin: Margin,
    multiline: bool,
    interactive: bool,
    desired_width: Option<f32>,
    desired_height_rows: usize,
    event_filter: EventFilter,
    cursor_at_end: bool,
    min_size: Vec2,
    align: Align2,
    clip_text: bool,
    char_limit: usize,
    return_key: Option<KeyboardShortcut>,
    background_color: Option<Color32>,
}

impl WidgetWithState for TextEdit<'_> {
    type State = TextEditState;
}

impl TextEdit<'_> {
    pub fn load_state(ctx: &Context, id: impl Into<Id>) -> Option<TextEditState> {
        TextEditState::load(ctx, id)
    }

    pub fn store_state(ctx: &Context, id: impl Into<Id>, state: TextEditState) {
        state.store(ctx, id);
    }
}

impl<'t> TextEdit<'t> {
    /// No newlines (`\n`) allowed. Pressing enter key will result in the [`TextEdit`] losing focus (`response.lost_focus`).
    pub fn singleline(text: &'t mut dyn TextBuffer) -> Self {
        Self {
            desired_height_rows: 1,
            multiline: false,
            clip_text: true,
            ..Self::multiline(text)
        }
    }

    /// A [`TextEdit`] for multiple lines. Pressing enter key will create a new line by default (can be changed with [`return_key`](TextEdit::return_key)).
    pub fn multiline(text: &'t mut dyn TextBuffer) -> Self {
        Self {
            text,
            hint_text: Default::default(),
            hint_text_font: None,
            id: None,
            id_salt: None,
            font_selection: Default::default(),
            text_color: None,
            layouter: None,
            password: false,
            frame: true,
            margin: Margin::symmetric(4, 2),
            multiline: true,
            interactive: true,
            desired_width: None,
            desired_height_rows: 4,
            event_filter: EventFilter {
                // moving the cursor is really important
                horizontal_arrows: true,
                vertical_arrows: true,
                tab: false, // tab is used to change focus, not to insert a tab character
                ..Default::default()
            },
            cursor_at_end: true,
            min_size: Vec2::ZERO,
            align: Align2::LEFT_TOP,
            clip_text: false,
            char_limit: usize::MAX,
            return_key: Some(KeyboardShortcut::new(Modifiers::NONE, Key::Enter)),
            background_color: None,
        }
    }

    /// Build a [`TextEdit`] focused on code editing.
    /// By default it comes with:
    /// - monospaced font
    /// - focus lock (tab will insert a tab character instead of moving focus)
    pub fn code_editor(self) -> Self {
        self.font(TextStyle::Monospace).lock_focus(true)
    }

    /// Use if you want to set an explicit [`Id`] for this widget.
    #[inline]
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// A source for the unique [`Id`], e.g. `.id_source("second_text_edit_field")` or `.id_source(loop_index)`.
    #[inline]
    pub fn id_source(self, id_salt: impl std::hash::Hash) -> Self {
        self.id_salt(id_salt)
    }

    /// A source for the unique [`Id`], e.g. `.id_salt("second_text_edit_field")` or `.id_salt(loop_index)`.
    #[inline]
    pub fn id_salt(mut self, id_salt: impl std::hash::Hash) -> Self {
        self.id_salt = Some(Id::new(id_salt));
        self
    }

    /// Show a faint hint text when the text field is empty.
    ///
    /// If the hint text needs to be persisted even when the text field has input,
    /// the following workaround can be used:
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_string = String::new();
    /// # use egui::{ Color32, FontId };
    /// let text_edit = egui::TextEdit::multiline(&mut my_string)
    ///     .desired_width(f32::INFINITY);
    /// let output = text_edit.show(ui);
    /// let painter = ui.painter_at(output.response.rect);
    /// let text_color = Color32::from_rgba_premultiplied(100, 100, 100, 100);
    /// let galley = painter.layout(
    ///     String::from("Enter text"),
    ///     FontId::default(),
    ///     text_color,
    ///     f32::INFINITY
    /// );
    /// painter.galley(output.galley_pos, galley, text_color);
    /// # });
    /// ```
    #[inline]
    pub fn hint_text(mut self, hint_text: impl Into<WidgetText>) -> Self {
        self.hint_text = hint_text.into();
        self
    }

    /// Set the background color of the [`TextEdit`]. The default is [`crate::Visuals::text_edit_bg_color`].
    // TODO(bircni): remove this once #3284 is implemented
    #[inline]
    pub fn background_color(mut self, color: Color32) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Set a specific style for the hint text.
    #[inline]
    pub fn hint_text_font(mut self, hint_text_font: impl Into<FontSelection>) -> Self {
        self.hint_text_font = Some(hint_text_font.into());
        self
    }

    /// If true, hide the letters from view and prevent copying from the field.
    #[inline]
    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    /// Pick a [`crate::FontId`] or [`TextStyle`].
    #[inline]
    pub fn font(mut self, font_selection: impl Into<FontSelection>) -> Self {
        self.font_selection = font_selection.into();
        self
    }

    #[inline]
    pub fn text_color(mut self, text_color: Color32) -> Self {
        self.text_color = Some(text_color);
        self
    }

    #[inline]
    pub fn text_color_opt(mut self, text_color: Option<Color32>) -> Self {
        self.text_color = text_color;
        self
    }

    /// Override how text is being shown inside the [`TextEdit`].
    ///
    /// This can be used to implement things like syntax highlighting.
    ///
    /// This function will be called at least once per frame,
    /// so it is strongly suggested that you cache the results of any syntax highlighter
    /// so as not to waste CPU highlighting the same string every frame.
    ///
    /// The arguments is the enclosing [`Ui`] (so you can access e.g. [`Ui::fonts`]),
    /// the text and the wrap width.
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_code = String::new();
    /// # fn my_memoized_highlighter(s: &str) -> egui::text::LayoutJob { Default::default() }
    /// let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
    ///     let mut layout_job: egui::text::LayoutJob = my_memoized_highlighter(buf.as_str());
    ///     layout_job.wrap.max_width = wrap_width;
    ///     ui.fonts(|f| f.layout_job(layout_job))
    /// };
    /// ui.add(egui::TextEdit::multiline(&mut my_code).layouter(&mut layouter));
    /// # });
    /// ```
    #[inline]
    pub fn layouter(
        mut self,
        layouter: &'t mut dyn FnMut(&Ui, &dyn TextBuffer, f32) -> Arc<Galley>,
    ) -> Self {
        self.layouter = Some(layouter);

        self
    }

    /// Default is `true`. If set to `false` then you cannot interact with the text (neither edit or select it).
    ///
    /// Consider using [`Ui::add_enabled`] instead to also give the [`TextEdit`] a greyed out look.
    #[inline]
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Default is `true`. If set to `false` there will be no frame showing that this is editable text!
    #[inline]
    pub fn frame(mut self, frame: bool) -> Self {
        self.frame = frame;
        self
    }

    /// Set margin of text. Default is `Margin::symmetric(4.0, 2.0)`
    #[inline]
    pub fn margin(mut self, margin: impl Into<Margin>) -> Self {
        self.margin = margin.into();
        self
    }

    /// Set to 0.0 to keep as small as possible.
    /// Set to [`f32::INFINITY`] to take up all available space (i.e. disable automatic word wrap).
    #[inline]
    pub fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = Some(desired_width);
        self
    }

    /// Set the number of rows to show by default.
    /// The default for singleline text is `1`.
    /// The default for multiline text is `4`.
    #[inline]
    pub fn desired_rows(mut self, desired_height_rows: usize) -> Self {
        self.desired_height_rows = desired_height_rows;
        self
    }

    /// When `false` (default), pressing TAB will move focus
    /// to the next widget.
    ///
    /// When `true`, the widget will keep the focus and pressing TAB
    /// will insert the `'\t'` character.
    #[inline]
    pub fn lock_focus(mut self, tab_will_indent: bool) -> Self {
        self.event_filter.tab = tab_will_indent;
        self
    }

    /// When `true` (default), the cursor will initially be placed at the end of the text.
    ///
    /// When `false`, the cursor will initially be placed at the beginning of the text.
    #[inline]
    pub fn cursor_at_end(mut self, b: bool) -> Self {
        self.cursor_at_end = b;
        self
    }

    /// When `true` (default), overflowing text will be clipped.
    ///
    /// When `false`, widget width will expand to make all text visible.
    ///
    /// This only works for singleline [`TextEdit`].
    #[inline]
    pub fn clip_text(mut self, b: bool) -> Self {
        // always show everything in multiline
        if !self.multiline {
            self.clip_text = b;
        }
        self
    }

    /// Sets the limit for the amount of characters can be entered
    ///
    /// This only works for singleline [`TextEdit`]
    #[inline]
    pub fn char_limit(mut self, limit: usize) -> Self {
        self.char_limit = limit;
        self
    }

    /// Set the horizontal align of the inner text.
    #[inline]
    pub fn horizontal_align(mut self, align: Align) -> Self {
        self.align.0[0] = align;
        self
    }

    /// Set the vertical align of the inner text.
    #[inline]
    pub fn vertical_align(mut self, align: Align) -> Self {
        self.align.0[1] = align;
        self
    }

    /// Set the minimum size of the [`TextEdit`].
    #[inline]
    pub fn min_size(mut self, min_size: Vec2) -> Self {
        self.min_size = min_size;
        self
    }

    /// Set the return key combination.
    ///
    /// This combination will cause a newline on multiline,
    /// whereas on singleline it will cause the widget to lose focus.
    ///
    /// This combination is optional and can be disabled by passing [`None`] into this function.
    #[inline]
    pub fn return_key(mut self, return_key: impl Into<Option<KeyboardShortcut>>) -> Self {
        self.return_key = return_key.into();
        self
    }
}

// ----------------------------------------------------------------------------

impl Widget for TextEdit<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

impl TextEdit<'_> {
    /// Show the [`TextEdit`], returning a rich [`TextEditOutput`].
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_string = String::new();
    /// let output = egui::TextEdit::singleline(&mut my_string).show(ui);
    /// if let Some(text_cursor_range) = output.cursor_range {
    ///     use egui::TextBuffer as _;
    ///     let selected_chars = text_cursor_range.as_sorted_char_range();
    ///     let selected_text = my_string.char_range(selected_chars);
    ///     ui.label("Selected text: ");
    ///     ui.monospace(selected_text);
    /// }
    /// # });
    /// ```
    pub fn show(self, ui: &mut Ui) -> TextEditOutput {
        let is_mutable = self.text.is_mutable();
        let frame = self.frame;
        let where_to_put_background = ui.painter().add(Shape::Noop);
        let background_color = self
            .background_color
            .unwrap_or_else(|| ui.visuals().text_edit_bg_color());
        let output = self.show_content(ui);

        if frame {
            let visuals = ui.style().interact(&output.response);
            let frame_rect = output.response.rect.expand(visuals.expansion);
            let shape = if is_mutable {
                if output.response.has_focus() {
                    epaint::RectShape::new(
                        frame_rect,
                        visuals.corner_radius,
                        background_color,
                        ui.visuals().selection.stroke,
                        StrokeKind::Inside,
                    )
                } else {
                    epaint::RectShape::new(
                        frame_rect,
                        visuals.corner_radius,
                        background_color,
                        visuals.bg_stroke, // TODO(emilk): we want to show something here, or a text-edit field doesn't "pop".
                        StrokeKind::Inside,
                    )
                }
            } else {
                let visuals = &ui.style().visuals.widgets.inactive;
                epaint::RectShape::stroke(
                    frame_rect,
                    visuals.corner_radius,
                    visuals.bg_stroke, // TODO(emilk): we want to show something here, or a text-edit field doesn't "pop".
                    StrokeKind::Inside,
                )
            };

            ui.painter().set(where_to_put_background, shape);
        }

        output
    }

    fn show_content(self, ui: &mut Ui) -> TextEditOutput {
        let TextEdit {
            text,
            hint_text,
            hint_text_font,
            id,
            id_salt,
            font_selection,
            text_color,
            layouter,
            password,
            frame: _,
            margin,
            multiline,
            interactive,
            desired_width,
            desired_height_rows,
            event_filter,
            cursor_at_end,
            min_size,
            align,
            clip_text,
            char_limit,
            return_key,
            background_color: _,
        } = self;

        let text_color = text_color
            .or(ui.visuals().override_text_color)
            // .unwrap_or_else(|| ui.style().interact(&response).text_color()); // too bright
            .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());

        let prev_text = text.as_str().to_owned();
        let hint_text_str = hint_text.text().to_owned();

        let font_id = font_selection.resolve(ui.style());
        let row_height = ui.fonts(|f| f.row_height(&font_id));
        const MIN_WIDTH: f32 = 24.0; // Never make a [`TextEdit`] more narrow than this.
        let available_width = (ui.available_width() - margin.sum().x).at_least(MIN_WIDTH);
        let desired_width = desired_width.unwrap_or_else(|| ui.spacing().text_edit_width);
        let wrap_width = if ui.layout().horizontal_justify() {
            available_width
        } else {
            desired_width.min(available_width)
        };

        let font_id_clone = font_id.clone();
        let mut default_layouter = move |ui: &Ui, text: &dyn TextBuffer, wrap_width: f32| {
            let text = mask_if_password(password, text.as_str());
            let layout_job = if multiline {
                LayoutJob::simple(text, font_id_clone.clone(), text_color, wrap_width)
            } else {
                LayoutJob::simple_singleline(text, font_id_clone.clone(), text_color)
            };
            ui.fonts(|f| f.layout_job(layout_job))
        };

        let layouter = layouter.unwrap_or(&mut default_layouter);

        let mut galley = layouter(ui, text, wrap_width);

        let desired_inner_width = if clip_text {
            wrap_width // visual clipping with scroll in singleline input.
        } else {
            galley.size().x.max(wrap_width)
        };
        let desired_height = (desired_height_rows.at_least(1) as f32) * row_height;
        let desired_inner_size = vec2(desired_inner_width, galley.size().y.max(desired_height));
        let desired_outer_size = (desired_inner_size + margin.sum()).at_least(min_size);
        let (auto_id, outer_rect) = ui.allocate_space(desired_outer_size);
        let rect = outer_rect - margin; // inner rect (excluding frame/margin).

        let id = id.unwrap_or_else(|| {
            if let Some(id_salt) = id_salt {
                ui.make_persistent_id(id_salt)
            } else {
                auto_id // Since we are only storing the cursor a persistent Id is not super important
            }
        });
        let mut state = TextEditState::load(ui.ctx(), id).unwrap_or_default();

        // On touch screens (e.g. mobile in `eframe` web), should
        // dragging select text, or scroll the enclosing [`ScrollArea`] (if any)?
        // Since currently copying selected text in not supported on `eframe` web,
        // we prioritize touch-scrolling:
        let allow_drag_to_select =
            ui.input(|i| !i.has_touch_screen()) || ui.memory(|mem| mem.has_focus(id));

        let sense = if interactive {
            if allow_drag_to_select {
                Sense::click_and_drag()
            } else {
                Sense::click()
            }
        } else {
            Sense::hover()
        };
        let mut response = ui.interact(outer_rect, id, sense);
        response.intrinsic_size = Some(Vec2::new(desired_width, desired_outer_size.y));

        // Don't sent `OutputEvent::Clicked` when a user presses the space bar
        response.flags -= response::Flags::FAKE_PRIMARY_CLICKED;
        let text_clip_rect = rect;
        let painter = ui.painter_at(text_clip_rect.expand(1.0)); // expand to avoid clipping cursor

        if interactive {
            if let Some(pointer_pos) = response.interact_pointer_pos() {
                if response.hovered() && text.is_mutable() {
                    ui.output_mut(|o| o.mutable_text_under_cursor = true);
                }

                // TODO(emilk): drag selected text to either move or clone (ctrl on windows, alt on mac)

                let singleline_offset = vec2(state.singleline_offset, 0.0);
                let cursor_at_pointer =
                    galley.cursor_from_pos(pointer_pos - rect.min + singleline_offset);

                if ui.visuals().text_cursor.preview
                    && response.hovered()
                    && ui.input(|i| i.pointer.is_moving())
                {
                    // text cursor preview:
                    let cursor_rect = TSTransform::from_translation(rect.min.to_vec2())
                        * cursor_rect(&galley, &cursor_at_pointer, row_height);
                    text_selection::visuals::paint_cursor_end(&painter, ui.visuals(), cursor_rect);
                }

                let is_being_dragged = ui.ctx().is_being_dragged(response.id);
                let did_interact = state.cursor.pointer_interaction(
                    ui,
                    &response,
                    cursor_at_pointer,
                    &galley,
                    is_being_dragged,
                );

                if did_interact || response.clicked() {
                    ui.memory_mut(|mem| mem.request_focus(response.id));

                    state.last_interaction_time = ui.ctx().input(|i| i.time);
                }
            }
        }

        if interactive && response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }

        let mut cursor_range = None;
        let prev_cursor_range = state.cursor.range(&galley);
        if interactive && ui.memory(|mem| mem.has_focus(id)) {
            ui.memory_mut(|mem| mem.set_focus_lock_filter(id, event_filter));

            let default_cursor_range = if cursor_at_end {
                CCursorRange::one(galley.end())
            } else {
                CCursorRange::default()
            };

            let (changed, new_cursor_range) = events(
                ui,
                &mut state,
                text,
                &mut galley,
                layouter,
                id,
                wrap_width,
                multiline,
                password,
                default_cursor_range,
                char_limit,
                event_filter,
                return_key,
            );

            if changed {
                response.mark_changed();
            }
            cursor_range = Some(new_cursor_range);
        }

        let mut galley_pos = align
            .align_size_within_rect(galley.size(), rect)
            .intersect(rect) // limit pos to the response rect area
            .min;
        let align_offset = rect.left() - galley_pos.x;

        // Visual clipping for singleline text editor with text larger than width
        if clip_text && align_offset == 0.0 {
            let cursor_pos = match (cursor_range, ui.memory(|mem| mem.has_focus(id))) {
                (Some(cursor_range), true) => galley.pos_from_cursor(cursor_range.primary).min.x,
                _ => 0.0,
            };

            let mut offset_x = state.singleline_offset;
            let visible_range = offset_x..=offset_x + desired_inner_size.x;

            if !visible_range.contains(&cursor_pos) {
                if cursor_pos < *visible_range.start() {
                    offset_x = cursor_pos;
                } else {
                    offset_x = cursor_pos - desired_inner_size.x;
                }
            }

            offset_x = offset_x
                .at_most(galley.size().x - desired_inner_size.x)
                .at_least(0.0);

            state.singleline_offset = offset_x;
            galley_pos -= vec2(offset_x, 0.0);
        } else {
            state.singleline_offset = align_offset;
        }

        let selection_changed = if let (Some(cursor_range), Some(prev_cursor_range)) =
            (cursor_range, prev_cursor_range)
        {
            prev_cursor_range != cursor_range
        } else {
            false
        };

        if ui.is_rect_visible(rect) {
            if text.as_str().is_empty() && !hint_text.is_empty() {
                let hint_text_color = ui.visuals().weak_text_color();
                let hint_text_font_id = hint_text_font.unwrap_or(font_id.into());
                let galley = if multiline {
                    hint_text.into_galley(
                        ui,
                        Some(TextWrapMode::Wrap),
                        desired_inner_size.x,
                        hint_text_font_id,
                    )
                } else {
                    hint_text.into_galley(
                        ui,
                        Some(TextWrapMode::Extend),
                        f32::INFINITY,
                        hint_text_font_id,
                    )
                };
                let galley_pos = align
                    .align_size_within_rect(galley.size(), rect)
                    .intersect(rect)
                    .min;
                painter.galley(galley_pos, galley, hint_text_color);
            }

            let has_focus = ui.memory(|mem| mem.has_focus(id));

            if has_focus {
                if let Some(cursor_range) = state.cursor.range(&galley) {
                    // Add text selection rectangles to the galley:
                    paint_text_selection(&mut galley, ui.visuals(), &cursor_range, None);
                }
            }

            if !clip_text {
                // Allocate additional space if edits were made this frame that changed the size. This is important so that,
                // if there's a ScrollArea, it can properly scroll to the cursor.
                // Condition `!clip_text` is important to avoid breaking layout for `TextEdit::singleline` (PR #5640)
                let extra_size = galley.size() - rect.size();
                if extra_size.x > 0.0 || extra_size.y > 0.0 {
                    ui.allocate_rect(
                        Rect::from_min_size(outer_rect.max, extra_size),
                        Sense::hover(),
                    );
                }
            }

            painter.galley(galley_pos, galley.clone(), text_color);

            if has_focus {
                if let Some(cursor_range) = state.cursor.range(&galley) {
                    let primary_cursor_rect =
                        cursor_rect(&galley, &cursor_range.primary, row_height)
                            .translate(galley_pos.to_vec2());

                    if response.changed() || selection_changed {
                        // Scroll to keep primary cursor in view:
                        ui.scroll_to_rect(primary_cursor_rect + margin, None);
                    }

                    if text.is_mutable() && interactive {
                        let now = ui.ctx().input(|i| i.time);
                        if response.changed() || selection_changed {
                            state.last_interaction_time = now;
                        }

                        // Only show (and blink) cursor if the egui viewport has focus.
                        // This is for two reasons:
                        // * Don't give the impression that the user can type into a window without focus
                        // * Don't repaint the ui because of a blinking cursor in an app that is not in focus
                        let viewport_has_focus = ui.ctx().input(|i| i.focused);
                        if viewport_has_focus {
                            text_selection::visuals::paint_text_cursor(
                                ui,
                                &painter,
                                primary_cursor_rect,
                                now - state.last_interaction_time,
                            );
                        }

                        // Set IME output (in screen coords) when text is editable and visible
                        let to_global = ui
                            .ctx()
                            .layer_transform_to_global(ui.layer_id())
                            .unwrap_or_default();

                        ui.ctx().output_mut(|o| {
                            o.ime = Some(crate::output::IMEOutput {
                                rect: to_global * rect,
                                cursor_rect: to_global * primary_cursor_rect,
                            });
                        });
                    }
                }
            }
        }

        // Ensures correct IME behavior when the text input area gains or loses focus.
        if state.ime_enabled && (response.gained_focus() || response.lost_focus()) {
            state.ime_enabled = false;
            if let Some(mut ccursor_range) = state.cursor.char_range() {
                ccursor_range.secondary.index = ccursor_range.primary.index;
                state.cursor.set_char_range(Some(ccursor_range));
            }
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Ime(_))));
        }

        state.clone().store(ui.ctx(), id);

        if response.changed() {
            response.widget_info(|| {
                WidgetInfo::text_edit(
                    ui.is_enabled(),
                    mask_if_password(password, prev_text.as_str()),
                    mask_if_password(password, text.as_str()),
                    hint_text_str.as_str(),
                )
            });
        } else if selection_changed {
            let cursor_range = cursor_range.unwrap();
            let char_range = cursor_range.primary.index..=cursor_range.secondary.index;
            let info = WidgetInfo::text_selection_changed(
                ui.is_enabled(),
                char_range,
                mask_if_password(password, text.as_str()),
            );
            response.output_event(OutputEvent::TextSelectionChanged(info));
        } else {
            response.widget_info(|| {
                WidgetInfo::text_edit(
                    ui.is_enabled(),
                    mask_if_password(password, prev_text.as_str()),
                    mask_if_password(password, text.as_str()),
                    hint_text_str.as_str(),
                )
            });
        }

        #[cfg(feature = "accesskit")]
        {
            let role = if password {
                accesskit::Role::PasswordInput
            } else if multiline {
                accesskit::Role::MultilineTextInput
            } else {
                accesskit::Role::TextInput
            };

            crate::text_selection::accesskit_text::update_accesskit_for_text_widget(
                ui.ctx(),
                id,
                cursor_range,
                role,
                TSTransform::from_translation(galley_pos.to_vec2()),
                &galley,
            );
        }

        TextEditOutput {
            response,
            galley,
            galley_pos,
            text_clip_rect,
            state,
            cursor_range,
        }
    }
}

fn mask_if_password(is_password: bool, text: &str) -> String {
    fn mask_password(text: &str) -> String {
        std::iter::repeat_n(
            epaint::text::PASSWORD_REPLACEMENT_CHAR,
            text.chars().count(),
        )
        .collect::<String>()
    }

    if is_password {
        mask_password(text)
    } else {
        text.to_owned()
    }
}

// ----------------------------------------------------------------------------

/// Check for (keyboard) events to edit the cursor and/or text.
#[expect(clippy::too_many_arguments)]
fn events(
    ui: &crate::Ui,
    state: &mut TextEditState,
    text: &mut dyn TextBuffer,
    galley: &mut Arc<Galley>,
    layouter: &mut dyn FnMut(&Ui, &dyn TextBuffer, f32) -> Arc<Galley>,
    id: impl Into<Id>,
    wrap_width: f32,
    multiline: bool,
    password: bool,
    default_cursor_range: CCursorRange,
    char_limit: usize,
    event_filter: EventFilter,
    return_key: Option<KeyboardShortcut>,
) -> (bool, CCursorRange) {
    let id = id.into();
    let os = ui.ctx().os();

    let mut cursor_range = state.cursor.range(galley).unwrap_or(default_cursor_range);

    // We feed state to the undoer both before and after handling input
    // so that the undoer creates automatic saves even when there are no events for a while.
    state.undoer.lock().feed_state(
        ui.input(|i| i.time),
        &(cursor_range, text.as_str().to_owned()),
    );

    let copy_if_not_password = |ui: &Ui, text: String| {
        if !password {
            ui.ctx().copy_text(text);
        }
    };

    let mut any_change = false;

    let mut events = ui.input(|i| i.filtered_events(&event_filter));

    if state.ime_enabled {
        remove_ime_incompatible_events(&mut events);
        // Process IME events first:
        events.sort_by_key(|e| !matches!(e, Event::Ime(_)));
    }

    for event in &events {
        let did_mutate_text = match event {
            // First handle events that only changes the selection cursor, not the text:
            event if cursor_range.on_event(os, event, galley, id) => None,

            Event::Copy => {
                if cursor_range.is_empty() {
                    None
                } else {
                    copy_if_not_password(ui, cursor_range.slice_str(text.as_str()).to_owned());
                    None
                }
            }
            Event::Cut => {
                if cursor_range.is_empty() {
                    None
                } else {
                    copy_if_not_password(ui, cursor_range.slice_str(text.as_str()).to_owned());
                    Some(CCursorRange::one(text.delete_selected(&cursor_range)))
                }
            }
            Event::Paste(text_to_insert) => {
                if !text_to_insert.is_empty() {
                    let mut ccursor = text.delete_selected(&cursor_range);

                    text.insert_text_at(&mut ccursor, text_to_insert, char_limit);

                    Some(CCursorRange::one(ccursor))
                } else {
                    None
                }
            }
            Event::Text(text_to_insert) => {
                // Newlines are handled by `Key::Enter`.
                if !text_to_insert.is_empty() && text_to_insert != "\n" && text_to_insert != "\r" {
                    let mut ccursor = text.delete_selected(&cursor_range);

                    text.insert_text_at(&mut ccursor, text_to_insert, char_limit);

                    Some(CCursorRange::one(ccursor))
                } else {
                    None
                }
            }
            Event::Key {
                key: Key::Tab,
                pressed: true,
                modifiers,
                ..
            } if multiline => {
                let mut ccursor = text.delete_selected(&cursor_range);
                if modifiers.shift {
                    // TODO(emilk): support removing indentation over a selection?
                    text.decrease_indentation(&mut ccursor);
                } else {
                    text.insert_text_at(&mut ccursor, "\t", char_limit);
                }
                Some(CCursorRange::one(ccursor))
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if return_key.is_some_and(|return_key| {
                *key == return_key.logical_key && modifiers.matches_logically(return_key.modifiers)
            }) =>
            {
                if multiline {
                    let mut ccursor = text.delete_selected(&cursor_range);
                    text.insert_text_at(&mut ccursor, "\n", char_limit);
                    // TODO(emilk): if code editor, auto-indent by same leading tabs, + one if the lines end on an opening bracket
                    Some(CCursorRange::one(ccursor))
                } else {
                    ui.memory_mut(|mem| mem.surrender_focus(id)); // End input with enter
                    break;
                }
            }

            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if (modifiers.matches_logically(Modifiers::COMMAND) && *key == Key::Y)
                || (modifiers.matches_logically(Modifiers::SHIFT | Modifiers::COMMAND)
                    && *key == Key::Z) =>
            {
                if let Some((redo_ccursor_range, redo_txt)) = state
                    .undoer
                    .lock()
                    .redo(&(cursor_range, text.as_str().to_owned()))
                {
                    text.replace_with(redo_txt);
                    Some(*redo_ccursor_range)
                } else {
                    None
                }
            }

            Event::Key {
                key: Key::Z,
                pressed: true,
                modifiers,
                ..
            } if modifiers.matches_logically(Modifiers::COMMAND) => {
                if let Some((undo_ccursor_range, undo_txt)) = state
                    .undoer
                    .lock()
                    .undo(&(cursor_range, text.as_str().to_owned()))
                {
                    text.replace_with(undo_txt);
                    Some(*undo_ccursor_range)
                } else {
                    None
                }
            }

            Event::Key {
                modifiers,
                key,
                pressed: true,
                ..
            } => check_for_mutating_key_press(os, &cursor_range, text, galley, modifiers, *key),

            Event::Ime(ime_event) => match ime_event {
                ImeEvent::Enabled => {
                    state.ime_enabled = true;
                    state.ime_cursor_range = cursor_range;
                    None
                }
                ImeEvent::Preedit(text_mark) => {
                    if text_mark == "\n" || text_mark == "\r" {
                        None
                    } else {
                        // Empty prediction can be produced when user press backspace
                        // or escape during IME, so we clear current text.
                        let mut ccursor = text.delete_selected(&cursor_range);
                        let start_cursor = ccursor;
                        if !text_mark.is_empty() {
                            text.insert_text_at(&mut ccursor, text_mark, char_limit);
                        }
                        state.ime_cursor_range = cursor_range;
                        Some(CCursorRange::two(start_cursor, ccursor))
                    }
                }
                ImeEvent::Commit(prediction) => {
                    if prediction == "\n" || prediction == "\r" {
                        None
                    } else {
                        state.ime_enabled = false;

                        if !prediction.is_empty()
                            && cursor_range.secondary.index
                                == state.ime_cursor_range.secondary.index
                        {
                            let mut ccursor = text.delete_selected(&cursor_range);
                            text.insert_text_at(&mut ccursor, prediction, char_limit);
                            Some(CCursorRange::one(ccursor))
                        } else {
                            let ccursor = cursor_range.primary;
                            Some(CCursorRange::one(ccursor))
                        }
                    }
                }
                ImeEvent::Disabled => {
                    state.ime_enabled = false;
                    None
                }
            },

            _ => None,
        };

        if let Some(new_ccursor_range) = did_mutate_text {
            any_change = true;

            // Layout again to avoid frame delay, and to keep `text` and `galley` in sync.
            *galley = layouter(ui, text, wrap_width);

            // Set cursor_range using new galley:
            cursor_range = new_ccursor_range;
        }
    }

    state.cursor.set_char_range(Some(cursor_range));

    state.undoer.lock().feed_state(
        ui.input(|i| i.time),
        &(cursor_range, text.as_str().to_owned()),
    );

    (any_change, cursor_range)
}

// ----------------------------------------------------------------------------

fn remove_ime_incompatible_events(events: &mut Vec<Event>) {
    // Remove key events which cause problems while 'IME' is being used.
    // See https://github.com/emilk/egui/pull/4509
    events.retain(|event| {
        !matches!(
            event,
            Event::Key { repeat: true, .. }
                | Event::Key {
                    key: Key::Backspace
                        | Key::ArrowUp
                        | Key::ArrowDown
                        | Key::ArrowLeft
                        | Key::ArrowRight,
                    ..
                }
        )
    });
}

// ----------------------------------------------------------------------------

/// Returns `Some(new_cursor)` if we did mutate `text`.
fn check_for_mutating_key_press(
    os: OperatingSystem,
    cursor_range: &CCursorRange,
    text: &mut dyn TextBuffer,
    galley: &Galley,
    modifiers: &Modifiers,
    key: Key,
) -> Option<CCursorRange> {
    match key {
        Key::Backspace => {
            let ccursor = if modifiers.mac_cmd {
                text.delete_paragraph_before_cursor(galley, cursor_range)
            } else if let Some(cursor) = cursor_range.single() {
                if modifiers.alt || modifiers.ctrl {
                    // alt on mac, ctrl on windows
                    text.delete_previous_word(cursor)
                } else {
                    text.delete_previous_char(cursor)
                }
            } else {
                text.delete_selected(cursor_range)
            };
            Some(CCursorRange::one(ccursor))
        }

        Key::Delete if !modifiers.shift || os != OperatingSystem::Windows => {
            let ccursor = if modifiers.mac_cmd {
                text.delete_paragraph_after_cursor(galley, cursor_range)
            } else if let Some(cursor) = cursor_range.single() {
                if modifiers.alt || modifiers.ctrl {
                    // alt on mac, ctrl on windows
                    text.delete_next_word(cursor)
                } else {
                    text.delete_next_char(cursor)
                }
            } else {
                text.delete_selected(cursor_range)
            };
            let ccursor = CCursor {
                prefer_next_row: true,
                ..ccursor
            };
            Some(CCursorRange::one(ccursor))
        }

        Key::H if modifiers.ctrl => {
            let ccursor = text.delete_previous_char(cursor_range.primary);
            Some(CCursorRange::one(ccursor))
        }

        Key::K if modifiers.ctrl => {
            let ccursor = text.delete_paragraph_after_cursor(galley, cursor_range);
            Some(CCursorRange::one(ccursor))
        }

        Key::U if modifiers.ctrl => {
            let ccursor = text.delete_paragraph_before_cursor(galley, cursor_range);
            Some(CCursorRange::one(ccursor))
        }

        Key::W if modifiers.ctrl => {
            let ccursor = if let Some(cursor) = cursor_range.single() {
                text.delete_previous_word(cursor)
            } else {
                text.delete_selected(cursor_range)
            };
            Some(CCursorRange::one(ccursor))
        }

        _ => None,
    }
}
