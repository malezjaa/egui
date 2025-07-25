use crate::{
    Id, IdMap, InputState,
    emath::{NumExt as _, remap_clamp},
};

#[derive(Clone, Default)]
pub(crate) struct AnimationManager {
    bools: IdMap<BoolAnim>,
    values: IdMap<ValueAnim>,
}

#[derive(Clone, Debug)]
struct BoolAnim {
    last_value: f32,
    last_tick: f64,
}

#[derive(Clone, Debug)]
struct ValueAnim {
    from_value: f32,

    to_value: f32,

    /// when did `value` last toggle?
    toggle_time: f64,
}

impl AnimationManager {
    /// See [`crate::Context::animate_bool`] for documentation
    pub fn animate_bool(
        &mut self,
        input: &InputState,
        animation_time: f32,
        id: impl Into<Id>,
        value: bool,
    ) -> f32 {
        let id = id.into();
        let (start, end) = if value { (0.0, 1.0) } else { (1.0, 0.0) };
        match self.bools.get_mut(&id) {
            None => {
                self.bools.insert(
                    id,
                    BoolAnim {
                        last_value: end,
                        last_tick: input.time - input.stable_dt as f64,
                    },
                );
                end
            }
            Some(anim) => {
                let BoolAnim {
                    last_value,
                    last_tick,
                } = anim;
                let current_time = input.time;
                let elapsed = ((current_time - *last_tick) as f32).at_most(input.stable_dt);
                let new_value = *last_value + (end - start) * elapsed / animation_time;
                *last_value = if new_value.is_finite() {
                    new_value.clamp(0.0, 1.0)
                } else {
                    end
                };
                *last_tick = current_time;
                *last_value
            }
        }
    }

    pub fn animate_value(
        &mut self,
        input: &InputState,
        animation_time: f32,
        id: impl Into<Id>,
        value: f32,
    ) -> f32 {
        let id = id.into();
        match self.values.get_mut(&id) {
            None => {
                self.values.insert(
                    id,
                    ValueAnim {
                        from_value: value,
                        to_value: value,
                        toggle_time: -f64::INFINITY, // long time ago
                    },
                );
                value
            }
            Some(anim) => {
                let time_since_toggle = (input.time - anim.toggle_time) as f32;
                // On the frame we toggle we don't want to return the old value,
                // so we extrapolate forwards by half a frame:
                let time_since_toggle = time_since_toggle + input.predicted_dt / 2.0;
                let current_value = remap_clamp(
                    time_since_toggle,
                    0.0..=animation_time,
                    anim.from_value..=anim.to_value,
                );
                if anim.to_value != value {
                    anim.from_value = current_value; //start new animation from current position of playing animation
                    anim.to_value = value;
                    anim.toggle_time = input.time;
                }
                if animation_time == 0.0 {
                    anim.from_value = value;
                    anim.to_value = value;
                }
                current_value
            }
        }
    }
}
