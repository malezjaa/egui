use std::{any::Any, sync::Arc};

use crate::{
    Context, CursorIcon, Id, LayerId, PointerButton, Popup, PopupKind, Sense, Tooltip, Ui,
    WidgetRect, WidgetText,
    emath::{Align, Pos2, Rect, Vec2},
    pass_state,
};
// ----------------------------------------------------------------------------

/// The result of adding a widget to a [`Ui`].
///
/// A [`Response`] lets you know whether a widget is being hovered, clicked or dragged.
/// It also lets you easily show a tooltip on hover.
///
/// Whenever something gets added to a [`Ui`], a [`Response`] object is returned.
/// [`ui.add`] returns a [`Response`], as does [`ui.button`], and all similar shortcuts.
///
/// ⚠️ The `Response` contains a clone of [`Context`], and many methods lock the `Context`.
/// It can therefore be a deadlock to use `Context` from within a context-locking closures,
/// such as [`Context::input`].
#[derive(Clone, Debug)]
pub struct Response {
    // CONTEXT:
    /// Used for optionally showing a tooltip and checking for more interactions.
    pub ctx: Context,

    // IN:
    /// Which layer the widget is part of.
    pub layer_id: LayerId,

    /// The [`Id`] of the widget/area this response pertains.
    pub id: Id,

    /// The area of the screen we are talking about.
    pub rect: Rect,

    /// The rectangle sensing interaction.
    ///
    /// This is sometimes smaller than [`Self::rect`] because of clipping
    /// (e.g. when inside a scroll area).
    pub interact_rect: Rect,

    /// The senses (click and/or drag) that the widget was interested in (if any).
    ///
    /// Note: if [`Self::enabled`] is `false`, then
    /// the widget _effectively_ doesn't sense anything,
    /// but can still have the same `Sense`.
    /// This is because the sense informs the styling of the widget,
    /// but we don't want to change the style when a widget is disabled
    /// (that is handled by the `Painter` directly).
    pub sense: Sense,

    // OUT:
    /// Where the pointer (mouse/touch) were when this widget was clicked or dragged.
    /// `None` if the widget is not being interacted with.
    #[doc(hidden)]
    pub interact_pointer_pos: Option<Pos2>,

    /// The intrinsic / desired size of the widget.
    ///
    /// This is the size that a non-wrapped, non-truncated, non-justified version of the widget
    /// would have.
    ///
    /// If this is `None`, use [`Self::rect`] instead.
    ///
    /// At the time of writing, this is only used by external crates
    /// for improved layouting.
    /// See for instance [`egui_flex`](https://github.com/lucasmerlin/hello_egui/tree/main/crates/egui_flex).
    pub intrinsic_size: Option<Vec2>,

    #[doc(hidden)]
    pub flags: Flags,
}

/// A bit set for various boolean properties of `Response`.
#[doc(hidden)]
#[derive(Copy, Clone, Debug)]
pub struct Flags(u16);

bitflags::bitflags! {
    impl Flags: u16 {
        /// Was the widget enabled?
        /// If `false`, there was no interaction attempted (not even hover).
        const ENABLED = 1<<0;

        /// The pointer is above this widget with no other blocking it.
        const CONTAINS_POINTER = 1<<1;

        /// The pointer is hovering above this widget or the widget was clicked/tapped this frame.
        const HOVERED = 1<<2;

        /// The widget is highlighted via a call to [`Response::highlight`] or
        /// [`Context::highlight_widget`].
        const HIGHLIGHTED = 1<<3;

        /// This widget was clicked this frame.
        ///
        /// Which pointer and how many times we don't know,
        /// and ask [`crate::InputState`] about at runtime.
        ///
        /// This is only set to true if the widget was clicked
        /// by an actual mouse.
        const CLICKED = 1<<4;

        /// This widget should act as if clicked due
        /// to something else than a click.
        ///
        /// This is set to true if the widget has keyboard focus and
        /// the user hit the Space or Enter key.
        const FAKE_PRIMARY_CLICKED = 1<<5;

        /// This widget was long-pressed on a touch screen to simulate a secondary click.
        const LONG_TOUCHED = 1<<6;

        /// The widget started being dragged this frame.
        const DRAG_STARTED = 1<<7;

        /// The widget is being dragged.
        const DRAGGED = 1<<8;

        /// The widget was being dragged, but now it has been released.
        const DRAG_STOPPED = 1<<9;

        /// Is the pointer button currently down on this widget?
        /// This is true if the pointer is pressing down or dragging a widget
        const IS_POINTER_BUTTON_DOWN_ON = 1<<10;

        /// Was the underlying data changed?
        ///
        /// e.g. the slider was dragged, text was entered in a [`TextEdit`](crate::TextEdit) etc.
        /// Always `false` for something like a [`Button`](crate::Button).
        ///
        /// Note that this can be `true` even if the user did not interact with the widget,
        /// for instance if an existing slider value was clamped to the given range.
        const CHANGED = 1<<11;

        /// Should this container be closed?
        const CLOSE = 1<<12;
    }
}

impl Response {
    /// Returns true if this widget was clicked this frame by the primary button.
    ///
    /// A click is registered when the mouse or touch is released within
    /// a certain amount of time and distance from when and where it was pressed.
    ///
    /// This will also return true if the widget was clicked via accessibility integration,
    /// or if the widget had keyboard focus and the use pressed Space/Enter.
    ///
    /// Note that the widget must be sensing clicks with [`Sense::click`].
    /// [`crate::Button`] senses clicks; [`crate::Label`] does not (unless you call [`crate::Label::sense`]).
    ///
    /// You can use [`Self::interact`] to sense more things *after* adding a widget.
    #[inline(always)]
    pub fn clicked(&self) -> bool {
        self.flags.contains(Flags::FAKE_PRIMARY_CLICKED) || self.clicked_by(PointerButton::Primary)
    }

    /// Returns true if this widget was clicked this frame by the given mouse button.
    ///
    /// This will NOT return true if the widget was "clicked" via
    /// some accessibility integration, or if the widget had keyboard focus and the
    /// user pressed Space/Enter. For that, use [`Self::clicked`] instead.
    ///
    /// This will likewise ignore the press-and-hold action on touch screens.
    /// Use [`Self::secondary_clicked`] instead to also detect that.
    #[inline]
    pub fn clicked_by(&self, button: PointerButton) -> bool {
        self.flags.contains(Flags::CLICKED) && self.ctx.input(|i| i.pointer.button_clicked(button))
    }

    /// Returns true if this widget was clicked this frame by the secondary mouse button (e.g. the right mouse button).
    ///
    /// This also returns true if the widget was pressed-and-held on a touch screen.
    #[inline]
    pub fn secondary_clicked(&self) -> bool {
        self.flags.contains(Flags::LONG_TOUCHED) || self.clicked_by(PointerButton::Secondary)
    }

    /// Was this long-pressed on a touch screen?
    ///
    /// Usually you want to check [`Self::secondary_clicked`] instead.
    #[inline]
    pub fn long_touched(&self) -> bool {
        self.flags.contains(Flags::LONG_TOUCHED)
    }

    /// Returns true if this widget was clicked this frame by the middle mouse button.
    #[inline]
    pub fn middle_clicked(&self) -> bool {
        self.clicked_by(PointerButton::Middle)
    }

    /// Returns true if this widget was double-clicked this frame by the primary button.
    #[inline]
    pub fn double_clicked(&self) -> bool {
        self.double_clicked_by(PointerButton::Primary)
    }

    /// Returns true if this widget was triple-clicked this frame by the primary button.
    #[inline]
    pub fn triple_clicked(&self) -> bool {
        self.triple_clicked_by(PointerButton::Primary)
    }

    /// Returns true if this widget was double-clicked this frame by the given button.
    #[inline]
    pub fn double_clicked_by(&self, button: PointerButton) -> bool {
        self.flags.contains(Flags::CLICKED)
            && self.ctx.input(|i| i.pointer.button_double_clicked(button))
    }

    /// Returns true if this widget was triple-clicked this frame by the given button.
    #[inline]
    pub fn triple_clicked_by(&self, button: PointerButton) -> bool {
        self.flags.contains(Flags::CLICKED)
            && self.ctx.input(|i| i.pointer.button_triple_clicked(button))
    }

    /// Was this widget middle-clicked or clicked while holding down a modifier key?
    ///
    /// This is used by [`crate::Hyperlink`] to check if a URL should be opened
    /// in a new tab, using [`crate::OpenUrl::new_tab`].
    pub fn clicked_with_open_in_background(&self) -> bool {
        self.middle_clicked() || self.clicked() && self.ctx.input(|i| i.modifiers.any())
    }

    /// `true` if there was a click *outside* the rect of this widget.
    ///
    /// Clicks on widgets contained in this one counts as clicks inside this widget,
    /// so that clicking a button in an area will not be considered as clicking "elsewhere" from the area.
    ///
    /// Clicks on other layers above this widget *will* be considered as clicking elsewhere.
    pub fn clicked_elsewhere(&self) -> bool {
        let (pointer_interact_pos, any_click) = self
            .ctx
            .input(|i| (i.pointer.interact_pos(), i.pointer.any_click()));

        // We do not use self.clicked(), because we want to catch all clicks within our frame,
        // even if we aren't clickable (or even enabled).
        // This is important for windows and such that should close then the user clicks elsewhere.
        if any_click {
            if self.contains_pointer() || self.hovered() {
                false
            } else if let Some(pos) = pointer_interact_pos {
                let layer_under_pointer = self.ctx.layer_id_at(pos);
                if layer_under_pointer != Some(self.layer_id) {
                    true
                } else {
                    !self.interact_rect.contains(pos)
                }
            } else {
                false // clicked without a pointer, weird
            }
        } else {
            false
        }
    }

    /// Was the widget enabled?
    /// If false, there was no interaction attempted
    /// and the widget should be drawn in a gray disabled look.
    #[inline(always)]
    pub fn enabled(&self) -> bool {
        self.flags.contains(Flags::ENABLED)
    }

    /// The pointer is hovering above this widget or the widget was clicked/tapped this frame.
    ///
    /// In contrast to [`Self::contains_pointer`], this will be `false` whenever some other widget is being dragged.
    /// `hovered` is always `false` for disabled widgets.
    #[inline(always)]
    pub fn hovered(&self) -> bool {
        self.flags.contains(Flags::HOVERED)
    }

    /// Returns true if the pointer is contained by the response rect, and no other widget is covering it.
    ///
    /// In contrast to [`Self::hovered`], this can be `true` even if some other widget is being dragged.
    /// This means it is useful for styling things like drag-and-drop targets.
    /// `contains_pointer` can also be `true` for disabled widgets.
    ///
    /// This is slightly different from [`Ui::rect_contains_pointer`] and [`Context::rect_contains_pointer`], in that
    /// [`Self::contains_pointer`] also checks that no other widget is covering this response rectangle.
    #[inline(always)]
    pub fn contains_pointer(&self) -> bool {
        self.flags.contains(Flags::CONTAINS_POINTER)
    }

    /// The widget is highlighted via a call to [`Self::highlight`] or [`Context::highlight_widget`].
    #[doc(hidden)]
    #[inline(always)]
    pub fn highlighted(&self) -> bool {
        self.flags.contains(Flags::HIGHLIGHTED)
    }

    /// This widget has the keyboard focus (i.e. is receiving key presses).
    ///
    /// This function only returns true if the UI as a whole (e.g. window)
    /// also has the keyboard focus. That makes this function suitable
    /// for style choices, e.g. a thicker border around focused widgets.
    pub fn has_focus(&self) -> bool {
        self.ctx.input(|i| i.focused) && self.ctx.memory(|mem| mem.has_focus(self.id))
    }

    /// True if this widget has keyboard focus this frame, but didn't last frame.
    pub fn gained_focus(&self) -> bool {
        self.ctx.memory(|mem| mem.gained_focus(self.id))
    }

    /// The widget had keyboard focus and lost it,
    /// either because the user pressed tab or clicked somewhere else,
    /// or (in case of a [`crate::TextEdit`]) because the user pressed enter.
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut my_text = String::new();
    /// # fn do_request(_: &str) {}
    /// let response = ui.text_edit_singleline(&mut my_text);
    /// if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
    ///     do_request(&my_text);
    /// }
    /// # });
    /// ```
    pub fn lost_focus(&self) -> bool {
        self.ctx.memory(|mem| mem.lost_focus(self.id))
    }

    /// Request that this widget get keyboard focus.
    pub fn request_focus(&self) {
        self.ctx.memory_mut(|mem| mem.request_focus(self.id));
    }

    /// Surrender keyboard focus for this widget.
    pub fn surrender_focus(&self) {
        self.ctx.memory_mut(|mem| mem.surrender_focus(self.id));
    }

    /// Did a drag on this widget begin this frame?
    ///
    /// This is only true if the widget sense drags.
    /// If the widget also senses clicks, this will only become true if the pointer has moved a bit.
    ///
    /// This will only be true for a single frame.
    #[inline]
    pub fn drag_started(&self) -> bool {
        self.flags.contains(Flags::DRAG_STARTED)
    }

    /// Did a drag on this widget by the button begin this frame?
    ///
    /// This is only true if the widget sense drags.
    /// If the widget also senses clicks, this will only become true if the pointer has moved a bit.
    ///
    /// This will only be true for a single frame.
    #[inline]
    pub fn drag_started_by(&self, button: PointerButton) -> bool {
        self.drag_started() && self.ctx.input(|i| i.pointer.button_down(button))
    }

    /// The widget is being dragged.
    ///
    /// To find out which button(s), use [`Self::dragged_by`].
    ///
    /// If the widget is only sensitive to drags, this is `true` as soon as the pointer presses down on it.
    /// If the widget also senses clicks, this won't be true until the pointer has moved a bit,
    /// or the user has pressed down for long enough.
    /// See [`crate::input_state::PointerState::is_decidedly_dragging`] for details.
    ///
    /// If you want to avoid the delay, use [`Self::is_pointer_button_down_on`] instead.
    ///
    /// If the widget is NOT sensitive to drags, this will always be `false`.
    /// [`crate::DragValue`] senses drags; [`crate::Label`] does not (unless you call [`crate::Label::sense`]).
    /// You can use [`Self::interact`] to sense more things *after* adding a widget.
    #[inline(always)]
    pub fn dragged(&self) -> bool {
        self.flags.contains(Flags::DRAGGED)
    }

    /// See [`Self::dragged`].
    #[inline]
    pub fn dragged_by(&self, button: PointerButton) -> bool {
        self.dragged() && self.ctx.input(|i| i.pointer.button_down(button))
    }

    /// The widget was being dragged, but now it has been released.
    #[inline]
    pub fn drag_stopped(&self) -> bool {
        self.flags.contains(Flags::DRAG_STOPPED)
    }

    /// The widget was being dragged by the button, but now it has been released.
    pub fn drag_stopped_by(&self, button: PointerButton) -> bool {
        self.drag_stopped() && self.ctx.input(|i| i.pointer.button_released(button))
    }

    /// If dragged, how many points were we dragged and in what direction?
    #[inline]
    pub fn drag_delta(&self) -> Vec2 {
        if self.dragged() {
            let mut delta = self.ctx.input(|i| i.pointer.delta());
            if let Some(from_global) = self.ctx.layer_transform_from_global(self.layer_id) {
                delta *= from_global.scaling;
            }
            delta
        } else {
            Vec2::ZERO
        }
    }

    /// If dragged, how far did the mouse move?
    /// This will use raw mouse movement if provided by the integration, otherwise will fall back to [`Response::drag_delta`]
    /// Raw mouse movement is unaccelerated and unclamped by screen boundaries, and does not relate to any position on the screen.
    /// This may be useful in certain situations such as draggable values and 3D cameras, where screen position does not matter.
    #[inline]
    pub fn drag_motion(&self) -> Vec2 {
        if self.dragged() {
            self.ctx
                .input(|i| i.pointer.motion().unwrap_or(i.pointer.delta()))
        } else {
            Vec2::ZERO
        }
    }

    /// If the user started dragging this widget this frame, store the payload for drag-and-drop.
    #[doc(alias = "drag and drop")]
    pub fn dnd_set_drag_payload<Payload: Any + Send + Sync>(&self, payload: Payload) {
        if self.drag_started() {
            crate::DragAndDrop::set_payload(&self.ctx, payload);
        }

        if self.hovered() && !self.sense.senses_click() {
            // Things that can be drag-dropped should use the Grab cursor icon,
            // but if the thing is _also_ clickable, that can be annoying.
            self.ctx.set_cursor_icon(CursorIcon::Grab);
        }
    }

    /// Drag-and-Drop: Return what is being held over this widget, if any.
    ///
    /// Only returns something if [`Self::contains_pointer`] is true,
    /// and the user is drag-dropping something of this type.
    #[doc(alias = "drag and drop")]
    pub fn dnd_hover_payload<Payload: Any + Send + Sync>(&self) -> Option<Arc<Payload>> {
        // NOTE: we use `response.contains_pointer` here instead of `hovered`, because
        // `hovered` is always false when another widget is being dragged.
        if self.contains_pointer() {
            crate::DragAndDrop::payload::<Payload>(&self.ctx)
        } else {
            None
        }
    }

    /// Drag-and-Drop: Return what is being dropped onto this widget, if any.
    ///
    /// Only returns something if [`Self::contains_pointer`] is true,
    /// the user is drag-dropping something of this type,
    /// and they released it this frame
    #[doc(alias = "drag and drop")]
    pub fn dnd_release_payload<Payload: Any + Send + Sync>(&self) -> Option<Arc<Payload>> {
        // NOTE: we use `response.contains_pointer` here instead of `hovered`, because
        // `hovered` is always false when another widget is being dragged.
        if self.contains_pointer() && self.ctx.input(|i| i.pointer.any_released()) {
            crate::DragAndDrop::take_payload::<Payload>(&self.ctx)
        } else {
            None
        }
    }

    /// Where the pointer (mouse/touch) were when this widget was clicked or dragged.
    ///
    /// `None` if the widget is not being interacted with.
    #[inline]
    pub fn interact_pointer_pos(&self) -> Option<Pos2> {
        self.interact_pointer_pos
    }

    /// If it is a good idea to show a tooltip, where is pointer?
    ///
    /// None if the pointer is outside the response area.
    #[inline]
    pub fn hover_pos(&self) -> Option<Pos2> {
        if self.hovered() {
            let mut pos = self.ctx.input(|i| i.pointer.hover_pos())?;
            if let Some(from_global) = self.ctx.layer_transform_from_global(self.layer_id) {
                pos = from_global * pos;
            }
            Some(pos)
        } else {
            None
        }
    }

    /// Is the pointer button currently down on this widget?
    ///
    /// This is true if the pointer is pressing down or dragging a widget,
    /// even when dragging outside the widget.
    ///
    /// This could also be thought of as "is this widget being interacted with?".
    #[inline(always)]
    pub fn is_pointer_button_down_on(&self) -> bool {
        self.flags.contains(Flags::IS_POINTER_BUTTON_DOWN_ON)
    }

    /// Was the underlying data changed?
    ///
    /// e.g. the slider was dragged, text was entered in a [`TextEdit`](crate::TextEdit) etc.
    /// Always `false` for something like a [`Button`](crate::Button).
    ///
    /// Can sometimes be `true` even though the data didn't changed
    /// (e.g. if the user entered a character and erased it the same frame).
    ///
    /// This is not set if the *view* of the data was changed.
    /// For instance, moving the cursor in a [`TextEdit`](crate::TextEdit) does not set this to `true`.
    ///
    /// Note that this can be `true` even if the user did not interact with the widget,
    /// for instance if an existing slider value was clamped to the given range.
    #[inline(always)]
    pub fn changed(&self) -> bool {
        self.flags.contains(Flags::CHANGED)
    }

    /// Report the data shown by this widget changed.
    ///
    /// This must be called by widgets that represent some mutable data,
    /// e.g. checkboxes, sliders etc.
    ///
    /// This should be called when the *content* changes, but not when the view does.
    /// So we call this when the text of a [`crate::TextEdit`], but not when the cursor changes.
    #[inline(always)]
    pub fn mark_changed(&mut self) {
        self.flags.set(Flags::CHANGED, true);
    }

    /// Should the container be closed?
    ///
    /// Will e.g. be set by calling [`Ui::close`] in a child [`Ui`] or by calling
    /// [`Self::set_close`].
    pub fn should_close(&self) -> bool {
        self.flags.contains(Flags::CLOSE)
    }

    /// Set the [`Flags::CLOSE`] flag.
    ///
    /// Can be used to e.g. signal that a container should be closed.
    pub fn set_close(&mut self) {
        self.flags.set(Flags::CLOSE, true);
    }

    /// Show this UI if the widget was hovered (i.e. a tooltip).
    ///
    /// The text will not be visible if the widget is not enabled.
    /// For that, use [`Self::on_disabled_hover_ui`] instead.
    ///
    /// If you call this multiple times the tooltips will stack underneath the previous ones.
    ///
    /// The widget can contain interactive widgets, such as buttons and links.
    /// If so, it will stay open as the user moves their pointer over it.
    /// By default, the text of a tooltip is NOT selectable (i.e. interactive),
    /// but you can change this by setting [`style::Interaction::selectable_labels` from within the tooltip:
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// ui.label("Hover me").on_hover_ui(|ui| {
    ///     ui.style_mut().interaction.selectable_labels = true;
    ///     ui.label("This text can be selected");
    /// });
    /// # });
    /// ```
    #[doc(alias = "tooltip")]
    pub fn on_hover_ui(self, add_contents: impl FnOnce(&mut Ui)) -> Self {
        Tooltip::for_enabled(&self).show(add_contents);
        self
    }

    /// Show this UI when hovering if the widget is disabled.
    pub fn on_disabled_hover_ui(self, add_contents: impl FnOnce(&mut Ui)) -> Self {
        Tooltip::for_disabled(&self).show(add_contents);
        self
    }

    /// Like `on_hover_ui`, but show the ui next to cursor.
    pub fn on_hover_ui_at_pointer(self, add_contents: impl FnOnce(&mut Ui)) -> Self {
        Tooltip::for_enabled(&self)
            .at_pointer()
            .gap(12.0)
            .show(add_contents);
        self
    }

    /// Always show this tooltip, even if disabled and the user isn't hovering it.
    ///
    /// This can be used to give attention to a widget during a tutorial.
    pub fn show_tooltip_ui(&self, add_contents: impl FnOnce(&mut Ui)) {
        Popup::from_response(self)
            .kind(PopupKind::Tooltip)
            .show(add_contents);
    }

    /// Always show this tooltip, even if disabled and the user isn't hovering it.
    ///
    /// This can be used to give attention to a widget during a tutorial.
    pub fn show_tooltip_text(&self, text: impl Into<WidgetText>) {
        self.show_tooltip_ui(|ui| {
            ui.label(text);
        });
    }

    /// Was the tooltip open last frame?
    pub fn is_tooltip_open(&self) -> bool {
        Tooltip::was_tooltip_open_last_frame(&self.ctx, self.id)
    }

    /// Like `on_hover_text`, but show the text next to cursor.
    #[doc(alias = "tooltip")]
    pub fn on_hover_text_at_pointer(self, text: impl Into<WidgetText>) -> Self {
        self.on_hover_ui_at_pointer(|ui| {
            // Prevent `Area` auto-sizing from shrinking tooltips with dynamic content.
            // See https://github.com/emilk/egui/issues/5167
            ui.set_max_width(ui.spacing().tooltip_width);

            ui.add(crate::widgets::Label::new(text));
        })
    }

    /// Show this text if the widget was hovered (i.e. a tooltip).
    ///
    /// The text will not be visible if the widget is not enabled.
    /// For that, use [`Self::on_disabled_hover_text`] instead.
    ///
    /// If you call this multiple times the tooltips will stack underneath the previous ones.
    #[doc(alias = "tooltip")]
    pub fn on_hover_text(self, text: impl Into<WidgetText>) -> Self {
        self.on_hover_ui(|ui| {
            // Prevent `Area` auto-sizing from shrinking tooltips with dynamic content.
            // See https://github.com/emilk/egui/issues/5167
            ui.set_max_width(ui.spacing().tooltip_width);

            ui.add(crate::widgets::Label::new(text));
        })
    }

    /// Highlight this widget, to make it look like it is hovered, even if it isn't.
    ///
    /// The highlight takes one frame to take effect if you call this after the widget has been fully rendered.
    ///
    /// See also [`Context::highlight_widget`].
    #[inline]
    pub fn highlight(mut self) -> Self {
        self.ctx.highlight_widget(self.id);
        self.flags.set(Flags::HIGHLIGHTED, true);
        self
    }

    /// Show this text when hovering if the widget is disabled.
    pub fn on_disabled_hover_text(self, text: impl Into<WidgetText>) -> Self {
        self.on_disabled_hover_ui(|ui| {
            // Prevent `Area` auto-sizing from shrinking tooltips with dynamic content.
            // See https://github.com/emilk/egui/issues/5167
            ui.set_max_width(ui.spacing().tooltip_width);

            ui.add(crate::widgets::Label::new(text));
        })
    }

    /// When hovered, use this icon for the mouse cursor.
    #[inline]
    pub fn on_hover_cursor(self, cursor: CursorIcon) -> Self {
        if self.hovered() {
            self.ctx.set_cursor_icon(cursor);
        }
        self
    }

    /// When hovered or dragged, use this icon for the mouse cursor.
    #[inline]
    pub fn on_hover_and_drag_cursor(self, cursor: CursorIcon) -> Self {
        if self.hovered() || self.dragged() {
            self.ctx.set_cursor_icon(cursor);
        }
        self
    }

    /// Sense more interactions (e.g. sense clicks on a [`Response`] returned from a label).
    ///
    /// The interaction will occur on the same plane as the original widget,
    /// i.e. if the response was from a widget behind button, the interaction will also be behind that button.
    /// egui gives priority to the _last_ added widget (the one on top gets clicked first).
    ///
    /// Note that this call will not add any hover-effects to the widget, so when possible
    /// it is better to give the widget a [`Sense`] instead, e.g. using [`crate::Label::sense`].
    ///
    /// Using this method on a `Response` that is the result of calling `union` on multiple `Response`s
    /// is undefined behavior.
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// let horiz_response = ui.horizontal(|ui| {
    ///     ui.label("hello");
    /// }).response;
    /// assert!(!horiz_response.clicked()); // ui's don't sense clicks by default
    /// let horiz_response = horiz_response.interact(egui::Sense::click());
    /// if horiz_response.clicked() {
    ///     // The background behind the label was clicked
    /// }
    /// # });
    /// ```
    #[must_use]
    pub fn interact(&self, sense: Sense) -> Self {
        if (self.sense | sense) == self.sense {
            // Early-out: we already sense everything we need to sense.
            return self.clone();
        }

        self.ctx.create_widget(
            WidgetRect {
                layer_id: self.layer_id,
                id: self.id,
                rect: self.rect,
                interact_rect: self.interact_rect,
                sense: self.sense | sense,
                enabled: self.enabled(),
            },
            true,
        )
    }

    /// Adjust the scroll position until this UI becomes visible.
    ///
    /// If `align` is [`Align::TOP`] it means "put the top of the rect at the top of the scroll area", etc.
    /// If `align` is `None`, it'll scroll enough to bring the UI into view.
    ///
    /// See also: [`Ui::scroll_to_cursor`], [`Ui::scroll_to_rect`]. [`Ui::scroll_with_delta`].
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// egui::ScrollArea::vertical().show(ui, |ui| {
    ///     for i in 0..1000 {
    ///         let response = ui.button("Scroll to me");
    ///         if response.clicked() {
    ///             response.scroll_to_me(Some(egui::Align::Center));
    ///         }
    ///     }
    /// });
    /// # });
    /// ```
    pub fn scroll_to_me(&self, align: Option<Align>) {
        self.scroll_to_me_animation(align, self.ctx.style().scroll_animation);
    }

    /// Like [`Self::scroll_to_me`], but allows you to specify the [`crate::style::ScrollAnimation`].
    pub fn scroll_to_me_animation(
        &self,
        align: Option<Align>,
        animation: crate::style::ScrollAnimation,
    ) {
        self.ctx.pass_state_mut(|state| {
            state.scroll_target[0] = Some(pass_state::ScrollTarget::new(
                self.rect.x_range(),
                align,
                animation,
            ));
            state.scroll_target[1] = Some(pass_state::ScrollTarget::new(
                self.rect.y_range(),
                align,
                animation,
            ));
        });
    }

    /// For accessibility.
    ///
    /// Call after interacting and potential calls to [`Self::mark_changed`].
    pub fn widget_info(&self, make_info: impl Fn() -> crate::WidgetInfo) {
        use crate::output::OutputEvent;

        let event = if self.clicked() {
            Some(OutputEvent::Clicked(make_info()))
        } else if self.double_clicked() {
            Some(OutputEvent::DoubleClicked(make_info()))
        } else if self.triple_clicked() {
            Some(OutputEvent::TripleClicked(make_info()))
        } else if self.gained_focus() {
            Some(OutputEvent::FocusGained(make_info()))
        } else if self.changed() {
            Some(OutputEvent::ValueChanged(make_info()))
        } else {
            None
        };

        if let Some(event) = event {
            self.output_event(event);
        } else {
            #[cfg(feature = "accesskit")]
            self.ctx.accesskit_node_builder(self.id, |builder| {
                self.fill_accesskit_node_from_widget_info(builder, make_info());
            });

            self.ctx.register_widget_info(self.id, make_info);
        }
    }

    pub fn output_event(&self, event: crate::output::OutputEvent) {
        #[cfg(feature = "accesskit")]
        self.ctx.accesskit_node_builder(self.id, |builder| {
            self.fill_accesskit_node_from_widget_info(builder, event.widget_info().clone());
        });

        self.ctx
            .register_widget_info(self.id, || event.widget_info().clone());

        self.ctx.output_mut(|o| o.events.push(event));
    }

    #[cfg(feature = "accesskit")]
    pub(crate) fn fill_accesskit_node_common(&self, builder: &mut accesskit::Node) {
        if !self.enabled() {
            builder.set_disabled();
        }
        builder.set_bounds(accesskit::Rect {
            x0: self.rect.min.x.into(),
            y0: self.rect.min.y.into(),
            x1: self.rect.max.x.into(),
            y1: self.rect.max.y.into(),
        });
        if self.sense.is_focusable() {
            builder.add_action(accesskit::Action::Focus);
        }
        if self.sense.senses_click() {
            builder.add_action(accesskit::Action::Click);
        }
    }

    #[cfg(feature = "accesskit")]
    fn fill_accesskit_node_from_widget_info(
        &self,
        builder: &mut accesskit::Node,
        info: crate::WidgetInfo,
    ) {
        use crate::WidgetType;
        use accesskit::{Role, Toggled};

        self.fill_accesskit_node_common(builder);
        builder.set_role(match info.typ {
            WidgetType::Label => Role::Label,
            WidgetType::Link => Role::Link,
            WidgetType::TextEdit => Role::TextInput,
            WidgetType::Button | WidgetType::ImageButton | WidgetType::CollapsingHeader => {
                Role::Button
            }
            WidgetType::Image => Role::Image,
            WidgetType::Checkbox => Role::CheckBox,
            WidgetType::RadioButton => Role::RadioButton,
            WidgetType::RadioGroup => Role::RadioGroup,
            WidgetType::SelectableLabel => Role::Button,
            WidgetType::ComboBox => Role::ComboBox,
            WidgetType::Slider => Role::Slider,
            WidgetType::DragValue => Role::SpinButton,
            WidgetType::ColorButton => Role::ColorWell,
            WidgetType::ProgressIndicator => Role::ProgressIndicator,
            WidgetType::Window => Role::Window,
            WidgetType::Other => Role::Unknown,
        });
        if !info.enabled {
            builder.set_disabled();
        }
        if let Some(label) = info.label {
            if matches!(builder.role(), Role::Label) {
                builder.set_value(label);
            } else {
                builder.set_label(label);
            }
        }
        if let Some(value) = info.current_text_value {
            builder.set_value(value);
        }
        if let Some(value) = info.value {
            builder.set_numeric_value(value);
        }
        if let Some(selected) = info.selected {
            builder.set_toggled(if selected {
                Toggled::True
            } else {
                Toggled::False
            });
        } else if matches!(info.typ, WidgetType::Checkbox) {
            // Indeterminate state
            builder.set_toggled(Toggled::Mixed);
        }
        if let Some(hint_text) = info.hint_text {
            builder.set_placeholder(hint_text);
        }
    }

    /// Associate a label with a control for accessibility.
    ///
    /// # Example
    ///
    /// ```
    /// # egui::__run_test_ui(|ui| {
    /// # let mut text = "Arthur".to_string();
    /// ui.horizontal(|ui| {
    ///     let label = ui.label("Your name: ");
    ///     ui.text_edit_singleline(&mut text).labelled_by(label.id);
    /// });
    /// # });
    /// ```
    pub fn labelled_by(self, id: impl Into<Id>) -> Self {
        #[cfg(feature = "accesskit")]
        self.ctx.accesskit_node_builder(self.id, |builder| {
            builder.push_labelled_by(id.into().accesskit_id());
        });
        #[cfg(not(feature = "accesskit"))]
        {
            let _ = id;
        }

        self
    }

    /// Response to secondary clicks (right-clicks) by showing the given menu.
    ///
    /// Make sure the widget senses clicks (e.g. [`crate::Button`] does, [`crate::Label`] does not).
    ///
    /// ```
    /// # use egui::{Label, Sense};
    /// # egui::__run_test_ui(|ui| {
    /// let response = ui.add(Label::new("Right-click me!").sense(Sense::click()));
    /// response.context_menu(|ui| {
    ///     if ui.button("Close the menu").clicked() {
    ///         ui.close();
    ///     }
    /// });
    /// # });
    /// ```
    ///
    /// See also: [`Ui::menu_button`] and [`Ui::close`].
    pub fn context_menu(&self, add_contents: impl FnOnce(&mut Ui)) -> Option<InnerResponse<()>> {
        Popup::context_menu(self).show(add_contents)
    }

    /// Returns whether a context menu is currently open for this widget.
    ///
    /// See [`Self::context_menu`].
    pub fn context_menu_opened(&self) -> bool {
        Popup::context_menu(self).is_open()
    }

    /// Draw a debug rectangle over the response displaying the response's id and whether it is
    /// enabled and/or hovered.
    ///
    /// This function is intended for debugging purpose and can be useful, for example, in case of
    /// widget id instability.
    ///
    /// Color code:
    /// - Blue: Enabled but not hovered
    /// - Green: Enabled and hovered
    /// - Red: Disabled
    pub fn paint_debug_info(&self) {
        self.ctx.debug_painter().debug_rect(
            self.rect,
            if self.hovered() {
                crate::Color32::DARK_GREEN
            } else if self.enabled() {
                crate::Color32::BLUE
            } else {
                crate::Color32::RED
            },
            format!("{:?}", self.id),
        );
    }
}

impl Response {
    /// A logical "or" operation.
    /// For instance `a.union(b).hovered` means "was either a or b hovered?".
    ///
    /// The resulting [`Self::id`] will come from the first (`self`) argument.
    ///
    /// You may not call [`Self::interact`] on the resulting `Response`.
    pub fn union(&self, other: Self) -> Self {
        assert!(
            self.ctx == other.ctx,
            "Responses must be from the same `Context`"
        );
        debug_assert!(
            self.layer_id == other.layer_id,
            "It makes no sense to combine Responses from two different layers"
        );
        Self {
            ctx: other.ctx,
            layer_id: self.layer_id,
            id: self.id,
            rect: self.rect.union(other.rect),
            interact_rect: self.interact_rect.union(other.interact_rect),
            sense: self.sense.union(other.sense),
            flags: self.flags | other.flags,
            interact_pointer_pos: self.interact_pointer_pos.or(other.interact_pointer_pos),
            intrinsic_size: None,
        }
    }
}

impl Response {
    /// Returns a response with a modified [`Self::rect`].
    #[inline]
    pub fn with_new_rect(self, rect: Rect) -> Self {
        Self { rect, ..self }
    }
}

/// See [`Response::union`].
///
/// To summarize the response from many widgets you can use this pattern:
///
/// ```
/// use egui::*;
/// fn draw_vec2(ui: &mut Ui, v: &mut Vec2) -> Response {
///     ui.add(DragValue::new(&mut v.x)) | ui.add(DragValue::new(&mut v.y))
/// }
/// ```
///
/// Now `draw_vec2(ui, foo).hovered` is true if either [`DragValue`](crate::DragValue) were hovered.
impl std::ops::BitOr for Response {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// See [`Response::union`].
///
/// To summarize the response from many widgets you can use this pattern:
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// # let (widget_a, widget_b, widget_c) = (egui::Label::new("a"), egui::Label::new("b"), egui::Label::new("c"));
/// let mut response = ui.add(widget_a);
/// response |= ui.add(widget_b);
/// response |= ui.add(widget_c);
/// if response.hovered() { ui.label("You hovered at least one of the widgets"); }
/// # });
/// ```
impl std::ops::BitOrAssign for Response {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

// ----------------------------------------------------------------------------

/// Returned when we wrap some ui-code and want to return both
/// the results of the inner function and the ui as a whole, e.g.:
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// let inner_resp = ui.horizontal(|ui| {
///     ui.label("Blah blah");
///     42
/// });
/// inner_resp.response.on_hover_text("You hovered the horizontal layout");
/// assert_eq!(inner_resp.inner, 42);
/// # });
/// ```
#[derive(Debug)]
pub struct InnerResponse<R> {
    /// What the user closure returned.
    pub inner: R,

    /// The response of the area.
    pub response: Response,
}

impl<R> InnerResponse<R> {
    #[inline]
    pub fn new(inner: R, response: Response) -> Self {
        Self { inner, response }
    }
}
