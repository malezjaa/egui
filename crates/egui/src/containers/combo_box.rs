use epaint::Shape;

use crate::{
    Align2, Context, Id, InnerResponse, NumExt as _, Painter, Popup, PopupCloseBehavior, Rect,
    Response, ScrollArea, Sense, Stroke, TextStyle, TextWrapMode, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetText, WidgetType, epaint, style::StyleModifier, style::WidgetVisuals, vec2,
};

#[expect(unused_imports)] // Documentation
use crate::style::Spacing;

/// A function that paints the [`ComboBox`] icon
pub type IconPainter = Box<dyn FnOnce(&Ui, Rect, &WidgetVisuals, bool)>;

/// A drop-down selection menu with a descriptive label.
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// # #[derive(Debug, PartialEq, Copy, Clone)]
/// # enum Enum { First, Second, Third }
/// # let mut selected = Enum::First;
/// let before = selected;
/// egui::ComboBox::from_label("Select one!")
///     .selected_text(format!("{:?}", selected))
///     .show_ui(ui, |ui| {
///         ui.selectable_value(&mut selected, Enum::First, "First");
///         ui.selectable_value(&mut selected, Enum::Second, "Second");
///         ui.selectable_value(&mut selected, Enum::Third, "Third");
///     }
/// );
///
/// if selected != before {
///     // Handle selection change
/// }
/// # });
/// ```
#[must_use = "You should call .show*"]
pub struct ComboBox {
    id_salt: Id,
    label: Option<WidgetText>,
    selected_text: WidgetText,
    width: Option<f32>,
    height: Option<f32>,
    icon: Option<IconPainter>,
    wrap_mode: Option<TextWrapMode>,
    close_behavior: Option<PopupCloseBehavior>,
}

impl ComboBox {
    /// Create new [`ComboBox`] with id and label
    pub fn new(id_salt: impl std::hash::Hash, label: impl Into<WidgetText>) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            label: Some(label.into()),
            selected_text: Default::default(),
            width: None,
            height: None,
            icon: None,
            wrap_mode: None,
            close_behavior: None,
        }
    }

    /// Label shown next to the combo box
    pub fn from_label(label: impl Into<WidgetText>) -> Self {
        let label = label.into();
        Self {
            id_salt: Id::new(label.text()),
            label: Some(label),
            selected_text: Default::default(),
            width: None,
            height: None,
            icon: None,
            wrap_mode: None,
            close_behavior: None,
        }
    }

    /// Without label.
    pub fn from_id_salt(id_salt: impl std::hash::Hash) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            label: Default::default(),
            selected_text: Default::default(),
            width: None,
            height: None,
            icon: None,
            wrap_mode: None,
            close_behavior: None,
        }
    }

    /// Without label.
    #[deprecated = "Renamed from_id_salt"]
    pub fn from_id_source(id_salt: impl std::hash::Hash) -> Self {
        Self::from_id_salt(id_salt)
    }

    /// Set the outer width of the button and menu.
    ///
    /// Default is [`Spacing::combo_width`].
    #[inline]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Set the maximum outer height of the menu.
    ///
    /// Default is [`Spacing::combo_height`].
    #[inline]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// What we show as the currently selected value
    #[inline]
    pub fn selected_text(mut self, selected_text: impl Into<WidgetText>) -> Self {
        self.selected_text = selected_text.into();
        self
    }

    /// Use the provided function to render a different [`ComboBox`] icon.
    /// Defaults to a triangle that expands when the cursor is hovering over the [`ComboBox`].
    ///
    /// For example:
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let text = "Selected text";
    /// pub fn filled_triangle(
    ///     ui: &egui::Ui,
    ///     rect: egui::Rect,
    ///     visuals: &egui::style::WidgetVisuals,
    ///     _is_open: bool,
    /// ) {
    ///     let rect = egui::Rect::from_center_size(
    ///         rect.center(),
    ///         egui::vec2(rect.width() * 0.6, rect.height() * 0.4),
    ///     );
    ///     ui.painter().add(egui::Shape::convex_polygon(
    ///         vec![rect.left_top(), rect.right_top(), rect.center_bottom()],
    ///         visuals.fg_stroke.color,
    ///         visuals.fg_stroke,
    ///     ));
    /// }
    ///
    /// egui::ComboBox::from_id_salt("my-combobox")
    ///     .selected_text(text)
    ///     .icon(filled_triangle)
    ///     .show_ui(ui, |_ui| {});
    /// # });
    /// ```
    #[inline]
    pub fn icon(mut self, icon_fn: impl FnOnce(&Ui, Rect, &WidgetVisuals, bool) + 'static) -> Self {
        self.icon = Some(Box::new(icon_fn));
        self
    }

    /// Controls the wrap mode used for the selected text.
    ///
    /// By default, [`Ui::wrap_mode`] will be used, which can be overridden with [`crate::Style::wrap_mode`].
    ///
    /// Note that any `\n` in the text will always produce a new line.
    #[inline]
    pub fn wrap_mode(mut self, wrap_mode: TextWrapMode) -> Self {
        self.wrap_mode = Some(wrap_mode);
        self
    }

    /// Set [`Self::wrap_mode`] to [`TextWrapMode::Wrap`].
    #[inline]
    pub fn wrap(mut self) -> Self {
        self.wrap_mode = Some(TextWrapMode::Wrap);
        self
    }

    /// Set [`Self::wrap_mode`] to [`TextWrapMode::Truncate`].
    #[inline]
    pub fn truncate(mut self) -> Self {
        self.wrap_mode = Some(TextWrapMode::Truncate);
        self
    }

    /// Controls the close behavior for the popup.
    ///
    /// By default, `PopupCloseBehavior::CloseOnClick` will be used.
    #[inline]
    pub fn close_behavior(mut self, close_behavior: PopupCloseBehavior) -> Self {
        self.close_behavior = Some(close_behavior);
        self
    }

    /// Show the combo box, with the given ui code for the menu contents.
    ///
    /// Returns `InnerResponse { inner: None }` if the combo box is closed.
    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        menu_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        self.show_ui_dyn(ui, Box::new(menu_contents))
    }

    fn show_ui_dyn<'c, R>(
        self,
        ui: &mut Ui,
        menu_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
    ) -> InnerResponse<Option<R>> {
        let Self {
            id_salt,
            label,
            selected_text,
            width,
            height,
            icon,
            wrap_mode,
            close_behavior,
        } = self;

        let button_id = ui.make_persistent_id(id_salt);

        ui.horizontal(|ui| {
            let mut ir = combo_box_dyn(
                ui,
                button_id,
                selected_text,
                menu_contents,
                icon,
                wrap_mode,
                close_behavior,
                (width, height),
            );
            if let Some(label) = label {
                ir.response.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), label.text())
                });
                ir.response |= ui.label(label);
            } else {
                ir.response
                    .widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), ""));
            }
            ir
        })
        .inner
    }

    /// Show a list of items with the given selected index.
    ///
    ///
    /// ```
    /// # #[derive(Debug, PartialEq)]
    /// # enum Enum { First, Second, Third }
    /// # let mut selected = Enum::First;
    /// # egui::__run_test_ui(|ui| {
    /// let alternatives = ["a", "b", "c", "d"];
    /// let mut selected = 2;
    /// egui::ComboBox::from_label("Select one!").show_index(
    ///     ui,
    ///     &mut selected,
    ///     alternatives.len(),
    ///     |i| alternatives[i]
    /// );
    /// # });
    /// ```
    pub fn show_index<Text: Into<WidgetText>>(
        self,
        ui: &mut Ui,
        selected: &mut usize,
        len: usize,
        get: impl Fn(usize) -> Text,
    ) -> Response {
        let slf = self.selected_text(get(*selected));

        let mut changed = false;

        let mut response = slf
            .show_ui(ui, |ui| {
                for i in 0..len {
                    if ui.selectable_label(i == *selected, get(i)).clicked() {
                        *selected = i;
                        changed = true;
                    }
                }
            })
            .response;

        if changed {
            response.mark_changed();
        }
        response
    }

    /// Check if the [`ComboBox`] with the given id has its popup menu currently opened.
    pub fn is_open(ctx: &Context, id: impl Into<Id>) -> bool {
        Popup::is_id_open(ctx, Self::widget_to_popup_id(id))
    }

    /// Convert a [`ComboBox`] id to the id used to store it's popup state.
    fn widget_to_popup_id(widget_id: impl Into<Id>) -> Id {
        widget_id.into().with("popup")
    }
}

#[expect(clippy::too_many_arguments)]
fn combo_box_dyn<'c, R>(
    ui: &mut Ui,
    button_id: impl Into<Id>,
    selected_text: WidgetText,
    menu_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
    icon: Option<IconPainter>,
    wrap_mode: Option<TextWrapMode>,
    close_behavior: Option<PopupCloseBehavior>,
    (width, height): (Option<f32>, Option<f32>),
) -> InnerResponse<Option<R>> {
    let button_id = button_id.into();
    let popup_id = ComboBox::widget_to_popup_id(button_id);

    let is_popup_open = Popup::is_id_open(ui.ctx(), popup_id);

    let wrap_mode = wrap_mode.unwrap_or_else(|| ui.wrap_mode());

    let close_behavior = close_behavior.unwrap_or(PopupCloseBehavior::CloseOnClick);

    let margin = ui.spacing().button_padding;
    let button_response = button_frame(ui, button_id, is_popup_open, Sense::click(), |ui| {
        let icon_spacing = ui.spacing().icon_spacing;
        let icon_size = Vec2::splat(ui.spacing().icon_width);

        // The combo box selected text will always have this minimum width.
        // Note: the `ComboBox::width()` if set or `Spacing::combo_width` are considered as the
        // minimum overall width, regardless of the wrap mode.
        let minimum_width = width.unwrap_or_else(|| ui.spacing().combo_width) - 2.0 * margin.x;

        // width against which to lay out the selected text
        let wrap_width = if wrap_mode == TextWrapMode::Extend {
            // Use all the width necessary to display the currently selected value's text.
            f32::INFINITY
        } else {
            // Use the available width, currently selected value's text will be wrapped if exceeds this value.
            ui.available_width() - icon_spacing - icon_size.x
        };

        let galley = selected_text.into_galley(ui, Some(wrap_mode), wrap_width, TextStyle::Button);

        let actual_width = (galley.size().x + icon_spacing + icon_size.x).at_least(minimum_width);
        let actual_height = galley.size().y.max(icon_size.y);

        let (_, rect) = ui.allocate_space(Vec2::new(actual_width, actual_height));
        let button_rect = ui.min_rect().expand2(ui.spacing().button_padding);
        let response = ui.interact(button_rect, button_id, Sense::click());
        // response.active |= is_popup_open;

        if ui.is_rect_visible(rect) {
            let icon_rect = Align2::RIGHT_CENTER.align_size_within_rect(icon_size, rect);
            let visuals = if is_popup_open {
                &ui.visuals().widgets.open
            } else {
                ui.style().interact(&response)
            };

            if let Some(icon) = icon {
                icon(
                    ui,
                    icon_rect.expand(visuals.expansion),
                    visuals,
                    is_popup_open,
                );
            } else {
                paint_default_icon(ui.painter(), icon_rect.expand(visuals.expansion), visuals);
            }

            let text_rect = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), rect);
            ui.painter()
                .galley(text_rect.min, galley, visuals.text_color());
        }
    });

    let height = height.unwrap_or_else(|| ui.spacing().combo_height);

    let inner = Popup::menu(&button_response)
        .id(popup_id)
        .style(StyleModifier::default())
        .width(button_response.rect.width())
        .close_behavior(close_behavior)
        .show(|ui| {
            ui.set_min_width(ui.available_width());

            ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    // Often the button is very narrow, which means this popup
                    // is also very narrow. Having wrapping on would therefore
                    // result in labels that wrap very early.
                    // Instead, we turn it off by default so that the labels
                    // expand the width of the menu.
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    menu_contents(ui)
                })
                .inner
        })
        .map(|r| r.inner);

    InnerResponse {
        inner,
        response: button_response,
    }
}

fn button_frame(
    ui: &mut Ui,
    id: impl Into<Id>,
    is_popup_open: bool,
    sense: Sense,
    add_contents: impl FnOnce(&mut Ui),
) -> Response {
    let where_to_put_background = ui.painter().add(Shape::Noop);

    let margin = ui.spacing().button_padding;
    let interact_size = ui.spacing().interact_size;

    let mut outer_rect = ui.available_rect_before_wrap();
    outer_rect.set_height(outer_rect.height().at_least(interact_size.y));

    let inner_rect = outer_rect.shrink2(margin);
    let mut content_ui = ui.new_child(UiBuilder::new().max_rect(inner_rect));
    add_contents(&mut content_ui);

    let mut outer_rect = content_ui.min_rect().expand2(margin);
    outer_rect.set_height(outer_rect.height().at_least(interact_size.y));

    let response = ui.interact(outer_rect, id, sense);

    if ui.is_rect_visible(outer_rect) {
        let visuals = if is_popup_open {
            &ui.visuals().widgets.open
        } else {
            ui.style().interact(&response)
        };

        ui.painter().set(
            where_to_put_background,
            epaint::RectShape::new(
                outer_rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                epaint::StrokeKind::Inside,
            ),
        );
    }

    ui.advance_cursor_after_rect(outer_rect);

    response
}

fn paint_default_icon(painter: &Painter, rect: Rect, visuals: &WidgetVisuals) {
    let rect = Rect::from_center_size(
        rect.center(),
        vec2(rect.width() * 0.7, rect.height() * 0.45),
    );

    // Downward pointing triangle
    // Previously, we would show an up arrow when we expected the popup to open upwards
    // (due to lack of space below the button), but this could look weird in edge cases, so this
    // feature was removed. (See https://github.com/emilk/egui/pull/5713#issuecomment-2654420245)
    painter.add(Shape::convex_polygon(
        vec![rect.left_top(), rect.right_top(), rect.center_bottom()],
        visuals.fg_stroke.color,
        Stroke::NONE,
    ));
}
