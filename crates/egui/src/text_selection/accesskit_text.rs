use emath::TSTransform;

use crate::{Context, Galley, Id};

use super::{CCursorRange, text_cursor_state::is_word_char};

/// Update accesskit with the current text state.
pub fn update_accesskit_for_text_widget(
    ctx: &Context,
    widget_id: impl Into<Id>,
    cursor_range: Option<CCursorRange>,
    role: accesskit::Role,
    global_from_galley: TSTransform,
    galley: &Galley,
) {
    let widget_id = widget_id.into();
    let parent_id = ctx.accesskit_node_builder(widget_id, |builder| {
        let parent_id = widget_id;

        if let Some(cursor_range) = &cursor_range {
            let anchor = galley.layout_from_cursor(cursor_range.secondary);
            let focus = galley.layout_from_cursor(cursor_range.primary);
            builder.set_text_selection(accesskit::TextSelection {
                anchor: accesskit::TextPosition {
                    node: parent_id.with(anchor.row).accesskit_id(),
                    character_index: anchor.column,
                },
                focus: accesskit::TextPosition {
                    node: parent_id.with(focus.row).accesskit_id(),
                    character_index: focus.column,
                },
            });
        }

        builder.set_role(role);

        parent_id
    });

    let Some(parent_id) = parent_id else {
        return;
    };

    ctx.with_accessibility_parent(parent_id, || {
        for (row_index, row) in galley.rows.iter().enumerate() {
            let row_id = parent_id.with(row_index);
            ctx.accesskit_node_builder(row_id, |builder| {
                builder.set_role(accesskit::Role::TextRun);
                let rect = global_from_galley * row.rect_without_leading_space();
                builder.set_bounds(accesskit::Rect {
                    x0: rect.min.x.into(),
                    y0: rect.min.y.into(),
                    x1: rect.max.x.into(),
                    y1: rect.max.y.into(),
                });
                builder.set_text_direction(accesskit::TextDirection::LeftToRight);
                // TODO(mwcampbell): Set more node fields for the row
                // once AccessKit adapters expose text formatting info.

                let glyph_count = row.glyphs.len();
                let mut value = String::new();
                value.reserve(glyph_count);
                let mut character_lengths = Vec::<u8>::with_capacity(glyph_count);
                let mut character_positions = Vec::<f32>::with_capacity(glyph_count);
                let mut character_widths = Vec::<f32>::with_capacity(glyph_count);
                let mut word_lengths = Vec::<u8>::new();
                let mut was_at_word_end = false;
                let mut last_word_start = 0usize;

                for glyph in &row.glyphs {
                    let is_word_char = is_word_char(glyph.chr);
                    if is_word_char && was_at_word_end {
                        word_lengths.push((character_lengths.len() - last_word_start) as _);
                        last_word_start = character_lengths.len();
                    }
                    was_at_word_end = !is_word_char;
                    let old_len = value.len();
                    value.push(glyph.chr);
                    character_lengths.push((value.len() - old_len) as _);
                    character_positions.push(glyph.pos.x - row.pos.x);
                    character_widths.push(glyph.advance_width);
                }

                if row.ends_with_newline {
                    value.push('\n');
                    character_lengths.push(1);
                    character_positions.push(row.size.x);
                    character_widths.push(0.0);
                }
                word_lengths.push((character_lengths.len() - last_word_start) as _);

                builder.set_value(value);
                builder.set_character_lengths(character_lengths);
                builder.set_character_positions(character_positions);
                builder.set_character_widths(character_widths);
                builder.set_word_lengths(word_lengths);
            });
        }
    });
}
