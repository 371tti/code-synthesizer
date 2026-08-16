//! Real-time-safe, allocation-free polyphonic stereo synth engine.
//!
//! The audio callback owns a [`SynthEngine`]. DSL compilation happens on a
//! non-real-time thread and publishes boxed programs through [`ProgramExchange`].
//! Old programs are returned to the non-real-time thread before they are
//! dropped, so swapping a program never allocates or deallocates in the audio
//! callback.

mod midi;
mod program;
mod shared;
mod worker;

use midi::{ChannelState, Voice};
pub use midi::{MidiEvent, MidiNote};
pub use program::ProgramExchange;
use program::RuntimeProgram;
pub use shared::{KeyboardNoteState, UserParameterStore, WaveformMonitor};
use std::sync::Arc;
use synth_dsl::{
    Inputs, MAX_USER_PARAMETERS, NoteOutputMode, Program,
    sample_rate_or_default as sanitize_sample_rate,
};
use worker::{MAX_BLOCK_FRAMES, VoiceBlockResult, VoiceBlockSpec, VoiceWorkerPool};

pub const MAX_VOICES: usize = 64;
pub const MIDI_CHANNELS: usize = 16;
pub const MIDI_CC_COUNT: usize = 128;
pub const WAVEFORM_CAPACITY: usize = 4_096;
/// Voice-affine native block workers are enabled for every active voice.
pub const VOICE_WORKER_ENABLED: bool = true;

pub struct SynthEngine {
    sample_rate: f32,
    worker_pool: Option<VoiceWorkerPool>,
    program: Box<RuntimeProgram>,
    exchange: Option<Arc<ProgramExchange>>,
    deferred_retire: Option<Box<RuntimeProgram>>,
    voices: [Voice; MAX_VOICES],
    channels: [ChannelState; MIDI_CHANNELS],
    age: u64,
    global_rng: u32,
    software_revisions_at_block_start: [u32; MAX_USER_PARAMETERS],
    parameters: Option<Arc<UserParameterStore>>,
    waveform: Option<Arc<WaveformMonitor>>,
    block_inputs: Vec<Inputs>,
    block_outputs: Vec<synth_dsl::Outputs>,
    block_audio_left: Vec<f32>,
    block_audio_right: Vec<f32>,
    block_mix_left: Vec<f32>,
    block_mix_right: Vec<f32>,
}

impl SynthEngine {
    pub fn new(sample_rate: f32, program: Program) -> Self {
        Self::new_with_worker_setting(sample_rate, program, true)
    }

    fn new_with_worker_setting(sample_rate: f32, program: Program, workers_enabled: bool) -> Self {
        let sample_rate = sanitize_sample_rate(sample_rate);
        let mut program = Box::new(RuntimeProgram::from_program(program, sample_rate));
        let worker_pool = VoiceWorkerPool::new(&mut program, workers_enabled);
        Self {
            sample_rate,
            worker_pool,
            program,
            exchange: None,
            deferred_retire: None,
            voices: [Voice::default(); MAX_VOICES],
            channels: [ChannelState::default(); MIDI_CHANNELS],
            age: 0,
            global_rng: 0xA341_316C,
            software_revisions_at_block_start: [0; MAX_USER_PARAMETERS],
            parameters: None,
            waveform: None,
            block_inputs: vec![Inputs::default(); MAX_BLOCK_FRAMES],
            block_outputs: vec![synth_dsl::Outputs::default(); MAX_BLOCK_FRAMES],
            block_audio_left: vec![0.0; MAX_BLOCK_FRAMES],
            block_audio_right: vec![0.0; MAX_BLOCK_FRAMES],
            block_mix_left: vec![0.0; MAX_BLOCK_FRAMES],
            block_mix_right: vec![0.0; MAX_BLOCK_FRAMES],
        }
    }

    pub fn with_exchange(
        sample_rate: f32,
        program: Program,
        exchange: Arc<ProgramExchange>,
    ) -> Self {
        let mut engine = Self::new(sample_rate, program);
        exchange.seed(&engine.program.instance);
        engine.exchange = Some(exchange);
        engine
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sanitize_sample_rate(sample_rate);
        let program = self.program.instance.program().clone();
        let mut next = Box::new(RuntimeProgram::from_program(program, self.sample_rate));
        if let Some(pool) = &mut self.worker_pool {
            pool.configure(&mut next, &mut self.program);
        }
        self.program = next;
        if let Some(exchange) = &self.exchange {
            exchange.seed(&self.program.instance);
        }
        if let Some(waveform) = &self.waveform {
            waveform.set_sample_rate(self.sample_rate);
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn attach_runtime_state(
        &mut self,
        parameters: Arc<UserParameterStore>,
        waveform: Arc<WaveformMonitor>,
    ) {
        waveform.set_sample_rate(self.sample_rate);
        self.parameters = Some(parameters);
        self.waveform = Some(waveform);
        self.refresh_all_keyboard_notes();
    }

    /// Apply a host-driven user parameter change in the audio thread.
    ///
    /// This respects the block-start software revision snapshot captured by
    /// `begin_block()` and forwards the change into the shared
    /// `UserParameterStore`. Returns true if the value was accepted.
    pub fn apply_host_user_parameter(&mut self, index: usize, value: f32) -> bool {
        if let Some(parameters) = &self.parameters {
            // `software_revisions_at_block_start` is populated by `begin_block`.
            let block_rev = self
                .software_revisions_at_block_start
                .get(index)
                .copied()
                .unwrap_or(0);
            parameters.set_from_midi(index, value.clamp(0.0, 1.0), block_rev)
        } else {
            false
        }
    }

    /// Read the current normalized user parameter value from the runtime
    /// store, if available. Returns `None` when the runtime hasn't been attached.
    pub fn get_user_parameter_normalized(&self, index: usize) -> Option<f32> {
        self.parameters
            .as_ref()
            .map(|params| params.get_normalized(index))
    }

    /// Immediately set a user parameter's normalized value in the runtime
    /// store. Returns true if the runtime store is present and the value was
    /// written.
    pub fn set_user_parameter_immediate(&mut self, index: usize, value: f32) -> bool {
        if let Some(parameters) = &self.parameters {
            parameters.set_normalized(index, value.clamp(0.0, 1.0))
        } else {
            false
        }
    }

    /// Intended for setup/tests. Hot reload during processing should use
    /// [`ProgramExchange`] so the old program is retired off the audio thread.
    pub fn set_program(&mut self, program: Program) {
        let mut next = Box::new(RuntimeProgram::from_program(program, self.sample_rate));
        if let Some(pool) = &mut self.worker_pool {
            pool.configure(&mut next, &mut self.program);
        }
        self.program = next;
        if let Some(exchange) = &self.exchange {
            exchange.seed(&self.program.instance);
        }
    }

    pub fn begin_block(&mut self) -> bool {
        if let Some(parameters) = &self.parameters {
            self.software_revisions_at_block_start = parameters.software_revisions();
        }
        let swapped = self.exchange.as_ref().is_some_and(|exchange| {
            exchange.swap_at_block_boundary(&mut self.program, &mut self.deferred_retire)
        });
        if swapped && let Some(pool) = &mut self.worker_pool {
            let retired = self
                .deferred_retire
                .as_deref_mut()
                .expect("a swapped program must retain its previous runtime");
            pool.configure(&mut self.program, retired);
        }
        swapped
    }

    pub fn note_on(&mut self, note: MidiNote, velocity: f32) {
        self.note_on_channel(0, note, velocity);
    }

    pub fn note_on_channel(&mut self, channel: u8, note: MidiNote, velocity: f32) {
        let channel = channel.min(15);
        let index = self
            .voices
            .iter()
            .position(|voice| !voice.active)
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, voice)| voice.released)
                    .min_by_key(|(_, voice)| voice.age)
                    .map(|(index, _)| index)
            })
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, voice)| voice.age)
                    .map(|(index, _)| index)
            })
            .expect("MAX_VOICES is non-zero");
        let replaced_note = self.voices[index].active.then_some(self.voices[index].note);
        self.program.instance.reset_voice(index);
        if let Some(pool) = &self.worker_pool {
            pool.reset_voice(index);
        }
        self.age = self.age.wrapping_add(1);
        self.voices[index] = Voice {
            active: true,
            key_down: true,
            released: false,
            channel,
            note,
            velocity: velocity.clamp(0.0, 1.0),
            poly_pressure: 0.0,
            age: self.age,
            t: 0.0,
            l: 0.0,
            rng: seed_voice(channel, note, self.age),
        };
        if let Some(replaced_note) = replaced_note
            && replaced_note != note
        {
            self.refresh_keyboard_note(replaced_note);
        }
        self.refresh_keyboard_note(note);
    }

    pub fn note_off(&mut self, note: MidiNote) {
        self.note_off_channel(0, note);
    }

    pub fn note_off_channel(&mut self, channel: u8, note: MidiNote) {
        let channel = channel.min(15);
        let sustain = self.channels[channel as usize].sustain;
        for voice in &mut self.voices {
            if voice.active && voice.channel == channel && voice.note == note {
                voice.key_down = false;
                if !sustain {
                    voice.begin_release();
                }
            }
        }
        self.refresh_keyboard_note(note);
    }

    pub fn handle_midi(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } if velocity > 0.0 => self.note_on_channel(channel, note, velocity),
            MidiEvent::NoteOn { channel, note, .. } | MidiEvent::NoteOff { channel, note, .. } => {
                self.note_off_channel(channel, note)
            }
            MidiEvent::PolyPressure {
                channel,
                note,
                value,
            } => {
                let channel = channel.min(15);
                for voice in &mut self.voices {
                    if voice.active && voice.channel == channel && voice.note == note {
                        voice.poly_pressure = value.clamp(0.0, 1.0);
                    }
                }
            }
            MidiEvent::ChannelPressure { channel, value } => {
                self.channels[channel.min(15) as usize].pressure = value.clamp(0.0, 1.0);
            }
            MidiEvent::PitchBend { channel, value } => {
                self.channels[channel.min(15) as usize].bend = value.clamp(-1.0, 1.0);
            }
            MidiEvent::ProgramChange { channel, program } => {
                self.channels[channel.min(15) as usize].program = program.min(127) as f32;
            }
            MidiEvent::ControlChange {
                channel,
                controller,
                value,
            } => self.control_change(channel, controller, value),
            MidiEvent::AllNotesOff { channel } => self.all_notes_off_channel(channel),
            MidiEvent::AllSoundOff => self.all_sound_off(),
        }
    }

    pub fn control_change(&mut self, channel: u8, controller: u8, value: f32) {
        let channel_index = channel.min(15) as usize;
        let controller = controller.min(127) as usize;
        let value = value.clamp(0.0, 1.0);
        self.channels[channel_index].cc[controller] = value;
        if let Some(parameters) = &self.parameters {
            for spec in self.program.instance.program().parameter_specs() {
                if spec.cc_link == Some(controller as u8) {
                    parameters.set_from_midi(
                        spec.index,
                        value,
                        self.software_revisions_at_block_start[spec.index],
                    );
                }
            }
        }
        match controller {
            1 => self.channels[channel_index].modulation = value,
            7 => self.channels[channel_index].volume = value,
            10 => self.channels[channel_index].pan = value * 2.0 - 1.0,
            11 => self.channels[channel_index].expression = value,
            64 => self.set_sustain(channel_index, value >= 0.5),
            120 => self.all_sound_off(),
            123 => self.all_notes_off_channel(channel_index as u8),
            _ => {}
        }
    }

    fn set_sustain(&mut self, channel: usize, enabled: bool) {
        let was_enabled = self.channels[channel].sustain;
        self.channels[channel].sustain = enabled;
        if was_enabled && !enabled {
            for voice in &mut self.voices {
                if voice.active && voice.channel as usize == channel && !voice.key_down {
                    voice.begin_release();
                }
            }
            self.refresh_all_keyboard_notes();
        }
    }

    pub fn all_notes_off_channel(&mut self, channel: u8) {
        let channel = channel.min(15);
        for voice in &mut self.voices {
            if voice.active && voice.channel == channel {
                voice.key_down = false;
                voice.begin_release();
            }
        }
        self.refresh_all_keyboard_notes();
    }

    pub fn all_sound_off(&mut self) {
        for (index, voice) in self.voices.iter_mut().enumerate() {
            voice.active = false;
            self.program.instance.reset_voice(index);
            if let Some(pool) = &self.worker_pool {
                pool.reset_voice(index);
            }
        }
        if let Some(waveform) = &self.waveform {
            waveform.clear_keyboard_note_states();
        }
    }

    /// Compatibility wrapper over the native block pipeline with a block size
    /// of one. Scheduling and state ownership therefore stay identical to
    /// larger host blocks.
    pub fn render_sample(&mut self, input: Inputs) -> (f32, f32) {
        let mut left = [0.0];
        let mut right = [0.0];
        self.render_block_with_ppq_step(&mut left, &mut right, input, 0.0);
        (left[0], right[0])
    }

    fn render_sample_scalar_with_input(
        &mut self,
        mut input: Inputs,
        audio_left: f32,
        audio_right: f32,
    ) -> (f32, f32) {
        input.sr = self.sample_rate;
        if let Some(parameters) = &self.parameters {
            parameters.fill_inputs(&mut input);
        }
        let dt = 1.0 / self.sample_rate;
        let mut out_l = 0.0;
        let mut out_r = 0.0;
        let mut retired_notes = [MidiNote::new(0); MAX_VOICES];
        let mut retired_count = 0;
        let output_mode = self.program.instance.program().note_output_mode();
        let mut outputs = [synth_dsl::Outputs::default(); MAX_VOICES];
        for (voice_index, output) in outputs.iter_mut().enumerate() {
            if !self.voices[voice_index].active {
                continue;
            }
            let voice = &mut self.voices[voice_index];
            let channel = self.channels[voice.channel as usize];
            input.t = voice.t;
            input.l = voice.l;
            input.s = voice.velocity;
            input.note = voice.note.number() as f32;
            input.ch = voice.channel as f32;
            input.bend = channel.bend;
            input.bend_st = channel.bend * channel.bend_range;
            input.freq = voice.note.frequency() * 2.0f32.powf(input.bend_st / 12.0);
            input.mw = channel.modulation;
            input.vol = channel.volume;
            input.midi_pan = channel.pan;
            input.mexpr = channel.expression;
            input.sustain = f32::from(channel.sustain);
            input.pressure = channel.pressure;
            input.poly_pressure = voice.poly_pressure;
            input.program = channel.program;
            input.voice = voice_index as f32;
            input.rand = voice.next_random();
            input.cc = channel.cc;
            *output = self.program.instance.evaluate_note(&input, voice_index);
            self.program.instance.commit_voice(voice_index);
        }
        for (voice_index, &output) in outputs.iter().enumerate() {
            if !self.voices[voice_index].active {
                continue;
            }
            match output_mode {
                NoteOutputMode::Mono => {
                    let (left_gain, right_gain) = equal_power_pan(output.pan);
                    out_l += output.wave * left_gain;
                    out_r += output.wave * right_gain;
                }
                NoteOutputMode::Stereo => {
                    out_l += output.wave_l;
                    out_r += output.wave_r;
                }
            }
            let voice = &mut self.voices[voice_index];
            voice.t += dt;
            if voice.released {
                voice.l += dt;
            }
            if voice.released && voice.l >= output.l_limit.max(0.0) {
                voice.active = false;
                retired_notes[retired_count] = voice.note;
                retired_count += 1;
                self.program.instance.reset_voice(voice_index);
                if let Some(pool) = &self.worker_pool {
                    pool.reset_voice(voice_index);
                }
            }
        }
        out_l += audio_left;
        out_r += audio_right;
        let pre_filter_left = out_l;
        let pre_filter_right = out_r;
        let master = self.channels[0];
        match self.program.instance.program().effect_input_layout() {
            synth_dsl::ChannelLayout::Mono => input.wave = (out_l + out_r) * 0.5,
            synth_dsl::ChannelLayout::Stereo => {
                input.wave_l = out_l;
                input.wave_r = out_r;
            }
        }
        input.mw = master.modulation;
        input.vol = master.volume;
        input.midi_pan = master.pan;
        input.mexpr = master.expression;
        input.sustain = f32::from(master.sustain);
        input.program = master.program;
        input.cc = master.cc;
        self.global_rng = next_rng(self.global_rng);
        input.rand = (self.global_rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let filtered = self.program.instance.evaluate_filter(&input);
        self.program.instance.commit_global();
        match self.program.instance.program().effect_output_layout() {
            synth_dsl::ChannelLayout::Mono => out_l = filtered.wave,
            synth_dsl::ChannelLayout::Stereo => {
                out_l = filtered.wave_l;
                out_r = filtered.wave_r;
            }
        }
        if self.program.instance.program().effect_output_layout() == synth_dsl::ChannelLayout::Mono
        {
            out_r = out_l;
        }
        for note in &retired_notes[..retired_count] {
            self.refresh_keyboard_note(*note);
        }
        let output = (finite_or_zero(out_l), finite_or_zero(out_r));
        if let Some(waveform) = &self.waveform {
            waveform.push(
                pre_filter_left,
                pre_filter_right,
                output.0,
                output.1,
                self.voices.iter().filter(|voice| voice.active).count(),
            );
        }
        output
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32], input: Inputs) {
        assert_eq!(
            left.len(),
            right.len(),
            "stereo buffers must have the same length"
        );
        self.begin_block();
        self.render_block(left, right, input);
    }

    /// Renders one event-free host block segment.
    ///
    /// Callers must apply MIDI state changes before calling this method. Keeping
    /// that boundary explicit lets a host split at sample-accurate MIDI offsets
    /// without re-entering block setup or publishing a new program mid-block.
    pub fn render_block(&mut self, left: &mut [f32], right: &mut [f32], input: Inputs) {
        self.render_block_with_ppq_step(left, right, input, 0.0);
    }

    /// Renders an event-free block while advancing the transport position.
    ///
    /// `ppq_step` is normally `tempo / (60 * sample_rate)`. MIDI is intentionally
    /// not accepted here: the VST adapter splits this method at every event
    /// position before it mutates engine state.
    pub fn render_block_with_ppq_step(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        input: Inputs,
        ppq_step: f32,
    ) {
        assert_eq!(
            left.len(),
            right.len(),
            "stereo buffers must have the same length"
        );
        let block_workers_ready = self
            .worker_pool
            .as_ref()
            .is_some_and(VoiceWorkerPool::is_ready);
        if !block_workers_ready {
            let mut input = input;
            for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
                let audio_left = *left_sample;
                let audio_right = *right_sample;
                (*left_sample, *right_sample) =
                    self.render_sample_scalar_with_input(input, audio_left, audio_right);
                input.ppq += ppq_step;
            }
            return;
        }
        let mut start = 0;
        let mut chunk_input = input;
        while start < left.len() {
            let end = (start + MAX_BLOCK_FRAMES).min(left.len());
            self.render_worker_chunk(
                &mut left[start..end],
                &mut right[start..end],
                chunk_input,
                ppq_step,
            );
            for _ in start..end {
                chunk_input.ppq += ppq_step;
            }
            start = end;
        }
    }

    fn render_worker_chunk(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        base: Inputs,
        ppq_step: f32,
    ) {
        let frames = left.len();
        self.block_audio_left[..frames].copy_from_slice(left);
        self.block_audio_right[..frames].copy_from_slice(right);
        let mut specs = [VoiceBlockSpec::default(); MAX_VOICES];
        let mut spec_count = 0;
        for slot in 0..MAX_VOICES {
            let voice = self.voices[slot];
            if !voice.active {
                continue;
            }
            let channel = self.channels[voice.channel as usize];
            let mut input = base;
            input.sr = self.sample_rate;
            if let Some(parameters) = &self.parameters {
                parameters.fill_inputs(&mut input);
            }
            input.t = voice.t;
            input.l = voice.l;
            input.s = voice.velocity;
            input.note = voice.note.number() as f32;
            input.ch = voice.channel as f32;
            input.bend = channel.bend;
            input.bend_st = channel.bend * channel.bend_range;
            input.freq = voice.note.frequency() * 2.0f32.powf(input.bend_st / 12.0);
            input.mw = channel.modulation;
            input.vol = channel.volume;
            input.midi_pan = channel.pan;
            input.mexpr = channel.expression;
            input.sustain = f32::from(channel.sustain);
            input.pressure = channel.pressure;
            input.poly_pressure = voice.poly_pressure;
            input.program = channel.program;
            input.voice = slot as f32;
            input.cc = channel.cc;
            specs[spec_count] = VoiceBlockSpec {
                input,
                voice_slot: slot,
                released: voice.released,
                rng: voice.rng,
            };
            spec_count += 1;
        }
        let mut results = [VoiceBlockResult::default(); MAX_VOICES];
        let mode = self.program.instance.program().note_output_mode();
        self.worker_pool
            .as_ref()
            .expect("parallel path requires workers")
            .evaluate_block(
                &specs[..spec_count],
                ppq_step,
                mode,
                left,
                right,
                &mut results,
            );
        for frame in 0..frames {
            left[frame] += self.block_audio_left[frame];
            right[frame] += self.block_audio_right[frame];
        }
        self.block_mix_left[..frames].copy_from_slice(left);
        self.block_mix_right[..frames].copy_from_slice(right);
        let mut retired = [MidiNote::new(0); MAX_VOICES];
        let mut retired_count = 0;
        for spec in &specs[..spec_count] {
            let result = results[spec.voice_slot];
            let voice = &mut self.voices[spec.voice_slot];
            voice.t = result.t;
            voice.l = result.l;
            voice.rng = result.rng;
            if result.became_inactive {
                voice.active = false;
                retired[retired_count] = voice.note;
                retired_count += 1;
                self.program.instance.reset_voice(spec.voice_slot);
                self.worker_pool
                    .as_ref()
                    .unwrap()
                    .reset_voice(spec.voice_slot);
            }
        }
        let master = self.channels[0];
        let mut filter_ppq = base.ppq;
        for frame in 0..frames {
            let mut input = base;
            input.sr = self.sample_rate;
            input.ppq = filter_ppq;
            if let Some(parameters) = &self.parameters {
                parameters.fill_inputs(&mut input);
            }
            match self.program.instance.program().effect_input_layout() {
                synth_dsl::ChannelLayout::Mono => {
                    input.wave = (self.block_mix_left[frame] + self.block_mix_right[frame]) * 0.5
                }
                synth_dsl::ChannelLayout::Stereo => {
                    input.wave_l = left[frame];
                    input.wave_r = right[frame];
                }
            }
            input.mw = master.modulation;
            input.vol = master.volume;
            input.midi_pan = master.pan;
            input.mexpr = master.expression;
            input.sustain = f32::from(master.sustain);
            input.program = master.program;
            input.cc = master.cc;
            self.global_rng = next_rng(self.global_rng);
            input.rand = (self.global_rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
            self.block_inputs[frame] = input;
            filter_ppq += ppq_step;
        }
        self.program.instance.evaluate_filter_block(
            &self.block_inputs[..frames],
            &mut self.block_outputs[..frames],
        );
        for frame in 0..frames {
            match self.program.instance.program().effect_output_layout() {
                synth_dsl::ChannelLayout::Mono => {
                    left[frame] = finite_or_zero(self.block_outputs[frame].wave);
                    right[frame] = left[frame];
                }
                synth_dsl::ChannelLayout::Stereo => {
                    left[frame] = finite_or_zero(self.block_outputs[frame].wave_l);
                    right[frame] = finite_or_zero(self.block_outputs[frame].wave_r);
                }
            }
            if let Some(waveform) = &self.waveform {
                let active = specs[..spec_count]
                    .iter()
                    .filter(|spec| {
                        let result = results[spec.voice_slot];
                        !result.became_inactive || result.rendered_frames > frame + 1
                    })
                    .count();
                waveform.push(
                    self.block_mix_left[frame],
                    self.block_mix_right[frame],
                    left[frame],
                    right[frame],
                    active,
                );
            }
        }
        for note in &retired[..retired_count] {
            self.refresh_keyboard_note(*note);
        }
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }

    fn refresh_keyboard_note(&self, note: MidiNote) {
        let Some(waveform) = &self.waveform else {
            return;
        };
        let mut pressed_velocity: f32 = 0.0;
        let mut released_velocity: f32 = 0.0;
        for voice in &self.voices {
            if !voice.active || voice.note != note {
                continue;
            }
            if voice.released {
                released_velocity = released_velocity.max(voice.velocity);
            } else if voice.key_down {
                pressed_velocity = pressed_velocity.max(voice.velocity);
            }
        }
        waveform.set_keyboard_note_state(note, pressed_velocity, released_velocity);
    }

    fn refresh_all_keyboard_notes(&self) {
        if self.waveform.is_none() {
            return;
        }
        for note in 0..128 {
            self.refresh_keyboard_note(MidiNote::new(note));
        }
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn equal_power_pan(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

fn seed_voice(channel: u8, note: MidiNote, age: u64) -> u32 {
    let mixed = (age as u32)
        ^ ((age >> 32) as u32).rotate_left(11)
        ^ (u32::from(channel) << 24)
        ^ (u32::from(note.number()) << 8)
        ^ 0x9E37_79B9;
    mixed.max(1)
}

fn next_rng(mut value: u32) -> u32 {
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    value.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_dsl::Compiler;

    fn program(source: &str) -> Program {
        let note_layout = if source.contains("out.wave_l") || source.contains("out.wave_r") {
            "stereo"
        } else {
            "mono"
        };
        let effect_layout = if source.contains("in.wave_l") || source.contains("in.wave_r") {
            "stereo"
        } else {
            "mono"
        };
        let effect = if source.contains("fn effect") {
            format!("effect.in.layout = {effect_layout}\neffect.out.layout = {effect_layout}\n")
        } else {
            String::new()
        };
        Compiler::new()
            .compile(&format!(
                "note.out.layout = {note_layout}\n{effect}{source}"
            ))
            .unwrap()
    }

    fn engine() -> SynthEngine {
        SynthEngine::new(
            48_000.0,
            program(
                "fn note(in, p) -> out {\nout.wave = sin(TAU * in.freq * in.t) * in.s * exp(-5*in.l)\nout.l_limit = 0.1\n}",
            ),
        )
    }

    #[test]
    fn renders_stereo_and_handles_note_lifecycle() {
        let mut engine = engine();
        engine.note_on(MidiNote::new(60), 1.0);
        let mut left = [0.0; 128];
        let mut right = [0.0; 128];
        engine.render(&mut left, &mut right, Inputs::default());
        assert_eq!(engine.active_voice_count(), 1);
        assert_eq!(left, right);
        engine.note_off(MidiNote::new(60));
        let mut left = [0.0; 5_000];
        let mut right = [0.0; 5_000];
        engine.render(&mut left, &mut right, Inputs::default());
        assert_eq!(engine.active_voice_count(), 0);
    }

    #[test]
    fn sustain_defers_release() {
        let mut engine = engine();
        engine.note_on(MidiNote::new(60), 1.0);
        engine.control_change(0, 64, 1.0);
        engine.note_off(MidiNote::new(60));
        let mut left = [0.0; 5_000];
        let mut right = [0.0; 5_000];
        engine.render(&mut left, &mut right, Inputs::default());
        assert_eq!(engine.active_voice_count(), 1);
        engine.control_change(0, 64, 0.0);
        engine.render(&mut left, &mut right, Inputs::default());
        assert_eq!(engine.active_voice_count(), 0);
    }

    #[test]
    fn program_swap_retires_old_program_off_audio_path() {
        let exchange = Arc::new(ProgramExchange::new(4));
        let mut engine = SynthEngine::with_exchange(
            48_000.0,
            program("fn note(in, p) -> out {\nout.wave = 0\nout.l_limit = 1\n}"),
            exchange.clone(),
        );
        engine.note_on(MidiNote::new(60), 1.0);
        exchange.publish(program(
            "fn note(in, p) -> out {\nout.wave = 1\nout.l_limit = 1\n}",
        ));
        assert!(engine.begin_block());
        let (left, right) = engine.render_sample(Inputs::default());
        assert!((left - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        assert_eq!(left, right);
        engine.begin_block();
        assert_eq!(exchange.collect_retired(), 1);
    }

    #[test]
    fn exposes_midi_program_to_the_dsl() {
        let mut engine = SynthEngine::new(
            48_000.0,
            program("fn note(in, p) -> out {\nout.wave = in.program / 127\nout.l_limit = 0.1\n}"),
        );
        engine.handle_midi(MidiEvent::ProgramChange {
            channel: 2,
            program: 127,
        });
        engine.note_on_channel(2, MidiNote::new(60), 1.0);
        let (left, right) = engine.render_sample(Inputs::default());
        assert!((left - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        assert_eq!(left, right);
    }

    #[test]
    fn shared_user_parameters_and_waveform_monitor_are_audio_safe() {
        let program = program(
            "p.gain = param(0.5, 0, 2, 0.01)\nfn note(in, p) -> out {\nout.wave = p.gain\nout.l_limit = 1\n}",
        );
        let parameters = Arc::new(UserParameterStore::new(program.parameter_specs()));
        let waveform = Arc::new(WaveformMonitor::new(48_000.0));
        let mut engine = SynthEngine::new(48_000.0, program);
        engine.attach_runtime_state(parameters.clone(), waveform.clone());
        parameters.set_normalized(0, 1.0);
        engine.note_on(MidiNote::new(60), 1.0);
        let (left, right) = engine.render_sample(Inputs::default());
        assert!((left - 2.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        assert_eq!(left, right);
        assert_eq!(waveform.active_voice_count(), 1);
        assert_eq!(waveform.read_recent(true, 1), (vec![left], vec![right]));
    }

    #[test]
    fn waveform_monitor_tracks_release_per_note_with_velocity() {
        let mut engine = engine();
        let parameters = Arc::new(UserParameterStore::new(&[]));
        let waveform = Arc::new(WaveformMonitor::new(48_000.0));
        engine.attach_runtime_state(parameters, waveform.clone());
        engine.note_on(MidiNote::new(60), 0.25);
        engine.note_on(MidiNote::new(64), 0.8);
        engine.note_off(MidiNote::new(60));

        let states = waveform.keyboard_note_states();
        let released = states.iter().find(|state| state.note == 60).unwrap();
        assert_eq!(released.pressed_velocity, 0.0);
        assert!((released.released_velocity - 0.25).abs() < 0.01);
        let pressed = states.iter().find(|state| state.note == 64).unwrap();
        assert!((pressed.pressed_velocity - 0.8).abs() < 0.01);
        assert_eq!(pressed.released_velocity, 0.0);
    }

    #[test]
    fn renders_true_stereo_then_post_mix_filter() {
        let source = "fn note(in, p) -> out {\nout.wave_l = 1\nout.wave_r = 2\nout.l_limit = 1\n}\nfn effect(in, p) -> out {\nout.wave_l = in.wave_l * 0.5\nout.wave_r = in.wave_r * 0.5\n}";
        let mut engine = SynthEngine::new(48_000.0, program(source));
        engine.note_on(MidiNote::new(60), 1.0);
        assert_eq!(engine.render_sample(Inputs::default()), (0.5, 1.0));
    }

    #[test]
    fn effect_receives_stereo_note_mix_plus_audio_input() {
        let source = "note.out.layout = mono\neffect.in.layout = stereo\neffect.out.layout = stereo\nfn note(in, p) -> out {\nout.wave = 1\nout.pan = 0\nout.l_limit = 1\n}\nfn effect(in, p) -> out {\nout.wave_l = in.wave_l\nout.wave_r = in.wave_r\n}";
        let mut engine = SynthEngine::new(48_000.0, Compiler::new().compile(source).unwrap());
        engine.note_on(MidiNote::new(60), 1.0);
        let mut left = [2.0; 4];
        let mut right = [3.0; 4];
        engine.render(&mut left, &mut right, Inputs::default());
        let note_gain = std::f32::consts::FRAC_1_SQRT_2;
        assert_audio_close(&left, &[2.0 + note_gain; 4], 1.0e-6);
        assert_audio_close(&right, &[3.0 + note_gain; 4], 1.0e-6);
    }

    #[test]
    fn block_render_advances_transport_for_each_frame() {
        let mut engine = SynthEngine::new(
            48_000.0,
            program("fn note(in, p) -> out {\nout.wave = in.ppq\nout.l_limit = 1\n}"),
        );
        engine.note_on(MidiNote::new(60), 1.0);
        let mut left = [0.0; 3];
        let mut right = [0.0; 3];

        engine.begin_block();
        engine.render_block_with_ppq_step(
            &mut left,
            &mut right,
            Inputs {
                ppq: 2.0,
                ..Inputs::default()
            },
            0.25,
        );

        let gain = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(left, [2.0 * gain, 2.25 * gain, 2.5 * gain]);
        assert_eq!(left, right);
    }

    #[test]
    fn voice_storage_is_isolated_for_retriggered_same_channel_note() {
        let source = "fn note(in, p) -> out {\nf32 voice count = 0\ncount = count + 1\nout.wave = count\nout.l_limit = 1\n}";
        let mut engine = SynthEngine::new(48_000.0, program(source));
        engine.note_on_channel(2, MidiNote::new(60), 1.0);
        engine.note_on_channel(2, MidiNote::new(60), 1.0);
        let (left, right) = engine.render_sample(Inputs::default());
        let expected = 2.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((left - expected).abs() < 0.0001);
        assert_eq!(left, right);
    }

    #[test]
    fn exact_storage_survives_hot_reload() {
        let source = "fn note(in, p) -> out {\nf32 voice count = 0\ncount = count + 1\nout.wave = count\nout.l_limit = 1\n}";
        let exchange = Arc::new(ProgramExchange::new(4));
        let mut engine = SynthEngine::with_exchange(48_000.0, program(source), exchange.clone());
        engine.note_on(MidiNote::new(60), 1.0);
        let first = engine.render_sample(Inputs::default()).0;
        exchange.publish(program(source));
        assert!(engine.begin_block());
        let second = engine.render_sample(Inputs::default()).0;
        assert!(second > first);
    }

    #[test]
    fn software_parameter_change_wins_over_linked_cc_in_same_block() {
        let source = "p.gain = param(0, 0, 1, 0.01, 74)\nfn note(in, p) -> out {\nout.wave = p.gain\nout.l_limit = 1\n}";
        let program = program(source);
        let parameters = Arc::new(UserParameterStore::new(program.parameter_specs()));
        let waveform = Arc::new(WaveformMonitor::new(48_000.0));
        let mut engine = SynthEngine::new(48_000.0, program);
        engine.attach_runtime_state(parameters.clone(), waveform);
        engine.note_on(MidiNote::new(60), 1.0);
        engine.begin_block();
        parameters.set_normalized(0, 0.25);
        engine.control_change(0, 74, 1.0);
        assert!((parameters.get_normalized(0) - 0.25).abs() < 0.001);
        engine.begin_block();
        engine.control_change(0, 74, 1.0);
        assert_eq!(parameters.get_normalized(0), 1.0);
    }

    #[test]
    fn worker_pool_renders_parallel_safe_polyphony() {
        let source = "fn note(in, p) -> out {\nf32 voice last = 0\nlast = in.voice\nf32 voice phase = 0\nphase = fract(phase + in.freq / in.sr)\nout.wave = sin(TAU * phase) * in.s\nout.l_limit = 1\n}";
        let mut engine = SynthEngine::new(48_000.0, program(source));
        for note in 48..56 {
            engine.note_on(MidiNote::new(note), 0.7);
        }
        let mut left = [0.0; 512];
        let mut right = [0.0; 512];
        engine.render(&mut left, &mut right, Inputs::default());
        assert_eq!(engine.active_voice_count(), 8);
        assert!(left.iter().all(|sample| sample.is_finite()));
        assert!(right.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn worker_pool_handles_voice_ringbuf_in_note() {
        let source = "fn note(in, p) -> out {\nRingBuf<f32, 20ms> voice feedback\nold = feedback.peek_linear(4ms)\nfeedback = sin(TAU * in.freq * in.t) + old * 0.3\nout.wave = feedback * in.s\nout.l_limit = 1\n}";
        let program = program(source);
        assert!(program.parallel_voice_safe());
        let mut engine = SynthEngine::new(48_000.0, program);
        for note in 48..56 {
            engine.note_on(MidiNote::new(note), 0.7);
        }
        let mut left = [0.0; 512];
        let mut right = [0.0; 512];
        engine.render(&mut left, &mut right, Inputs::default());
        assert!(left.iter().all(|sample| sample.is_finite()));
        assert!(right.iter().all(|sample| sample.is_finite()));
    }

    fn assert_audio_close(actual: &[f32], expected: &[f32], tolerance: f32) {
        assert_eq!(actual.len(), expected.len());
        for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (actual - expected).abs() <= tolerance,
                "sample {index}: actual={actual}, expected={expected}"
            );
        }
    }

    fn stateful_block_program() -> Program {
        program(
            "fn note(in, p) -> out {\n\
             f32 voice phase = 0\n\
             RingBuf<f32, 4> voice history\n\
             old = history\n\
             phase = fract(phase + in.freq / in.sr)\n\
             current = sin(TAU * phase)\n\
             history = current\n\
             out.wave = (current + old * 0.25 + in.ppq * 0.001) * in.s\n\
             out.l_limit = 1\n\
             }\n\
             fn effect(in, p) -> out {\n\
             f32 global drive = 0\n\
             RingBuf<f32, 3> global history\n\
             old = history\n\
             history = in.wave\n\
             drive = drive + 0.0001\n\
             out.wave = in.wave + old * 0.1 + drive\n\
             }",
        )
    }

    #[test]
    fn block_size_one_matches_chunked_native_blocks() {
        let compiled = stateful_block_program();
        let mut sample_engine = SynthEngine::new(48_000.0, compiled.clone());
        let mut block_engine = SynthEngine::new(48_000.0, compiled);
        for note in [48, 55, 60, 67, 72] {
            sample_engine.note_on(MidiNote::new(note), 0.7);
            block_engine.note_on(MidiNote::new(note), 0.7);
        }

        let frames = 513;
        let ppq_step = 0.000_731;
        let mut sample_left = vec![0.0; frames];
        let mut sample_right = vec![0.0; frames];
        let mut input = Inputs {
            ppq: 1.25,
            ..Inputs::default()
        };
        for frame in 0..frames {
            (sample_left[frame], sample_right[frame]) = sample_engine.render_sample(input);
            input.ppq += ppq_step;
        }

        let mut block_left = vec![0.0; frames];
        let mut block_right = vec![0.0; frames];
        block_engine.render_block_with_ppq_step(
            &mut block_left,
            &mut block_right,
            Inputs {
                ppq: 1.25,
                ..Inputs::default()
            },
            ppq_step,
        );

        assert_audio_close(&block_left, &sample_left, 2.0e-5);
        assert_audio_close(&block_right, &sample_right, 2.0e-5);
    }

    #[test]
    fn block_workers_match_serial_state_and_ring_semantics() {
        let compiled = stateful_block_program();
        let mut worker = SynthEngine::new_with_worker_setting(48_000.0, compiled.clone(), true);
        let mut serial = SynthEngine::new_with_worker_setting(48_000.0, compiled, false);
        for note in 36..52 {
            worker.note_on(MidiNote::new(note), 0.6);
            serial.note_on(MidiNote::new(note), 0.6);
        }
        assert!(
            (1..=worker::MAX_WORKERS)
                .contains(&worker.worker_pool.as_ref().unwrap().worker_count())
        );

        let mut worker_left = [0.0; 511];
        let mut worker_right = [0.0; 511];
        let mut serial_left = [0.0; 511];
        let mut serial_right = [0.0; 511];
        let input = Inputs {
            ppq: 3.0,
            ..Inputs::default()
        };
        worker.render_block_with_ppq_step(&mut worker_left, &mut worker_right, input, 0.000_347);
        serial.render_block_with_ppq_step(&mut serial_left, &mut serial_right, input, 0.000_347);

        assert_audio_close(&worker_left, &serial_left, 3.0e-5);
        assert_audio_close(&worker_right, &serial_right, 3.0e-5);
    }

    #[test]
    fn released_voice_becomes_silent_inside_the_block() {
        let mut engine = SynthEngine::new(
            48_000.0,
            program("fn note(in, p) -> out {\nout.wave = 1\nout.l_limit = 2.5 / in.sr\n}"),
        );
        engine.note_on(MidiNote::new(60), 1.0);
        engine.note_off(MidiNote::new(60));
        let mut left = [0.0; 8];
        let mut right = [0.0; 8];
        engine.render_block(&mut left, &mut right, Inputs::default());

        assert_audio_close(&left[..3], &[std::f32::consts::FRAC_1_SQRT_2; 3], 1.0e-6);
        assert_eq!(left[3..], [0.0; 5]);
        assert_eq!(left, right);
        assert_eq!(engine.active_voice_count(), 0);
    }

    #[test]
    fn offset_zero_and_same_boundary_events_apply_before_rendering() {
        let mut engine = SynthEngine::new(
            48_000.0,
            program("fn note(in, p) -> out {\nout.wave = in.s\nout.l_limit = 2 / in.sr\n}"),
        );
        engine.handle_midi(MidiEvent::NoteOn {
            channel: 0,
            note: MidiNote::new(60),
            velocity: 1.0,
        });
        engine.handle_midi(MidiEvent::NoteOff {
            channel: 0,
            note: MidiNote::new(60),
            velocity: 0.0,
        });
        let mut left = [0.0; 4];
        let mut right = [0.0; 4];
        engine.render_block(&mut left, &mut right, Inputs::default());

        assert_audio_close(&left[..2], &[std::f32::consts::FRAC_1_SQRT_2; 2], 1.0e-6);
        assert_eq!(left[2..], [0.0; 2]);
    }

    #[test]
    fn midi_event_boundary_does_not_leak_across_segments() {
        let mut engine = SynthEngine::new(
            48_000.0,
            program("fn note(in, p) -> out {\nout.wave = in.note\nout.l_limit = 1\n}"),
        );
        engine.note_on(MidiNote::new(60), 1.0);
        let mut before_left = [0.0; 5];
        let mut before_right = [0.0; 5];
        engine.render_block(&mut before_left, &mut before_right, Inputs::default());
        engine.note_on(MidiNote::new(64), 1.0);
        let mut after_left = [0.0; 5];
        let mut after_right = [0.0; 5];
        engine.render_block(&mut after_left, &mut after_right, Inputs::default());

        let gain = std::f32::consts::FRAC_1_SQRT_2;
        assert_audio_close(&before_left, &[60.0 * gain; 5], 1.0e-5);
        assert_audio_close(&after_left, &[(60.0 + 64.0) * gain; 5], 1.0e-5);
    }

    #[test]
    fn voice_steal_reuses_exactly_one_voice_slot() {
        let mut engine = engine();
        for note in 0..=MAX_VOICES as u8 {
            engine.note_on(MidiNote::new(note), 1.0);
        }

        assert_eq!(engine.active_voice_count(), MAX_VOICES);
        assert_eq!(engine.voices[0].note, MidiNote::new(MAX_VOICES as u8));
        assert!(
            engine
                .voices
                .iter()
                .all(|voice| voice.note != MidiNote::new(0))
        );
    }

    #[test]
    fn global_filter_state_and_ring_advance_once_per_sample_without_voices() {
        let mut engine = SynthEngine::new(
            48_000.0,
            program(
                "fn note(in, p) -> out {\nout.wave = 0\nout.l_limit = 1\n}\n\
                 fn effect(in, p) -> out {\n\
                 f32 global count = 0\n\
                 RingBuf<f32, 2> global delay\n\
                 old = delay\n\
                 delay = count + 1\n\
                 count = count + 1\n\
                 out.wave = count + old\n\
                 }",
            ),
        );
        let mut left = [0.0; 4];
        let mut right = [0.0; 4];
        engine.render_block(&mut left, &mut right, Inputs::default());

        assert_eq!(left, [1.0, 2.0, 4.0, 6.0]);
        assert_eq!(left, right);
    }
}
