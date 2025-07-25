#![warn(missing_docs)] // Let's keep this file well-documented.` to memory.rs

use std::num::NonZeroUsize;

use ahash::{HashMap, HashSet};
use epaint::emath::TSTransform;

use crate::{
    EventFilter, Id, IdMap, LayerId, Order, Pos2, Rangef, RawInput, Rect, Style, Vec2, ViewportId,
    ViewportIdMap, ViewportIdSet, area, vec2,
};

mod theme;
pub use theme::{Theme, ThemePreference};

// ----------------------------------------------------------------------------

/// The data that egui persists between frames.
///
/// This includes window positions and sizes,
/// how far the user has scrolled in a [`ScrollArea`](crate::ScrollArea) etc.
///
/// If you want this to persist when closing your app, you should serialize [`Memory`] and store it.
/// For this you need to enable the `persistence`.
///
/// If you want to store data for your widgets, you should look at [`Memory::data`]
#[derive(Clone, Debug)]
#[cfg_attr(feature = "persistence", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "persistence", serde(default))]
pub struct Memory {
    /// Global egui options.
    pub options: Options,

    /// This map stores some superficial state for all widgets with custom [`Id`]s.
    ///
    /// This includes storing whether a [`crate::CollapsingHeader`] is open, how far scrolled a
    /// [`crate::ScrollArea`] is, where the cursor in a [`crate::TextEdit`] is, etc.
    ///
    /// This is NOT meant to store any important data. Store that in your own structures!
    ///
    /// Each read clones the data, so keep your values cheap to clone.
    /// If you want to store a lot of data, you should wrap it in `Arc<Mutex<…>>` so it is cheap to clone.
    ///
    /// This will be saved between different program runs if you use the `persistence` feature.
    ///
    /// To store a state common for all your widgets (a singleton), use [`Id::NULL`] as the key.
    pub data: crate::util::IdTypeMap,

    // ------------------------------------------
    /// Can be used to cache computations from one frame to another.
    ///
    /// This is for saving CPU time when you have something that may take 1-100ms to compute.
    /// Very slow operations (>100ms) should instead be done async (i.e. in another thread)
    /// so as not to lock the UI thread.
    ///
    /// ```
    /// use egui::cache::{ComputerMut, FrameCache};
    ///
    /// #[derive(Default)]
    /// struct CharCounter {}
    /// impl ComputerMut<&str, usize> for CharCounter {
    ///     fn compute(&mut self, s: &str) -> usize {
    ///         s.chars().count() // you probably want to cache something more expensive than this
    ///     }
    /// }
    /// type CharCountCache<'a> = FrameCache<usize, CharCounter>;
    ///
    /// # let mut ctx = egui::Context::default();
    /// ctx.memory_mut(|mem| {
    ///     let cache = mem.caches.cache::<CharCountCache<'_>>();
    ///     assert_eq!(cache.get("hello"), 5);
    /// });
    /// ```
    #[cfg_attr(feature = "persistence", serde(skip))]
    pub caches: crate::cache::CacheStorage,

    // ------------------------------------------
    /// new fonts that will be applied at the start of the next frame
    #[cfg_attr(feature = "persistence", serde(skip))]
    pub(crate) new_font_definitions: Option<epaint::text::FontDefinitions>,

    /// add new font that will be applied at the start of the next frame
    #[cfg_attr(feature = "persistence", serde(skip))]
    pub(crate) add_fonts: Vec<epaint::text::FontInsert>,

    // Current active viewport
    #[cfg_attr(feature = "persistence", serde(skip))]
    pub(crate) viewport_id: ViewportId,

    #[cfg_attr(feature = "persistence", serde(skip))]
    everything_is_visible: bool,

    /// Transforms per layer.
    ///
    /// Instead of using this directly, use:
    /// * [`crate::Context::set_transform_layer`]
    /// * [`crate::Context::layer_transform_to_global`]
    /// * [`crate::Context::layer_transform_from_global`]
    pub to_global: HashMap<LayerId, TSTransform>,

    // -------------------------------------------------
    // Per-viewport:
    areas: ViewportIdMap<Areas>,

    #[cfg_attr(feature = "persistence", serde(skip))]
    pub(crate) interactions: ViewportIdMap<InteractionState>,

    #[cfg_attr(feature = "persistence", serde(skip))]
    pub(crate) focus: ViewportIdMap<Focus>,

    /// Which popup-window is open on a viewport (if any)?
    /// Could be a combo box, color picker, menu, etc.
    /// Optionally stores the position of the popup (usually this would be the position where
    /// the user clicked).
    /// If position is [`None`], the popup position will be calculated based on some configuration
    /// (e.g. relative to some other widget).
    #[cfg_attr(feature = "persistence", serde(skip))]
    popups: ViewportIdMap<OpenPopup>,
}

impl Default for Memory {
    fn default() -> Self {
        let mut slf = Self {
            options: Default::default(),
            data: Default::default(),
            caches: Default::default(),
            new_font_definitions: Default::default(),
            interactions: Default::default(),
            focus: Default::default(),
            viewport_id: Default::default(),
            areas: Default::default(),
            to_global: Default::default(),
            popups: Default::default(),
            everything_is_visible: Default::default(),
            add_fonts: Default::default(),
        };
        slf.interactions.entry(slf.viewport_id).or_default();
        slf.areas.entry(slf.viewport_id).or_default();
        slf
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FocusDirection {
    /// Select the widget closest above the current focused widget.
    Up,

    /// Select the widget to the right of the current focused widget.
    Right,

    /// Select the widget below the current focused widget.
    Down,

    /// Select the widget to the left of the current focused widget.
    Left,

    /// Select the previous widget that had focus.
    Previous,

    /// Select the next widget that wants focus.
    Next,

    /// Don't change focus.
    #[default]
    None,
}

impl FocusDirection {
    fn is_cardinal(&self) -> bool {
        match self {
            Self::Up | Self::Right | Self::Down | Self::Left => true,

            Self::Previous | Self::Next | Self::None => false,
        }
    }
}

// ----------------------------------------------------------------------------

/// Some global options that you can read and write.
///
/// See also [`crate::style::DebugOptions`].
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Options {
    /// The default style for new [`Ui`](crate::Ui):s in dark mode.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub dark_style: std::sync::Arc<Style>,

    /// The default style for new [`Ui`](crate::Ui):s in light mode.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub light_style: std::sync::Arc<Style>,

    /// Preference for selection between dark and light [`crate::Context::style`]
    /// as the active style used by all subsequent windows, panels, etc.
    ///
    /// Default: `ThemePreference::System`.
    pub theme_preference: ThemePreference,

    /// Which theme to use in case [`Self::theme_preference`] is [`ThemePreference::System`]
    /// and egui fails to detect the system theme.
    ///
    /// Default: [`crate::Theme::Dark`].
    pub fallback_theme: Theme,

    /// The current system theme, used to choose between
    /// dark and light style in case [`Self::theme_preference`] is [`ThemePreference::System`].
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) system_theme: Option<Theme>,

    /// Global zoom factor of the UI.
    ///
    /// This is used to calculate the `pixels_per_point`
    /// for the UI as `pixels_per_point = zoom_fator * native_pixels_per_point`.
    ///
    /// The default is 1.0. Increase it to make all UI elements larger.
    ///
    /// You should call [`crate::Context::set_zoom_factor`]
    /// instead of modifying this directly!
    pub zoom_factor: f32,

    /// If `true`, egui will change the scale of the ui ([`crate::Context::zoom_factor`]) when the user
    /// presses Cmd+Plus, Cmd+Minus or Cmd+0, just like in a browser.
    ///
    /// This is `true` by default.
    ///
    /// On the web-backend of `eframe` this is set to false by default,
    /// so that the zoom shortcuts are handled exclusively by the browser,
    /// which will change the `native_pixels_per_point` (`devicePixelRatio`).
    /// You can still zoom egui independently by calling [`crate::Context::set_zoom_factor`],
    /// which will be applied on top of the browsers global zoom.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub zoom_with_keyboard: bool,

    /// Controls the tessellator.
    pub tessellation_options: epaint::TessellationOptions,

    /// If any widget moves or changes id, repaint everything.
    ///
    /// It is recommended you keep this OFF, as it may
    /// lead to endless repaints for an unknown reason. See
    /// (<https://github.com/rerun-io/rerun/issues/5018>).
    pub repaint_on_widget_change: bool,

    /// Maximum number of passes to run in one frame.
    ///
    /// Set to `1` for pure single-pass immediate mode.
    /// Set to something larger than `1` to allow multi-pass when needed.
    ///
    /// Default is `2`. This means sometimes a frame will cost twice as much,
    /// but usually only rarely (e.g. when showing a new panel for the first time).
    ///
    /// egui will usually only ever run one pass, even if `max_passes` is large.
    ///
    /// If this is `1`, [`crate::Context::request_discard`] will be ignored.
    ///
    /// Multi-pass is supported by [`crate::Context::run`].
    ///
    /// See [`crate::Context::request_discard`] for more.
    pub max_passes: NonZeroUsize,

    /// This is a signal to any backend that we want the [`crate::PlatformOutput::events`] read out loud.
    ///
    /// The only change to egui is that labels can be focused by pressing tab.
    ///
    /// Screen readers are an experimental feature of egui, and not supported on all platforms.
    /// `eframe` only supports it on web.
    ///
    /// Consider using [AccessKit](https://github.com/AccessKit/accesskit) instead,
    /// which is supported by `eframe`.
    pub screen_reader: bool,

    /// If true, the most common glyphs (ASCII) are pre-rendered to the texture atlas.
    ///
    /// Only the fonts in [`Style::text_styles`] will be pre-cached.
    ///
    /// This can lead to fewer texture operations, but may use up the texture atlas quicker
    /// if you are changing [`Style::text_styles`], or have a lot of text styles.
    pub preload_font_glyphs: bool,

    /// Check reusing of [`Id`]s, and show a visual warning on screen when one is found.
    ///
    /// By default this is `true` in debug builds.
    pub warn_on_id_clash: bool,

    /// Options related to input state handling.
    pub input_options: crate::input_state::InputOptions,

    /// If `true`, `egui` will discard the loaded image data after
    /// the texture is loaded onto the GPU to reduce memory usage.
    ///
    /// In modern GPU rendering, the texture data is not required after the texture is loaded.
    ///
    /// This is beneficial when using a large number or resolution of images and there is no need to
    /// retain the image data, potentially saving a significant amount of memory.
    ///
    /// The drawback is that it becomes impossible to serialize the loaded images or render in non-GPU systems.
    ///
    /// Default is `false`.
    pub reduce_texture_memory: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dark_style: std::sync::Arc::new(Theme::Dark.default_style()),
            light_style: std::sync::Arc::new(Theme::Light.default_style()),
            theme_preference: Default::default(),
            fallback_theme: Theme::Dark,
            system_theme: None,
            zoom_factor: 1.0,
            zoom_with_keyboard: true,
            tessellation_options: Default::default(),
            repaint_on_widget_change: false,
            max_passes: NonZeroUsize::new(2).unwrap(),
            screen_reader: false,
            preload_font_glyphs: true,
            warn_on_id_clash: cfg!(debug_assertions),

            // Input:
            input_options: Default::default(),
            reduce_texture_memory: false,
        }
    }
}

impl Options {
    pub(crate) fn begin_pass(&mut self, new_raw_input: &RawInput) {
        self.system_theme = new_raw_input.system_theme;
    }

    /// The currently active theme (may depend on the system theme).
    pub(crate) fn theme(&self) -> Theme {
        match self.theme_preference {
            ThemePreference::Dark => Theme::Dark,
            ThemePreference::Light => Theme::Light,
            ThemePreference::System => self.system_theme.unwrap_or(self.fallback_theme),
        }
    }

    pub(crate) fn style(&self) -> &std::sync::Arc<Style> {
        match self.theme() {
            Theme::Dark => &self.dark_style,
            Theme::Light => &self.light_style,
        }
    }

    pub(crate) fn style_mut(&mut self) -> &mut std::sync::Arc<Style> {
        match self.theme() {
            Theme::Dark => &mut self.dark_style,
            Theme::Light => &mut self.light_style,
        }
    }
}

impl Options {
    /// Show the options in the ui.
    pub fn ui(&mut self, ui: &mut crate::Ui) {
        let theme = self.theme();

        let Self {
            dark_style, // covered above
            light_style,
            theme_preference,
            fallback_theme: _,
            system_theme: _,
            zoom_factor: _, // TODO(emilk)
            zoom_with_keyboard,
            tessellation_options,
            repaint_on_widget_change,
            max_passes,
            screen_reader: _, // needs to come from the integration
            preload_font_glyphs: _,
            warn_on_id_clash,
            input_options,
            reduce_texture_memory,
        } = self;

        use crate::Widget as _;
        use crate::containers::CollapsingHeader;

        CollapsingHeader::new("⚙ Options")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Max passes:");
                    ui.add(crate::DragValue::new(max_passes).range(0..=10));
                });

                ui.checkbox(
                    repaint_on_widget_change,
                    "Repaint if any widget moves or changes id",
                );

                ui.checkbox(
                    zoom_with_keyboard,
                    "Zoom with keyboard (Cmd +, Cmd -, Cmd 0)",
                );

                ui.checkbox(warn_on_id_clash, "Warn if two widgets have the same Id");

                ui.checkbox(reduce_texture_memory, "Reduce texture memory");
            });

        CollapsingHeader::new("🎑 Style")
            .default_open(true)
            .show(ui, |ui| {
                theme_preference.radio_buttons(ui);

                let style = std::sync::Arc::make_mut(match theme {
                    Theme::Dark => dark_style,
                    Theme::Light => light_style,
                });
                style.ui(ui);
            });

        CollapsingHeader::new("✒ Painting")
            .default_open(false)
            .show(ui, |ui| {
                tessellation_options.ui(ui);
                ui.vertical_centered(|ui| {
                    crate::reset_button(ui, tessellation_options, "Reset paint settings");
                });
            });

        CollapsingHeader::new("🖱 Input")
            .default_open(false)
            .show(ui, |ui| {
                input_options.ui(ui);
            });

        ui.vertical_centered(|ui| crate::reset_button(ui, self, "Reset all"));
    }
}

// ----------------------------------------------------------------------------

/// The state of the interaction in egui,
/// i.e. what is being dragged.
///
/// Say there is a button in a scroll area.
/// If the user clicks the button, the button should click.
/// If the user drags the button we should scroll the scroll area.
/// Therefore, when the mouse is pressed, we register both the button
/// and the scroll area (as `click_id`/`drag_id`).
/// If the user releases the button without moving the mouse, we register it as a click on `click_id`.
/// If the cursor moves too much, we clear the `click_id` and start passing move events to `drag_id`.
#[derive(Clone, Debug, Default)]
pub(crate) struct InteractionState {
    /// A widget interested in clicks that has a mouse press on it.
    pub potential_click_id: Option<Id>,

    /// A widget interested in drags that has a mouse press on it.
    ///
    /// Note that this is set as soon as the mouse is pressed,
    /// so the widget may not yet be marked as "dragged"
    /// as that can only happen after the mouse has moved a bit
    /// (at least if the widget is interesated in both clicks and drags).
    pub potential_drag_id: Option<Id>,
}

/// Keeps tracks of what widget has keyboard focus
#[derive(Clone, Debug, Default)]
pub(crate) struct Focus {
    /// The widget with keyboard focus (i.e. a text input field).
    focused_widget: Option<FocusWidget>,

    /// The ID of a widget that had keyboard focus during the previous frame.
    id_previous_frame: Option<Id>,

    /// The ID of a widget to give the focus to in the next frame.
    id_next_frame: Option<Id>,

    #[cfg(feature = "accesskit")]
    id_requested_by_accesskit: Option<accesskit::NodeId>,

    /// If set, the next widget that is interested in focus will automatically get it.
    /// Probably because the user pressed Tab.
    give_to_next: bool,

    /// The last widget interested in focus.
    last_interested: Option<Id>,

    /// Set when looking for widget with navigational keys like arrows, tab, shift+tab.
    focus_direction: FocusDirection,

    /// The top-most modal layer from the previous frame.
    top_modal_layer: Option<LayerId>,

    /// The top-most modal layer from the current frame.
    top_modal_layer_current_frame: Option<LayerId>,

    /// A cache of widget IDs that are interested in focus with their corresponding rectangles.
    focus_widgets_cache: IdMap<Rect>,
}

/// The widget with focus.
#[derive(Clone, Copy, Debug)]
struct FocusWidget {
    pub id: Id,
    pub filter: EventFilter,
}

impl FocusWidget {
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            filter: Default::default(),
        }
    }
}

impl InteractionState {
    /// Are we currently clicking or dragging an egui widget?
    pub fn is_using_pointer(&self) -> bool {
        self.potential_click_id.is_some() || self.potential_drag_id.is_some()
    }
}

impl Focus {
    /// Which widget currently has keyboard focus?
    pub fn focused(&self) -> Option<Id> {
        self.focused_widget.as_ref().map(|w| w.id)
    }

    fn begin_pass(&mut self, new_input: &crate::data::input::RawInput) {
        self.id_previous_frame = self.focused();
        if let Some(id) = self.id_next_frame.take() {
            self.focused_widget = Some(FocusWidget::new(id));
        }
        let event_filter = self.focused_widget.map(|w| w.filter).unwrap_or_default();

        #[cfg(feature = "accesskit")]
        {
            self.id_requested_by_accesskit = None;
        }

        self.focus_direction = FocusDirection::None;

        for event in &new_input.events {
            if !event_filter.matches(event) {
                if let crate::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if let Some(cardinality) = match key {
                        crate::Key::ArrowUp => Some(FocusDirection::Up),
                        crate::Key::ArrowRight => Some(FocusDirection::Right),
                        crate::Key::ArrowDown => Some(FocusDirection::Down),
                        crate::Key::ArrowLeft => Some(FocusDirection::Left),

                        crate::Key::Tab => {
                            if modifiers.shift {
                                Some(FocusDirection::Previous)
                            } else {
                                Some(FocusDirection::Next)
                            }
                        }
                        crate::Key::Escape => {
                            self.focused_widget = None;
                            Some(FocusDirection::None)
                        }
                        _ => None,
                    } {
                        self.focus_direction = cardinality;
                    }
                }
            }

            #[cfg(feature = "accesskit")]
            {
                if let crate::Event::AccessKitActionRequest(accesskit::ActionRequest {
                    action: accesskit::Action::Focus,
                    target,
                    data: None,
                }) = event
                {
                    self.id_requested_by_accesskit = Some(*target);
                }
            }
        }
    }

    pub(crate) fn end_pass(&mut self, used_ids: &IdMap<Rect>) {
        if self.focus_direction.is_cardinal() {
            if let Some(found_widget) = self.find_widget_in_direction(used_ids) {
                self.focused_widget = Some(FocusWidget::new(found_widget));
            }
        }

        if let Some(focused_widget) = self.focused_widget {
            // Allow calling `request_focus` one frame and not using it until next frame
            let recently_gained_focus = self.id_previous_frame != Some(focused_widget.id);

            if !recently_gained_focus && !used_ids.contains_key(&focused_widget.id) {
                // Dead-mans-switch: the widget with focus has disappeared!
                self.focused_widget = None;
            }
        }

        self.top_modal_layer = self.top_modal_layer_current_frame.take();
    }

    pub(crate) fn had_focus_last_frame(&self, id: impl Into<Id>) -> bool {
        self.id_previous_frame == Some(id.into())
    }

    fn interested_in_focus(&mut self, id: impl Into<Id>) {
        let id = id.into();
        #[cfg(feature = "accesskit")]
        {
            if self.id_requested_by_accesskit == Some(id.accesskit_id()) {
                self.focused_widget = Some(FocusWidget::new(id));
                self.id_requested_by_accesskit = None;
                self.give_to_next = false;
                self.reset_focus();
            }
        }

        // The rect is updated at the end of the frame.
        self.focus_widgets_cache
            .entry(id)
            .or_insert(Rect::EVERYTHING);

        if self.give_to_next && !self.had_focus_last_frame(id) {
            self.focused_widget = Some(FocusWidget::new(id));
            self.give_to_next = false;
        } else if self.focused() == Some(id) {
            if self.focus_direction == FocusDirection::Next {
                self.focused_widget = None;
                self.give_to_next = true;
                self.reset_focus();
            } else if self.focus_direction == FocusDirection::Previous {
                self.id_next_frame = self.last_interested; // frame-delay so gained_focus works
                self.reset_focus();
            }
        } else if self.focus_direction == FocusDirection::Next
            && self.focused_widget.is_none()
            && !self.give_to_next
        {
            // nothing has focus and the user pressed tab - give focus to the first widgets that wants it:
            self.focused_widget = Some(FocusWidget::new(id));
            self.reset_focus();
        } else if self.focus_direction == FocusDirection::Previous
            && self.focused_widget.is_none()
            && !self.give_to_next
        {
            // nothing has focus and the user pressed Shift+Tab - give focus to the last widgets that wants it:
            self.focused_widget = self.last_interested.map(FocusWidget::new);
            self.reset_focus();
        }

        self.last_interested = Some(id);
    }

    fn set_modal_layer(&mut self, layer_id: LayerId) {
        self.top_modal_layer_current_frame = Some(layer_id);
    }

    pub(crate) fn top_modal_layer(&self) -> Option<LayerId> {
        self.top_modal_layer
    }

    fn reset_focus(&mut self) {
        self.focus_direction = FocusDirection::None;
    }

    fn find_widget_in_direction(&mut self, new_rects: &IdMap<Rect>) -> Option<Id> {
        // NOTE: `new_rects` here include some widgets _not_ interested in focus.

        /// * negative if `a` is left of `b`
        /// * positive if `a` is right of `b`
        /// * zero if the ranges overlap significantly
        fn range_diff(a: Rangef, b: Rangef) -> f32 {
            let has_significant_overlap = a.intersection(b).span() >= 0.5 * b.span().min(a.span());
            if has_significant_overlap {
                0.0
            } else {
                a.center() - b.center()
            }
        }

        let current_focused = self.focused_widget?;

        // In what direction we are looking for the next widget.
        let search_direction = match self.focus_direction {
            FocusDirection::Up => Vec2::UP,
            FocusDirection::Right => Vec2::RIGHT,
            FocusDirection::Down => Vec2::DOWN,
            FocusDirection::Left => Vec2::LEFT,
            _ => {
                return None;
            }
        };

        // Update cache with new rects
        self.focus_widgets_cache.retain(|id, old_rect| {
            if let Some(new_rect) = new_rects.get(id) {
                *old_rect = *new_rect;
                true // Keep the item
            } else {
                false // Remove the item
            }
        });

        let current_rect = self.focus_widgets_cache.get(&current_focused.id)?;

        let mut best_score = f32::INFINITY;
        let mut best_id = None;

        for (candidate_id, candidate_rect) in &self.focus_widgets_cache {
            if *candidate_id == current_focused.id {
                continue;
            }

            // There is a lot of room for improvement here.
            let to_candidate = vec2(
                range_diff(candidate_rect.x_range(), current_rect.x_range()),
                range_diff(candidate_rect.y_range(), current_rect.y_range()),
            );

            let acos_angle = to_candidate.normalized().dot(search_direction);

            // Only interested in widgets that fall in a 90° cone (±45°)
            // of the search direction.
            let is_in_search_cone = 0.5_f32.sqrt() <= acos_angle;
            if is_in_search_cone {
                let distance = to_candidate.length();

                // There is a lot of room for improvement here.
                let score = distance / (acos_angle * acos_angle);

                if score < best_score {
                    best_score = score;
                    best_id = Some(*candidate_id);
                }
            }
        }

        best_id
    }
}

impl Memory {
    pub(crate) fn begin_pass(&mut self, new_raw_input: &RawInput, viewports: &ViewportIdSet) {
        profiling::function_scope!();

        self.viewport_id = new_raw_input.viewport_id;

        // Cleanup
        self.interactions.retain(|id, _| viewports.contains(id));
        self.areas.retain(|id, _| viewports.contains(id));
        self.popups.retain(|id, _| viewports.contains(id));

        self.areas.entry(self.viewport_id).or_default();

        // self.interactions  is handled elsewhere

        self.options.begin_pass(new_raw_input);

        self.focus
            .entry(self.viewport_id)
            .or_default()
            .begin_pass(new_raw_input);
    }

    pub(crate) fn end_pass(&mut self, used_ids: &IdMap<Rect>) {
        self.caches.update();
        self.areas_mut().end_pass();
        self.focus_mut().end_pass(used_ids);

        // Clean up abandoned popups.
        if let Some(popup) = self.popups.get_mut(&self.viewport_id) {
            if popup.open_this_frame {
                popup.open_this_frame = false;
            } else {
                self.popups.remove(&self.viewport_id);
            }
        }
    }

    pub(crate) fn set_viewport_id(&mut self, viewport_id: ViewportId) {
        self.viewport_id = viewport_id;
    }

    /// Access memory of the [`Area`](crate::containers::area::Area)s, such as `Window`s.
    pub fn areas(&self) -> &Areas {
        self.areas
            .get(&self.viewport_id)
            .expect("Memory broken: no area for the current viewport")
    }

    /// Access memory of the [`Area`](crate::containers::area::Area)s, such as `Window`s.
    pub fn areas_mut(&mut self) -> &mut Areas {
        self.areas.entry(self.viewport_id).or_default()
    }

    /// Top-most layer at the given position.
    pub fn layer_id_at(&self, pos: Pos2) -> Option<LayerId> {
        self.areas()
            .layer_id_at(pos, &self.to_global)
            .and_then(|layer_id| {
                if self.is_above_modal_layer(layer_id) {
                    Some(layer_id)
                } else {
                    self.top_modal_layer()
                }
            })
    }

    /// The currently set transform of a layer.
    #[deprecated = "Use `Context::layer_transform_to_global` instead"]
    pub fn layer_transforms(&self, layer_id: LayerId) -> Option<TSTransform> {
        self.to_global.get(&layer_id).copied()
    }

    /// An iterator over all layers. Back-to-front, top is last.
    pub fn layer_ids(&self) -> impl ExactSizeIterator<Item = LayerId> + '_ {
        self.areas().order().iter().copied()
    }

    /// Check if the layer had focus last frame.
    /// returns `true` if the layer had focus last frame, but not this one.
    pub fn had_focus_last_frame(&self, id: impl Into<Id>) -> bool {
        self.focus().and_then(|f| f.id_previous_frame) == Some(id.into())
    }

    /// Check if the layer lost focus last frame.
    /// returns `true` if the layer lost focus last frame, but not this one.
    pub(crate) fn lost_focus(&self, id: impl Into<Id>) -> bool {
        let id = id.into();
        self.had_focus_last_frame(id) && !self.has_focus(id)
    }

    /// Check if the layer gained focus this frame.
    /// returns `true` if the layer gained focus this frame, but not last one.
    pub(crate) fn gained_focus(&self, id: impl Into<Id>) -> bool {
        let id = id.into();
        !self.had_focus_last_frame(id) && self.has_focus(id)
    }

    /// Does this widget have keyboard focus?
    ///
    /// This function does not consider whether the UI as a whole (e.g. window)
    /// has the keyboard focus. That makes this function suitable for deciding
    /// widget state that should not be disrupted if the user moves away from
    /// the window and back.
    #[inline(always)]
    pub fn has_focus(&self, id: impl Into<Id>) -> bool {
        self.focused() == Some(id.into())
    }

    /// Which widget has keyboard focus?
    pub fn focused(&self) -> Option<Id> {
        self.focus().and_then(|f| f.focused())
    }

    /// Set an event filter for a widget.
    ///
    /// This allows you to control whether the widget will loose focus
    /// when the user presses tab, arrow keys, or escape.
    ///
    /// You must first give focus to the widget before calling this.
    pub fn set_focus_lock_filter(&mut self, id: impl Into<Id>, event_filter: EventFilter) {
        let id = id.into();
        if self.had_focus_last_frame(id) && self.has_focus(id) {
            if let Some(focused) = &mut self.focus_mut().focused_widget {
                if focused.id == id {
                    focused.filter = event_filter;
                }
            }
        }
    }

    /// Give keyboard focus to a specific widget.
    /// See also [`crate::Response::request_focus`].
    #[inline(always)]
    pub fn request_focus(&mut self, id: impl Into<Id>) {
        self.focus_mut().focused_widget = Some(FocusWidget::new(id));
    }

    /// Surrender keyboard focus for a specific widget.
    /// See also [`crate::Response::surrender_focus`].
    #[inline(always)]
    pub fn surrender_focus(&mut self, id: impl Into<Id>) {
        let focus = self.focus_mut();
        if focus.focused() == Some(id.into()) {
            focus.focused_widget = None;
        }
    }

    /// Returns true if
    /// - this layer is the top-most modal layer or above it
    /// - there is no modal layer
    pub fn is_above_modal_layer(&self, layer_id: LayerId) -> bool {
        if let Some(modal_layer) = self.focus().and_then(|f| f.top_modal_layer) {
            matches!(
                self.areas().compare_order(layer_id, modal_layer),
                std::cmp::Ordering::Equal | std::cmp::Ordering::Greater
            )
        } else {
            true
        }
    }

    /// Does this layer allow interaction?
    /// Returns true if
    ///  - the layer is not behind a modal layer
    ///  - the [`Order`] allows interaction
    pub fn allows_interaction(&self, layer_id: LayerId) -> bool {
        let is_above_modal_layer = self.is_above_modal_layer(layer_id);
        let ordering_allows_interaction = layer_id.order.allow_interaction();
        is_above_modal_layer && ordering_allows_interaction
    }

    /// Register this widget as being interested in getting keyboard focus.
    /// This will allow the user to select it with tab and shift-tab.
    /// This is normally done automatically when handling interactions,
    /// but it is sometimes useful to pre-register interest in focus,
    /// e.g. before deciding which type of underlying widget to use,
    /// as in the [`crate::DragValue`] widget, so a widget can be focused
    /// and rendered correctly in a single frame.
    ///
    /// Pass in the `layer_id` of the layer that the widget is in.
    #[inline(always)]
    pub fn interested_in_focus(&mut self, id: impl Into<Id>, layer_id: LayerId) {
        if !self.allows_interaction(layer_id) {
            return;
        }
        self.focus_mut().interested_in_focus(id);
    }

    /// Limit focus to widgets on the given layer and above.
    /// If this is called multiple times per frame, the top layer wins.
    pub fn set_modal_layer(&mut self, layer_id: LayerId) {
        if let Some(current) = self.focus().and_then(|f| f.top_modal_layer_current_frame) {
            if matches!(
                self.areas().compare_order(layer_id, current),
                std::cmp::Ordering::Less
            ) {
                return;
            }
        }

        self.focus_mut().set_modal_layer(layer_id);
    }

    /// Get the top modal layer (from the previous frame).
    pub fn top_modal_layer(&self) -> Option<LayerId> {
        self.focus()?.top_modal_layer()
    }

    /// Stop editing the active [`TextEdit`](crate::TextEdit) (if any).
    #[inline(always)]
    pub fn stop_text_input(&mut self) {
        self.focus_mut().focused_widget = None;
    }

    /// Forget window positions, sizes etc.
    /// Can be used to auto-layout windows.
    pub fn reset_areas(&mut self) {
        for area in self.areas.values_mut() {
            *area = Default::default();
        }
    }

    /// Obtain the previous rectangle of an area.
    pub fn area_rect(&self, id: impl Into<Id>) -> Option<Rect> {
        self.areas().get(id.into()).map(|state| state.rect())
    }

    pub(crate) fn interaction(&self) -> &InteractionState {
        self.interactions
            .get(&self.viewport_id)
            .expect("Failed to get interaction")
    }

    pub(crate) fn interaction_mut(&mut self) -> &mut InteractionState {
        self.interactions.entry(self.viewport_id).or_default()
    }

    pub(crate) fn focus(&self) -> Option<&Focus> {
        self.focus.get(&self.viewport_id)
    }

    pub(crate) fn focus_mut(&mut self) -> &mut Focus {
        self.focus.entry(self.viewport_id).or_default()
    }
}

/// State of an open popup.
#[derive(Clone, Copy, Debug)]
struct OpenPopup {
    /// Id of the popup.
    id: Id,

    /// Optional position of the popup.
    pos: Option<Pos2>,

    /// Whether this popup was still open this frame. Otherwise it's considered abandoned and `Memory::popup` will be cleared.
    open_this_frame: bool,
}

impl OpenPopup {
    /// Create a new `OpenPopup`.
    fn new(id: impl Into<Id>, pos: Option<Pos2>) -> Self {
        Self {
            id: id.into(),
            pos,
            open_this_frame: true,
        }
    }
}

/// ## Deprecated popup API
/// Use [`crate::Popup`] instead.
impl Memory {
    /// Is the given popup open?
    #[deprecated = "Use Popup::is_id_open instead"]
    pub fn is_popup_open(&self, popup_id: impl Into<Id>) -> bool {
        self.popups
            .get(&self.viewport_id)
            .is_some_and(|state| state.id == popup_id.into())
            || self.everything_is_visible()
    }

    /// Is any popup open?
    #[deprecated = "Use Popup::is_any_open instead"]
    pub fn any_popup_open(&self) -> bool {
        self.popups.contains_key(&self.viewport_id) || self.everything_is_visible()
    }

    /// Open the given popup and close all others.
    ///
    /// Note that you must call `keep_popup_open` on subsequent frames as long as the popup is open.
    #[deprecated = "Use Popup::open_id instead"]
    pub fn open_popup(&mut self, popup_id: impl Into<Id>) {
        self.popups
            .insert(self.viewport_id, OpenPopup::new(popup_id, None));
    }

    /// Popups must call this every frame while open.
    ///
    /// This is needed because in some cases popups can go away without `close_popup` being
    /// called. For example, when a context menu is open and the underlying widget stops
    /// being rendered.
    #[deprecated = "Use Popup::show instead"]
    pub fn keep_popup_open(&mut self, popup_id: impl Into<Id>) {
        if let Some(state) = self.popups.get_mut(&self.viewport_id) {
            if state.id == popup_id.into() {
                state.open_this_frame = true;
            }
        }
    }

    /// Open the popup and remember its position.
    #[deprecated = "Use Popup with PopupAnchor::Position instead"]
    pub fn open_popup_at(&mut self, popup_id: impl Into<Id>, pos: impl Into<Option<Pos2>>) {
        self.popups
            .insert(self.viewport_id, OpenPopup::new(popup_id, pos.into()));
    }

    /// Get the position for this popup.
    #[deprecated = "Use Popup::position_of_id instead"]
    pub fn popup_position(&self, id: impl Into<Id>) -> Option<Pos2> {
        self.popups
            .get(&self.viewport_id)
            .and_then(|state| if state.id == id.into() { state.pos } else { None })
    }

    /// Close any currently open popup.
    #[deprecated = "Use Popup::close_all instead"]
    pub fn close_all_popups(&mut self) {
        self.popups.clear();
    }

    /// Close the given popup, if it is open.
    ///
    /// See also [`Self::close_all_popups`] if you want to close any / all currently open popups.
    #[deprecated = "Use Popup::close_id instead"]
    pub fn close_popup(&mut self, popup_id: impl Into<Id>) {
        #[expect(deprecated)]
        if self.is_popup_open(popup_id) {
            self.popups.remove(&self.viewport_id);
        }
    }

    /// Toggle the given popup between closed and open.
    ///
    /// Note: At most, only one popup can be open at a time.
    #[deprecated = "Use Popup::toggle_id instead"]
    pub fn toggle_popup(&mut self, popup_id: impl Into<Id>) {
        let id = popup_id.into();

        #[expect(deprecated)]
        if self.is_popup_open(id) {
            self.close_popup(id);
        } else {
            self.open_popup(id);
        }
    }
}

impl Memory {
    /// If true, all windows, menus, tooltips, etc., will be visible at once.
    ///
    /// This is useful for testing, benchmarking, pre-caching, etc.
    ///
    /// Experimental feature!
    #[inline(always)]
    pub fn everything_is_visible(&self) -> bool {
        self.everything_is_visible
    }

    /// If true, all windows, menus, tooltips etc are to be visible at once.
    ///
    /// This is useful for testing, benchmarking, pre-caching, etc.
    ///
    /// Experimental feature!
    pub fn set_everything_is_visible(&mut self, value: bool) {
        self.everything_is_visible = value;
    }
}

// ----------------------------------------------------------------------------

/// Map containing the index of each layer in the order list, for quick lookups.
type OrderMap = HashMap<LayerId, usize>;

/// Keeps track of [`Area`](crate::containers::area::Area)s, which are free-floating [`Ui`](crate::Ui)s.
/// These [`Area`](crate::containers::area::Area)s can be in any [`Order`].
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Areas {
    areas: IdMap<area::AreaState>,

    visible_areas_last_frame: ahash::HashSet<LayerId>,
    visible_areas_current_frame: ahash::HashSet<LayerId>,

    // ----------------------------
    // Everything below this is general to all layers, not just areas.
    // TODO(emilk): move this to a separate struct.
    /// Back-to-front,  top is last.
    order: Vec<LayerId>,

    /// Inverse of [`Self::order`], calculated at the end of the frame.
    order_map: OrderMap,

    /// When an area wants to be on top, it is assigned here.
    /// This is used to reorder the layers at the end of the frame.
    /// If several layers want to be on top, they will keep their relative order.
    /// This means closing three windows and then reopening them all in one frame
    /// results in them being sent to the top and keeping their previous internal order.
    wants_to_be_on_top: ahash::HashSet<LayerId>,

    /// The sublayers that each layer has.
    ///
    /// The parent sublayer is moved directly above the child sublayers in the ordering.
    sublayers: ahash::HashMap<LayerId, HashSet<LayerId>>,
}

impl Areas {
    pub(crate) fn count(&self) -> usize {
        self.areas.len()
    }

    pub(crate) fn get(&self, id: impl Into<Id>) -> Option<&area::AreaState> {
        self.areas.get(&id.into())
    }

    /// All layers back-to-front, top is last.
    pub(crate) fn order(&self) -> &[LayerId] {
        &self.order
    }

    /// Compare the order of two layers, based on the order list from last frame.
    ///
    /// May return [`std::cmp::Ordering::Equal`] if the layers are not in the order list.
    pub(crate) fn compare_order(&self, a: LayerId, b: LayerId) -> std::cmp::Ordering {
        // Sort by layer `order` first and use `order_map` to resolve disputes.
        // If `order_map` only contains one layer ID, then the other one will be
        // lower because `None < Some(x)`.
        match a.order.cmp(&b.order) {
            std::cmp::Ordering::Equal => self.order_map.get(&a).cmp(&self.order_map.get(&b)),
            cmp => cmp,
        }
    }

    pub(crate) fn set_state(&mut self, layer_id: LayerId, state: area::AreaState) {
        self.visible_areas_current_frame.insert(layer_id);
        self.areas.insert(layer_id.id, state);
        if !self.order.contains(&layer_id) {
            self.order.push(layer_id);
        }
    }

    /// Top-most layer at the given position.
    pub fn layer_id_at(
        &self,
        pos: Pos2,
        layer_to_global: &HashMap<LayerId, TSTransform>,
    ) -> Option<LayerId> {
        for layer in self.order.iter().rev() {
            if self.is_visible(layer) {
                if let Some(state) = self.areas.get(&layer.id) {
                    let mut rect = state.rect();
                    if state.interactable {
                        if let Some(to_global) = layer_to_global.get(layer) {
                            rect = *to_global * rect;
                        }

                        if rect.contains(pos) {
                            return Some(*layer);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn visible_last_frame(&self, layer_id: &LayerId) -> bool {
        self.visible_areas_last_frame.contains(layer_id)
    }

    pub fn is_visible(&self, layer_id: &LayerId) -> bool {
        self.visible_areas_last_frame.contains(layer_id)
            || self.visible_areas_current_frame.contains(layer_id)
    }

    pub fn visible_layer_ids(&self) -> ahash::HashSet<LayerId> {
        self.visible_areas_last_frame
            .iter()
            .copied()
            .chain(self.visible_areas_current_frame.iter().copied())
            .collect()
    }

    pub(crate) fn visible_windows(&self) -> impl Iterator<Item = (LayerId, &area::AreaState)> {
        self.visible_layer_ids()
            .into_iter()
            .filter(|layer| layer.order == crate::Order::Middle)
            .filter(|&layer| !self.is_sublayer(&layer))
            .filter_map(|layer| Some((layer, self.get(layer.id)?)))
    }

    pub fn move_to_top(&mut self, layer_id: LayerId) {
        self.visible_areas_current_frame.insert(layer_id);
        self.wants_to_be_on_top.insert(layer_id);

        if !self.order.contains(&layer_id) {
            self.order.push(layer_id);
        }
    }

    /// Mark the `child` layer as a sublayer of `parent`.
    ///
    /// Sublayers are moved directly above the parent layer at the end of the frame. This is mainly
    /// intended for adding a new [Area](crate::Area) inside a [Window](crate::Window).
    ///
    /// This currently only supports one level of nesting. If `parent` is a sublayer of another
    /// layer, the behavior is unspecified.
    ///
    /// The two layers must have the same [`LayerId::order`].
    pub fn set_sublayer(&mut self, parent: LayerId, child: LayerId) {
        debug_assert_eq!(
            parent.order, child.order,
            "DEBUG ASSERT: Trying to set sublayers across layers of different order ({:?}, {:?}), which is currently undefined behavior in egui",
            parent.order, child.order
        );

        self.sublayers.entry(parent).or_default().insert(child);

        // Make sure the layers are in the order list:
        if !self.order.contains(&parent) {
            self.order.push(parent);
        }
        if !self.order.contains(&child) {
            self.order.push(child);
        }
    }

    pub fn top_layer_id(&self, order: Order) -> Option<LayerId> {
        self.order
            .iter()
            .filter(|layer| layer.order == order && !self.is_sublayer(layer))
            .next_back()
            .copied()
    }

    /// If this layer is the sublayer of another layer, return the parent.
    pub fn parent_layer(&self, layer_id: LayerId) -> Option<LayerId> {
        self.sublayers.iter().find_map(|(parent, children)| {
            if children.contains(&layer_id) {
                Some(*parent)
            } else {
                None
            }
        })
    }

    /// All the child layers of this layer.
    pub fn child_layers(&self, layer_id: LayerId) -> impl Iterator<Item = LayerId> + '_ {
        self.sublayers.get(&layer_id).into_iter().flatten().copied()
    }

    pub(crate) fn is_sublayer(&self, layer: &LayerId) -> bool {
        self.parent_layer(*layer).is_some()
    }

    pub(crate) fn end_pass(&mut self) {
        let Self {
            visible_areas_last_frame,
            visible_areas_current_frame,
            order,
            wants_to_be_on_top,
            sublayers,
            ..
        } = self;

        std::mem::swap(visible_areas_last_frame, visible_areas_current_frame);
        visible_areas_current_frame.clear();

        order.sort_by_key(|layer| (layer.order, wants_to_be_on_top.contains(layer)));
        wants_to_be_on_top.clear();

        // For all layers with sublayers, put the sublayers directly after the parent layer:
        let sublayers = std::mem::take(sublayers);
        for (parent, children) in sublayers {
            let mut moved_layers = vec![parent];
            order.retain(|l| {
                if children.contains(l) {
                    moved_layers.push(*l);
                    false
                } else {
                    true
                }
            });
            let Some(parent_pos) = order.iter().position(|l| l == &parent) else {
                continue;
            };
            order.splice(parent_pos..=parent_pos, moved_layers);
        }

        self.order_map = self
            .order
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
    }
}

// ----------------------------------------------------------------------------

#[test]
fn memory_impl_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Memory>();
}

#[test]
fn order_map_total_ordering() {
    let mut layers = [
        LayerId::new(Order::Tooltip, Id::new("a")),
        LayerId::new(Order::Background, Id::new("b")),
        LayerId::new(Order::Background, Id::new("c")),
        LayerId::new(Order::Tooltip, Id::new("d")),
        LayerId::new(Order::Background, Id::new("e")),
        LayerId::new(Order::Background, Id::new("f")),
        LayerId::new(Order::Tooltip, Id::new("g")),
    ];
    let mut areas = Areas::default();

    // skip some of the layers
    for &layer in &layers[3..] {
        areas.set_state(layer, crate::AreaState::default());
    }
    areas.end_pass(); // sort layers

    // Sort layers
    layers.sort_by(|&a, &b| areas.compare_order(a, b));

    // Assert that `areas.compare_order()` forms a total ordering
    let mut equivalence_classes = vec![0];
    let mut i = 0;
    for l in layers.windows(2) {
        assert!(l[0].order <= l[1].order, "does not follow LayerId.order");
        if areas.compare_order(l[0], l[1]) != std::cmp::Ordering::Equal {
            i += 1;
        }
        equivalence_classes.push(i);
    }
    assert_eq!(layers.len(), equivalence_classes.len());
    for (&l1, c1) in std::iter::zip(&layers, &equivalence_classes) {
        for (&l2, c2) in std::iter::zip(&layers, &equivalence_classes) {
            assert_eq!(
                c1.cmp(c2),
                areas.compare_order(l1, l2),
                "not a total ordering",
            );
        }
    }
}
