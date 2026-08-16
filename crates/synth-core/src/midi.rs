//! MIDI messages and per-channel/per-voice lifecycle state.

use crate::MIDI_CC_COUNT;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiNote(u8);

impl MidiNote {
    pub const fn new(note: u8) -> Self {
        Self(if note > 127 { 127 } else { note })
    }

    pub const fn number(self) -> u8 {
        self.0
    }

    pub fn frequency(self) -> f32 {
        440.0 * 2.0f32.powf((self.0 as f32 - 69.0) / 12.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        note: MidiNote,
        velocity: f32,
    },
    NoteOff {
        channel: u8,
        note: MidiNote,
        velocity: f32,
    },
    PolyPressure {
        channel: u8,
        note: MidiNote,
        value: f32,
    },
    ChannelPressure {
        channel: u8,
        value: f32,
    },
    PitchBend {
        channel: u8,
        value: f32,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: f32,
    },
    AllNotesOff {
        channel: u8,
    },
    AllSoundOff,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChannelState {
    pub(crate) bend: f32,
    pub(crate) bend_range: f32,
    pub(crate) modulation: f32,
    pub(crate) volume: f32,
    pub(crate) pan: f32,
    pub(crate) expression: f32,
    pub(crate) sustain: bool,
    pub(crate) pressure: f32,
    pub(crate) program: f32,
    pub(crate) cc: [f32; MIDI_CC_COUNT],
}

impl Default for ChannelState {
    fn default() -> Self {
        let mut cc = [0.0; MIDI_CC_COUNT];
        cc[7] = 1.0;
        cc[10] = 0.5;
        cc[11] = 1.0;
        Self {
            bend: 0.0,
            bend_range: 2.0,
            modulation: 0.0,
            volume: 1.0,
            pan: 0.0,
            expression: 1.0,
            sustain: false,
            pressure: 0.0,
            program: 0.0,
            cc,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Voice {
    pub(crate) active: bool,
    pub(crate) key_down: bool,
    pub(crate) released: bool,
    pub(crate) channel: u8,
    pub(crate) note: MidiNote,
    pub(crate) velocity: f32,
    pub(crate) poly_pressure: f32,
    pub(crate) age: u64,
    pub(crate) t: f32,
    pub(crate) l: f32,
    pub(crate) rng: u32,
    pub(crate) note_slot: usize,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            key_down: false,
            released: false,
            channel: 0,
            note: MidiNote::new(0),
            velocity: 0.0,
            poly_pressure: 0.0,
            age: 0,
            t: 0.0,
            l: 0.0,
            rng: 1,
            note_slot: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NoteDomain {
    pub(crate) active: bool,
    pub(crate) channel: u8,
    pub(crate) note: MidiNote,
    pub(crate) voices: u8,
}

impl Default for NoteDomain {
    fn default() -> Self {
        Self {
            active: false,
            channel: 0,
            note: MidiNote::new(0),
            voices: 0,
        }
    }
}

impl Voice {
    pub(crate) fn begin_release(&mut self) {
        if self.active && !self.released {
            self.released = true;
            self.l = 0.0;
        }
    }

    pub(crate) fn next_random(&mut self) -> f32 {
        let mut value = self.rng;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.rng = value.max(1);
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}
