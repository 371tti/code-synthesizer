//! Lock-free state shared by the host, UI, and audio thread.

use crate::{MidiNote, WAVEFORM_CAPACITY, finite_or_zero, sanitize_sample_rate};
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicUsize, Ordering};
use synth_dsl::{Inputs, MAX_USER_PARAMETERS, ParameterSpec};
/// Lock-free normalized values shared by the host, WebView, and audio thread.
pub struct UserParameterStore {
    values: [AtomicU32; MAX_USER_PARAMETERS],
    software_revisions: [AtomicU32; MAX_USER_PARAMETERS],
}

impl UserParameterStore {
    pub fn new(specs: &[ParameterSpec]) -> Self {
        Self {
            values: std::array::from_fn(|index| {
                AtomicU32::new(
                    specs
                        .get(index)
                        .map_or(0.0, ParameterSpec::default_normalized)
                        .to_bits(),
                )
            }),
            software_revisions: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    pub fn get_normalized(&self, index: usize) -> f32 {
        self.values
            .get(index)
            .map_or(0.0, |value| f32::from_bits(value.load(Ordering::Relaxed)))
            .clamp(0.0, 1.0)
    }

    pub fn set_normalized(&self, index: usize, value: f32) -> bool {
        let Some(target) = self.values.get(index) else {
            return false;
        };
        target.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.software_revisions[index].fetch_add(1, Ordering::Release);
        true
    }

    pub fn software_revisions(&self) -> [u32; MAX_USER_PARAMETERS] {
        std::array::from_fn(|index| self.software_revisions[index].load(Ordering::Acquire))
    }

    pub fn set_from_midi(&self, index: usize, value: f32, block_start_revision: u32) -> bool {
        let Some(target) = self.values.get(index) else {
            return false;
        };
        if self.software_revisions[index].load(Ordering::Acquire) != block_start_revision {
            return false;
        }
        target.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        true
    }

    pub fn fill_inputs(&self, input: &mut Inputs) {
        for (index, target) in input.params.iter_mut().enumerate() {
            *target = self.get_normalized(index);
        }
    }

    pub fn snapshot(&self) -> [f32; MAX_USER_PARAMETERS] {
        std::array::from_fn(|index| self.get_normalized(index))
    }
}

/// WebViewへ渡す鍵盤ごとの発音状態です。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyboardNoteState {
    pub note: u8,
    pub pressed_velocity: f32,
    pub released_velocity: f32,
}

/// Single-writer, lock-free audio history used by the WebView oscilloscope.
pub struct WaveformMonitor {
    mix_left: [AtomicU32; WAVEFORM_CAPACITY],
    mix_right: [AtomicU32; WAVEFORM_CAPACITY],
    output_left: [AtomicU32; WAVEFORM_CAPACITY],
    output_right: [AtomicU32; WAVEFORM_CAPACITY],
    write_index: AtomicUsize,
    active_voices: AtomicUsize,
    sample_rate: AtomicU32,
    // 下位 8 bit は押下中、上位 8 bit は release envelope 中の最大 velocity。
    keyboard_notes: [AtomicU16; 128],
}

impl WaveformMonitor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            mix_left: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            mix_right: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            output_left: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            output_right: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            write_index: AtomicUsize::new(0),
            active_voices: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(sanitize_sample_rate(sample_rate).to_bits()),
            keyboard_notes: std::array::from_fn(|_| AtomicU16::new(0)),
        }
    }

    pub fn set_sample_rate(&self, sample_rate: f32) {
        self.sample_rate.store(
            sanitize_sample_rate(sample_rate).to_bits(),
            Ordering::Relaxed,
        );
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    pub fn active_voice_count(&self) -> usize {
        self.active_voices.load(Ordering::Relaxed)
    }

    pub fn read_recent(&self, output: bool, length: usize) -> (Vec<f32>, Vec<f32>) {
        let length = length.clamp(1, WAVEFORM_CAPACITY);
        let end = self.write_index.load(Ordering::Acquire);
        let start = end.saturating_sub(length);
        let (left, right) = if output {
            (&self.output_left, &self.output_right)
        } else {
            (&self.mix_left, &self.mix_right)
        };
        let read = |samples: &[AtomicU32; WAVEFORM_CAPACITY]| {
            (start..end)
                .map(|index| {
                    let slot = index % WAVEFORM_CAPACITY;
                    f32::from_bits(samples[slot].load(Ordering::Relaxed))
                })
                .collect()
        };
        (read(left), read(right))
    }

    pub fn keyboard_note_states(&self) -> Vec<KeyboardNoteState> {
        self.keyboard_notes
            .iter()
            .enumerate()
            .filter_map(|(note, state)| {
                let state = state.load(Ordering::Relaxed);
                let pressed_velocity = (state & 0xff) as f32 / 127.0;
                let released_velocity = (state >> 8) as f32 / 127.0;
                (state != 0).then_some(KeyboardNoteState {
                    note: note as u8,
                    pressed_velocity,
                    released_velocity,
                })
            })
            .collect()
    }

    pub(crate) fn set_keyboard_note_state(
        &self,
        note: MidiNote,
        pressed_velocity: f32,
        released_velocity: f32,
    ) {
        let pressed = (pressed_velocity.clamp(0.0, 1.0) * 127.0).round() as u16;
        let released = (released_velocity.clamp(0.0, 1.0) * 127.0).round() as u16;
        self.keyboard_notes[note.number() as usize]
            .store(pressed | released << 8, Ordering::Relaxed);
    }

    pub(crate) fn clear_keyboard_note_states(&self) {
        for state in &self.keyboard_notes {
            state.store(0, Ordering::Relaxed);
        }
    }

    pub(crate) fn push(
        &self,
        mix_left: f32,
        mix_right: f32,
        output_left: f32,
        output_right: f32,
        active_voices: usize,
    ) {
        let index = self.write_index.load(Ordering::Relaxed);
        let slot = index % WAVEFORM_CAPACITY;
        self.mix_left[slot].store(finite_or_zero(mix_left).to_bits(), Ordering::Relaxed);
        self.mix_right[slot].store(finite_or_zero(mix_right).to_bits(), Ordering::Relaxed);
        self.output_left[slot].store(finite_or_zero(output_left).to_bits(), Ordering::Relaxed);
        self.output_right[slot].store(finite_or_zero(output_right).to_bits(), Ordering::Relaxed);
        self.active_voices.store(active_voices, Ordering::Relaxed);
        self.write_index
            .store(index.wrapping_add(1), Ordering::Release);
    }
}
