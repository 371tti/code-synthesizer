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

use midi::{ChannelState, NoteDomain, Voice};
pub use midi::{MidiEvent, MidiNote};
pub use program::ProgramExchange;
use program::RuntimeProgram;
pub use shared::{KeyboardNoteState, UserParameterStore, WaveformMonitor};
use std::sync::Arc;
use synth_dsl::{Inputs, MAX_USER_PARAMETERS, NoteOutputMode, Program};
use worker::{VoiceJob, VoiceWorkerPool, WORKER_THRESHOLD};

pub const MAX_VOICES: usize = 64;
pub const MIDI_CHANNELS: usize = 16;
pub const MIDI_CC_COUNT: usize = 128;
pub const WAVEFORM_CAPACITY: usize = 4_096;
/// 1 sampleごとのqueue同期は直列JITより遅いため、block workerへ移行するまで無効です。
pub const VOICE_WORKER_ENABLED: bool = false;

pub struct SynthEngine {
    sample_rate: f32,
    worker_pool: Option<VoiceWorkerPool>,
    program: Box<RuntimeProgram>,
    exchange: Option<Arc<ProgramExchange>>,
    deferred_retire: Option<Box<RuntimeProgram>>,
    voices: [Voice; MAX_VOICES],
    note_domains: [NoteDomain; MAX_VOICES],
    channels: [ChannelState; MIDI_CHANNELS],
    age: u64,
    global_rng: u32,
    software_revisions_at_block_start: [u32; MAX_USER_PARAMETERS],
    parameters: Option<Arc<UserParameterStore>>,
    waveform: Option<Arc<WaveformMonitor>>,
}

impl SynthEngine {
    pub fn new(sample_rate: f32, program: Program) -> Self {
        let sample_rate = sanitize_sample_rate(sample_rate);
        let program = Box::new(RuntimeProgram::from_program(program, sample_rate));
        let worker_pool = VoiceWorkerPool::new(&program.instance);
        Self {
            sample_rate,
            worker_pool,
            program,
            exchange: None,
            deferred_retire: None,
            voices: [Voice::default(); MAX_VOICES],
            note_domains: [NoteDomain::default(); MAX_VOICES],
            channels: [ChannelState::default(); MIDI_CHANNELS],
            age: 0,
            global_rng: 0xA341_316C,
            software_revisions_at_block_start: [0; MAX_USER_PARAMETERS],
            parameters: None,
            waveform: None,
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
        *self.program = RuntimeProgram::from_program(program, self.sample_rate);
        if let Some(pool) = &mut self.worker_pool {
            pool.configure(&self.program.instance);
        }
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

    /// Intended for setup/tests. Hot reload during processing should use
    /// [`ProgramExchange`] so the old program is retired off the audio thread.
    pub fn set_program(&mut self, program: Program) {
        *self.program = RuntimeProgram::from_program(program, self.sample_rate);
        if let Some(pool) = &mut self.worker_pool {
            pool.configure(&self.program.instance);
        }
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
            pool.configure(&self.program.instance);
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
        if self.voices[index].active {
            let old_note_slot = self.voices[index].note_slot;
            self.release_note_domain(old_note_slot);
        }
        self.program.instance.reset_voice(index);
        let note_slot = self.acquire_note_domain(channel, note);
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
            note_slot,
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
        }
        for (index, domain) in self.note_domains.iter_mut().enumerate() {
            if domain.active {
                self.program.instance.reset_note(index);
                *domain = NoteDomain::default();
            }
        }
        if let Some(waveform) = &self.waveform {
            waveform.clear_keyboard_note_states();
        }
    }

    fn acquire_note_domain(&mut self, channel: u8, note: MidiNote) -> usize {
        if let Some(index) = self
            .note_domains
            .iter()
            .position(|domain| domain.active && domain.channel == channel && domain.note == note)
        {
            self.note_domains[index].voices = self.note_domains[index].voices.saturating_add(1);
            return index;
        }
        let index = self
            .note_domains
            .iter()
            .position(|domain| !domain.active)
            .expect("active note domains cannot exceed active voices");
        self.program.instance.reset_note(index);
        self.note_domains[index] = NoteDomain {
            active: true,
            channel,
            note,
            voices: 1,
        };
        index
    }

    fn release_note_domain(&mut self, index: usize) {
        let domain = &mut self.note_domains[index];
        domain.voices = domain.voices.saturating_sub(1);
        if domain.voices == 0 {
            self.program.instance.reset_note(index);
            *domain = NoteDomain::default();
        }
    }

    pub fn render_sample(&mut self, mut input: Inputs) -> (f32, f32) {
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
        let parallel = self
            .worker_pool
            .as_ref()
            .is_some_and(VoiceWorkerPool::is_ready)
            && self.program.instance.program().parallel_voice_safe()
            && self.active_voice_count() >= WORKER_THRESHOLD;
        let mut jobs = [VoiceJob {
            input: Inputs::default(),
            voice_slot: 0,
            note_slot: 0,
        }; MAX_VOICES];
        let mut outputs = [synth_dsl::Outputs::default(); MAX_VOICES];
        let mut job_count = 0;
        for voice_index in 0..MAX_VOICES {
            if !self.voices[voice_index].active {
                continue;
            }
            let voice = &mut self.voices[voice_index];
            let note_slot = voice.note_slot;
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
            if parallel {
                jobs[job_count] = VoiceJob {
                    input,
                    voice_slot: voice_index,
                    note_slot,
                };
                job_count += 1;
            } else {
                outputs[voice_index] =
                    self.program
                        .instance
                        .evaluate_note(&input, voice_index, note_slot);
                self.program.instance.commit_voice(voice_index);
            }
        }
        if parallel && let Some(pool) = &self.worker_pool {
            pool.evaluate(&jobs[..job_count], &mut outputs);
        }
        for voice_index in 0..MAX_VOICES {
            if !self.voices[voice_index].active {
                continue;
            }
            let note_slot = self.voices[voice_index].note_slot;
            let output = outputs[voice_index];
            match output_mode {
                NoteOutputMode::Mono => {
                    let gain_l = ((1.0 - output.pan) * 0.5).sqrt();
                    let gain_r = ((1.0 + output.pan) * 0.5).sqrt();
                    out_l += output.wave * gain_l;
                    out_r += output.wave * gain_r;
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
                self.release_note_domain(note_slot);
            }
        }
        if !parallel {
            for note_slot in 0..MAX_VOICES {
                if self.note_domains[note_slot].active {
                    self.program.instance.commit_note(note_slot);
                }
            }
        }
        let master = self.channels[0];
        input.wave_l = out_l;
        input.wave_r = out_r;
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
        out_l = filtered.wave_l;
        out_r = filtered.wave_r;
        for note in &retired_notes[..retired_count] {
            self.refresh_keyboard_note(*note);
        }
        let output = (finite_or_zero(out_l), finite_or_zero(out_r));
        if let Some(waveform) = &self.waveform {
            waveform.push(
                (output.0 + output.1) * 0.5,
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
        for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
            (*left_sample, *right_sample) = self.render_sample(input);
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

fn sanitize_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() && sample_rate >= 1_000.0 {
        sample_rate
    } else {
        48_000.0
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
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
        Compiler::new().compile(source).unwrap()
    }

    fn engine() -> SynthEngine {
        SynthEngine::new(
            48_000.0,
            program(
                "fn note(in, p) -> out {\nout.wave = sin(TAU * in.freq * in.t) * in.s * exp(-5*in.l)\nout.pan = 0\nout.l_limit = 0.1\n}",
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
        assert_eq!(waveform.read_recent(1), vec![left]);
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
        let source = "fn note(in, p) -> out {\nout.wave_l = 1\nout.wave_r = 2\nout.l_limit = 1\n}\nfn filter(in, p) -> out {\nout.wave_l = in.wave_l * 0.5\nout.wave_r = in.wave_r * 0.5\n}";
        let mut engine = SynthEngine::new(48_000.0, program(source));
        engine.note_on(MidiNote::new(60), 1.0);
        assert_eq!(engine.render_sample(Inputs::default()), (0.5, 1.0));
    }

    #[test]
    fn note_storage_is_shared_by_retriggered_same_channel_note() {
        let source = "fn note(in, p) -> out {\nf32 note count = 0\ncount = count + 1\nout.wave = count\nout.l_limit = 1\n}";
        let mut engine = SynthEngine::new(48_000.0, program(source));
        engine.note_on_channel(2, MidiNote::new(60), 1.0);
        engine.note_on_channel(2, MidiNote::new(60), 1.0);
        let (left, right) = engine.render_sample(Inputs::default());
        let expected = 3.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((left - expected).abs() < 0.0001);
        assert_eq!(left, right);
    }

    #[test]
    fn exact_storage_survives_hot_reload() {
        let source = "fn note(in, p) -> out {\nf32 global count = 0\ncount = count + 1\nout.wave = count\nout.l_limit = 1\n}";
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
        let source = "fn note(in, p) -> out {\nf32 global last = 0\nlast = in.voice\nf32 voice phase = 0\nphase = fract(phase + in.freq / in.sr)\nout.wave = sin(TAU * phase) * in.s\nout.l_limit = 1\n}";
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
    fn worker_pool_shards_global_ringbuf_in_note() {
        let source = "fn note(in, p) -> out {\nRingBuf<f32, 20ms> global feedback\nold = feedback.peek_linear(4ms)\nfeedback = sin(TAU * in.freq * in.t) + old * 0.3\nout.wave = feedback * in.s\nout.l_limit = 1\n}";
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
}
