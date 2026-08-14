//! WebView/Monaco editor, IPC model, presets, and waveform preview.
//!
//! [`UiModel`] is shared with the VST3 adapter. Compilation, JSON handling,
//! preview generation, and program retirement all happen on the UI thread.
//! The audio thread only touches the bounded lock-free queues.

use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
#[cfg(target_os = "windows")]
use std::io::Write;
#[cfg(target_os = "windows")]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use synth_core::{MidiEvent, MidiNote, ProgramExchange, UserParameterStore, WaveformMonitor};
use synth_dsl::{CompileError, Compiler, Inputs, ParameterSpec, Program};

pub const DEFAULT_SOURCE: &str = include_str!("../../../presets/pure-sine.synth");

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Preset {
    pub name: &'static str,
    pub source: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Pure Sine",
        source: DEFAULT_SOURCE,
    },
    Preset {
        name: "Parameter Guide",
        source: include_str!("../../../presets/parameter-guide.synth"),
    },
    Preset {
        name: "FM Bell",
        source: include_str!("../../../presets/fm-bell.synth"),
    },
    Preset {
        name: "Warm Organ",
        source: include_str!("../../../presets/warm-organ.synth"),
    },
    Preset {
        name: "Soft Pad",
        source: include_str!("../../../presets/soft-pad.synth"),
    },
    Preset {
        name: "MIDI Reactive",
        source: include_str!("../../../presets/midi-reactive.synth"),
    },
    Preset {
        name: "Velocity Piano",
        source: include_str!("../../../presets/velocity-piano.synth"),
    },
    Preset {
        name: "MPE Lead",
        source: include_str!("../../../presets/mpe-lead.synth"),
    },
    Preset {
        name: "Expressive Strings",
        source: include_str!("../../../presets/expressive-strings.synth"),
    },
    Preset {
        name: "CC74 Pluck",
        source: include_str!("../../../presets/cc74-pluck.synth"),
    },
    Preset {
        name: "MIDI Bass",
        source: include_str!("../../../presets/midi-bass.synth"),
    },
    Preset {
        name: "Motion Pad",
        source: include_str!("../../../presets/motion-pad.synth"),
    },
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileStatus {
    pub ok: bool,
    pub message: String,
    pub warnings: Vec<String>,
    pub line: usize,
    pub column: usize,
    pub hint: Option<String>,
    pub generation: u64,
}

impl CompileStatus {
    fn success(generation: u64, warnings: &[String]) -> Self {
        Self {
            ok: true,
            message: "Compiled".into(),
            warnings: warnings.to_vec(),
            line: 0,
            column: 0,
            hint: None,
            generation,
        }
    }

    fn error(error: &CompileError, generation: u64) -> Self {
        Self {
            ok: false,
            message: error.message.clone(),
            warnings: Vec::new(),
            line: error.line,
            column: error.column,
            hint: error.hint.clone(),
            generation,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSnapshot {
    pub source: String,
    pub selected_preset: String,
    pub status: CompileStatus,
    pub preview_note: u8,
    pub preview_velocity: f32,
    pub presets: Vec<&'static str>,
    pub sample_rate: f32,
    pub active_voices: usize,
    pub active_notes: Vec<UiMidiNote>,
    pub release_notes: Vec<UiMidiNote>,
    pub parameters: Vec<UiParameter>,
    pub controls: Vec<ControlLayout>,
    pub mode: UiMode,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMidiNote {
    pub note: u8,
    pub velocity: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiParameter {
    pub index: usize,
    pub name: String,
    pub label: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub normalized: f32,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UiMode {
    #[default]
    Editor,
    Play,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlLayout {
    pub name: String,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformPreview {
    pub samples: Vec<f32>,
    pub frequency: f32,
    pub sample_rate: f32,
    pub active_voices: usize,
    pub live: bool,
}

struct UiState {
    source: String,
    active_source: String,
    selected_preset: String,
    status: CompileStatus,
    program: Program,
    preview_note: u8,
    preview_velocity: f32,
    controls: Vec<ControlLayout>,
    mode: UiMode,
}

pub struct UiModel {
    state: Mutex<UiState>,
    exchange: Arc<ProgramExchange>,
    midi: crossbeam_queue::ArrayQueue<MidiEvent>,
    parameters: Arc<UserParameterStore>,
    waveform: Arc<WaveformMonitor>,
    midi_preview: MidiPreview,
}

/// ロック不要でオーディオスレッドから更新できる、チャンネル別ノート保持状態。
struct MidiPreview {
    channels_by_note: [AtomicU16; 128],
    velocity_by_note: [AtomicU8; 128],
}

impl MidiPreview {
    fn new() -> Self {
        Self {
            channels_by_note: std::array::from_fn(|_| AtomicU16::new(0)),
            velocity_by_note: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }

    fn observe(&self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } if velocity > 0.0 => {
                let index = note.number() as usize;
                self.channels_by_note[index].fetch_or(1 << channel.min(15), Ordering::Relaxed);
                self.velocity_by_note[index].store(
                    (velocity.clamp(0.0, 1.0) * 127.0).round() as u8,
                    Ordering::Relaxed,
                );
            }
            MidiEvent::NoteOn { channel, note, .. } | MidiEvent::NoteOff { channel, note, .. } => {
                let index = note.number() as usize;
                let remaining = self.channels_by_note[index]
                    .fetch_and(!(1 << channel.min(15)), Ordering::Relaxed)
                    & !(1 << channel.min(15));
                if remaining == 0 {
                    self.velocity_by_note[index].store(0, Ordering::Relaxed);
                }
            }
            MidiEvent::AllNotesOff { channel } => {
                let bit = !(1 << channel.min(15));
                for channels in &self.channels_by_note {
                    channels.fetch_and(bit, Ordering::Relaxed);
                }
                for (index, channels) in self.channels_by_note.iter().enumerate() {
                    if channels.load(Ordering::Relaxed) == 0 {
                        self.velocity_by_note[index].store(0, Ordering::Relaxed);
                    }
                }
            }
            MidiEvent::AllSoundOff => {
                for channels in &self.channels_by_note {
                    channels.store(0, Ordering::Relaxed);
                }
                for velocity in &self.velocity_by_note {
                    velocity.store(0, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    fn active_notes(&self) -> Vec<UiMidiNote> {
        self.channels_by_note
            .iter()
            .enumerate()
            .filter_map(|(note, channels)| {
                (channels.load(Ordering::Relaxed) != 0).then_some(UiMidiNote {
                    note: note as u8,
                    velocity: self.velocity_by_note[note].load(Ordering::Relaxed) as f32 / 127.0,
                })
            })
            .collect()
    }
}

impl UiModel {
    pub fn new(source: impl Into<String>) -> Arc<Self> {
        let source = source.into();
        let (source, program, status) = match Compiler::new().compile(&source) {
            Ok(program) => {
                let status = CompileStatus::success(0, program.performance_warnings());
                (source, program, status)
            }
            Err(_) => {
                let program = Compiler::new()
                    .compile(DEFAULT_SOURCE)
                    .expect("default preset must compile");
                let status = CompileStatus::success(0, program.performance_warnings());
                (DEFAULT_SOURCE.to_owned(), program, status)
            }
        };
        let parameters = Arc::new(UserParameterStore::new(program.parameter_specs()));
        let controls = default_control_layout(program.parameter_specs());
        Arc::new(Self {
            state: Mutex::new(UiState {
                active_source: source.clone(),
                source,
                selected_preset: "Custom".into(),
                status,
                program,
                preview_note: 60,
                preview_velocity: 0.9,
                controls,
                mode: UiMode::Editor,
            }),
            exchange: Arc::new(ProgramExchange::new(8)),
            midi: crossbeam_queue::ArrayQueue::new(128),
            parameters,
            waveform: Arc::new(WaveformMonitor::new(48_000.0)),
            midi_preview: MidiPreview::new(),
        })
    }

    pub fn exchange(&self) -> Arc<ProgramExchange> {
        self.exchange.clone()
    }

    pub fn parameter_store(&self) -> Arc<UserParameterStore> {
        self.parameters.clone()
    }

    pub fn waveform_monitor(&self) -> Arc<WaveformMonitor> {
        self.waveform.clone()
    }

    pub fn user_parameter_spec(&self, index: usize) -> Option<ParameterSpec> {
        self.state
            .lock()
            .expect("UI state poisoned")
            .program
            .parameter_specs()
            .get(index)
            .cloned()
    }

    pub fn user_parameter_normalized(&self, index: usize) -> f32 {
        self.parameters.get_normalized(index)
    }

    pub fn set_user_parameter_normalized(&self, index: usize, value: f32) -> bool {
        self.parameters.set_normalized(index, value)
    }

    pub fn initial_program(&self) -> Program {
        self.state
            .lock()
            .expect("UI state poisoned")
            .program
            .clone()
    }

    pub fn source(&self) -> String {
        self.state
            .lock()
            .expect("UI state poisoned")
            .active_source
            .clone()
    }

    pub fn set_expression(&self, source: String) -> CompileStatus {
        self.exchange.collect_retired();
        let result = Compiler::new().compile(&source);
        let mut state = self.state.lock().expect("UI state poisoned");
        let generation = state.status.generation.wrapping_add(1);
        state.source = source;
        state.selected_preset = "Custom".into();
        match result {
            Ok(program) => {
                let status = CompileStatus::success(generation, program.performance_warnings());
                reconcile_parameters(
                    &self.parameters,
                    state.program.parameter_specs(),
                    program.parameter_specs(),
                );
                reconcile_control_layout(&mut state.controls, program.parameter_specs());
                self.exchange.publish(program.clone());
                state.program = program;
                state.active_source = state.source.clone();
                state.status = status;
            }
            Err(error) => state.status = CompileStatus::error(&error, generation),
        }
        state.status.clone()
    }

    pub fn load_preset(&self, name: &str) -> CompileStatus {
        let Some(preset) = PRESETS.iter().find(|preset| preset.name == name) else {
            let mut state = self.state.lock().expect("UI state poisoned");
            let generation = state.status.generation.wrapping_add(1);
            state.status = CompileStatus {
                ok: false,
                message: format!("Unknown preset: {name}"),
                warnings: Vec::new(),
                line: 0,
                column: 0,
                hint: Some("Choose one of the presets shown in the preset list.".into()),
                generation,
            };
            return state.status.clone();
        };
        let status = self.set_expression(preset.source.to_owned());
        if status.ok {
            let specs = {
                let mut state = self.state.lock().expect("UI state poisoned");
                state.selected_preset = preset.name.into();
                state.controls = default_control_layout(state.program.parameter_specs());
                state.program.parameter_specs().to_vec()
            };
            for (index, spec) in specs.iter().enumerate() {
                self.parameters
                    .set_normalized(index, spec.default_normalized());
            }
        }
        status
    }

    pub fn snapshot(&self) -> UiSnapshot {
        self.exchange.collect_retired();
        let state = self.state.lock().expect("UI state poisoned");
        UiSnapshot {
            source: state.source.clone(),
            selected_preset: state.selected_preset.clone(),
            status: state.status.clone(),
            preview_note: state.preview_note,
            preview_velocity: state.preview_velocity,
            presets: PRESETS.iter().map(|preset| preset.name).collect(),
            sample_rate: self.waveform.sample_rate(),
            active_voices: self.waveform.active_voice_count(),
            active_notes: self.midi_preview.active_notes(),
            release_notes: self
                .waveform
                .keyboard_note_states()
                .into_iter()
                .filter(|state| state.released_velocity > 0.0)
                .map(|state| UiMidiNote {
                    note: state.note,
                    velocity: state.released_velocity,
                })
                .collect(),
            parameters: parameter_snapshot(state.program.parameter_specs(), &self.parameters),
            controls: state.controls.clone(),
            mode: state.mode,
        }
    }

    pub fn waveform_preview(&self, length: usize) -> WaveformPreview {
        self.exchange.collect_retired();
        let state = self.state.lock().expect("UI state poisoned");
        let active_voices = self.waveform.active_voice_count();
        let sample_rate = self.waveform.sample_rate();
        let frequency = MidiNote::new(state.preview_note).frequency();
        let length = length.clamp(64, 2_048);
        if active_voices > 0 {
            return WaveformPreview {
                samples: self.waveform.read_recent(length),
                frequency,
                sample_rate,
                active_voices,
                live: true,
            };
        }
        let mut samples = Vec::with_capacity(length);
        let mut input = Inputs {
            s: state.preview_velocity,
            freq: frequency,
            note: state.preview_note as f32,
            sr: sample_rate,
            vol: 1.0,
            mexpr: 1.0,
            ..Inputs::default()
        };
        self.parameters.fill_inputs(&mut input);
        for index in 0..length {
            input.t = index as f32 / sample_rate;
            input.rand = preview_random(index as u32);
            samples.push(state.program.evaluate(&input).wave);
        }
        WaveformPreview {
            samples,
            frequency,
            sample_rate,
            active_voices,
            live: false,
        }
    }

    pub fn layout_json(&self) -> String {
        let state = self.state.lock().expect("UI state poisoned");
        serde_json::to_string(&(state.mode, &state.controls)).unwrap_or_else(|_| "[]".into())
    }

    pub fn restore_layout_json(&self, json: &str) -> Result<(), String> {
        let (mode, mut controls): (UiMode, Vec<ControlLayout>) =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        let mut state = self.state.lock().map_err(|_| "UI state poisoned")?;
        sanitize_control_layout(&mut controls);
        state.mode = mode;
        state.controls = controls;
        let specs = state.program.parameter_specs().to_vec();
        reconcile_control_layout(&mut state.controls, &specs);
        Ok(())
    }

    pub fn push_midi(&self, event: MidiEvent) {
        self.midi_preview.observe(event);
        let mut event = event;
        loop {
            match self.midi.push(event) {
                Ok(()) => return,
                Err(returned) => {
                    event = returned;
                    let _ = self.midi.pop();
                }
            }
        }
    }

    /// Called only by the audio callback. Does not allocate or lock.
    pub fn pop_midi_audio(&self) -> Option<MidiEvent> {
        self.midi.pop()
    }

    /// VSTのオーディオコールバックで受信したMIDIを画面キーボード用に記録する。
    /// ロックも確保も行わない。
    pub fn observe_midi_for_preview(&self, event: MidiEvent) {
        self.midi_preview.observe(event);
    }

    pub fn handle_json(&self, json: &str) -> Result<(), String> {
        let command: UiCommand = serde_json::from_str(json).map_err(|error| error.to_string())?;
        match command {
            UiCommand::SetExpression { source } => {
                self.set_expression(source);
            }
            UiCommand::LoadPreset { name } => {
                self.load_preset(&name);
            }
            UiCommand::SetParameter { name, value } => {
                let mut state = self.state.lock().map_err(|_| "UI state poisoned")?;
                match name.as_str() {
                    "previewNote" => state.preview_note = value.round().clamp(0.0, 127.0) as u8,
                    "previewVelocity" => state.preview_velocity = value.clamp(0.0, 1.0),
                    _ => return Err(format!("Unknown parameter: {name}")),
                }
            }
            UiCommand::SetUserParameter { index, value } => {
                let spec_count = self
                    .state
                    .lock()
                    .map_err(|_| "UI state poisoned")?
                    .program
                    .parameter_specs()
                    .len();
                if index >= spec_count || !self.parameters.set_normalized(index, value) {
                    return Err(format!("Unknown user parameter index: {index}"));
                }
            }
            UiCommand::SetLayout { mut controls } => {
                sanitize_control_layout(&mut controls);
                let mut state = self.state.lock().map_err(|_| "UI state poisoned")?;
                let specs = state.program.parameter_specs().to_vec();
                state.controls = controls;
                reconcile_control_layout(&mut state.controls, &specs);
            }
            UiCommand::SetMode { mode } => {
                self.state.lock().map_err(|_| "UI state poisoned")?.mode = mode;
            }
            UiCommand::NoteOn { note, velocity } => self.push_midi(MidiEvent::NoteOn {
                channel: 0,
                note: MidiNote::new(note),
                velocity: velocity.clamp(0.0, 1.0),
            }),
            UiCommand::NoteOff { note } => self.push_midi(MidiEvent::NoteOff {
                channel: 0,
                note: MidiNote::new(note),
                velocity: 0.0,
            }),
            UiCommand::UiError { message } => {
                #[cfg(target_os = "windows")]
                append_ui_log(&format!("javascript: {message}"));
            }
            UiCommand::UiReady => {
                #[cfg(target_os = "windows")]
                append_ui_log("Monaco UI ready");
            }
            UiCommand::RequestWaveform => {}
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
enum UiCommand {
    SetExpression { source: String },
    LoadPreset { name: String },
    SetParameter { name: String, value: f32 },
    SetUserParameter { index: usize, value: f32 },
    SetLayout { controls: Vec<ControlLayout> },
    SetMode { mode: UiMode },
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
    UiError { message: String },
    UiReady,
    RequestWaveform,
}

fn parameter_label(name: &str) -> String {
    name.strip_prefix("p_")
        .unwrap_or(name)
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parameter_snapshot(specs: &[ParameterSpec], store: &UserParameterStore) -> Vec<UiParameter> {
    specs
        .iter()
        .map(|spec| {
            let normalized = store.get_normalized(spec.index);
            UiParameter {
                index: spec.index,
                name: spec.name.clone(),
                label: parameter_label(&spec.name),
                default: spec.default,
                min: spec.min,
                max: spec.max,
                step: spec.step,
                normalized,
                value: spec.denormalize(normalized),
            }
        })
        .collect()
}

fn reconcile_parameters(
    store: &UserParameterStore,
    previous: &[ParameterSpec],
    next: &[ParameterSpec],
) {
    let old_values = store.snapshot();
    for spec in next {
        let normalized = previous
            .iter()
            .find(|previous| previous.name == spec.name)
            .map_or_else(
                || spec.default_normalized(),
                |previous| spec.normalize(previous.denormalize(old_values[previous.index])),
            );
        store.set_normalized(spec.index, normalized);
    }
    for index in next.len()..old_values.len() {
        store.set_normalized(index, 0.0);
    }
}

fn default_control_layout(specs: &[ParameterSpec]) -> Vec<ControlLayout> {
    specs
        .iter()
        .map(|spec| default_control(spec, spec.index))
        .collect()
}

fn default_control(spec: &ParameterSpec, ordinal: usize) -> ControlLayout {
    let column = ordinal % 4;
    let row = ordinal / 4;
    ControlLayout {
        name: spec.name.clone(),
        kind: "knob".into(),
        x: 2.0 + column as f32 * 24.0,
        y: 5.0 + row as f32 * 31.0,
        width: 20.0,
        height: 25.0,
    }
}

fn reconcile_control_layout(controls: &mut Vec<ControlLayout>, specs: &[ParameterSpec]) {
    controls.retain(|control| specs.iter().any(|spec| spec.name == control.name));
    for spec in specs {
        if !controls.iter().any(|control| control.name == spec.name) {
            controls.push(default_control(spec, spec.index));
        }
    }
    sanitize_control_layout(controls);
}

fn sanitize_control_layout(controls: &mut Vec<ControlLayout>) {
    for control in controls {
        if !matches!(control.kind.as_str(), "knob" | "slider" | "toggle") {
            control.kind = "knob".into();
        }
        // UIと同じ範囲を使い、保存時に大きなコントロールを勝手に縮小しない。
        // サイズを先に確定してから、右端・下端がステージ内に収まるよう位置を補正する。
        control.width = control.width.clamp(7.0, 100.0);
        control.height = control.height.clamp(14.0, 100.0);
        control.x = control.x.clamp(0.0, 100.0 - control.width);
        control.y = control.y.clamp(0.0, 100.0 - control.height);
    }
}

/// Writes a VST editor lifecycle message to the Windows UI diagnostic log.
pub fn write_ui_diagnostic(message: &str) {
    #[cfg(target_os = "windows")]
    append_ui_log(message);
    #[cfg(not(target_os = "windows"))]
    let _ = message;
}

#[cfg(target_os = "windows")]
fn ui_data_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Code Synthesizer")
}

#[cfg(target_os = "windows")]
fn append_ui_log(message: &str) {
    let directory = ui_data_directory();
    let _ = std::fs::create_dir_all(&directory);
    let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("ui.log"))
    else {
        return;
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let _ = writeln!(log, "[{timestamp}] {message}");
}

fn preview_random(index: u32) -> f32 {
    let mut value = index.wrapping_add(0x9E37_79B9);
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    (value as f32 / u32::MAX as f32) * 2.0 - 1.0
}

#[derive(RustEmbed)]
#[folder = "../../ui/dist/"]
struct UiAssets;

fn protocol_response(model: &UiModel, path: &str) -> wry::http::Response<Cow<'static, [u8]>> {
    let path = path.trim_start_matches('/');
    if path == "api/state" {
        return json_response(&model.snapshot());
    }
    if path == "api/waveform" {
        // The UI draws a 768-sample frame and uses the extra samples to move
        // the frame without wrapping its right edge.
        return json_response(&model.waveform_preview(1_024));
    }
    let asset_path = if path.is_empty() { "index.html" } else { path };
    match UiAssets::get(asset_path) {
        Some(asset) => {
            #[cfg(target_os = "windows")]
            if asset_path == "index.html" {
                append_ui_log("serving embedded index.html");
            }
            wry::http::Response::builder()
                .status(200)
                .header(
                    wry::http::header::CONTENT_TYPE,
                    mime_guess::from_path(asset_path)
                        .first_or_octet_stream()
                        .as_ref(),
                )
                .header(wry::http::header::CACHE_CONTROL, "no-cache")
                .body(asset.data)
                .expect("valid asset response")
        }
        None => {
            #[cfg(target_os = "windows")]
            append_ui_log(&format!("embedded asset not found: {asset_path}"));
            wry::http::Response::builder()
                .status(404)
                .header(wry::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(Cow::Borrowed(&b"Not found"[..]))
                .expect("valid not-found response")
        }
    }
}

fn json_response(value: &impl Serialize) -> wry::http::Response<Cow<'static, [u8]>> {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec());
    wry::http::Response::builder()
        .status(200)
        .header(
            wry::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )
        .header(wry::http::header::CACHE_CONTROL, "no-store")
        .body(Cow::Owned(body))
        .expect("valid JSON response")
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::num::NonZeroIsize;
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::raw_window_handle::{
        HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
    };
    use wry::{PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

    struct ParentWindow(NonZeroIsize);

    impl HasWindowHandle for ParentWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let handle = Win32WindowHandle::new(self.0);
            // SAFETY: VST3 calls `attached` on the UI thread and guarantees that
            // the HWND remains valid until `removed` is called.
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
        }
    }

    pub struct WebViewHost {
        webview: WebView,
        _context: WebContext,
        _parent: ParentWindow,
    }

    impl WebViewHost {
        /// Attaches a child WebView to a VST3 host window.
        ///
        /// # Safety
        ///
        /// `parent` must be a valid Windows `HWND` owned by the calling UI
        /// thread and must remain valid until this [`WebViewHost`] is dropped.
        pub unsafe fn attach(
            parent: *mut c_void,
            width: u32,
            height: u32,
            model: Arc<UiModel>,
        ) -> Result<Self, String> {
            let raw = NonZeroIsize::new(parent as isize).ok_or("VST3 supplied a null HWND")?;
            let parent = ParentWindow(raw);
            append_ui_log(&format!(
                "attaching WebView2 parent=0x{:X} size={}x{}",
                parent.0.get(),
                width,
                height
            ));
            let data_directory = ui_data_directory().join("WebView2");
            if let Err(error) = std::fs::create_dir_all(&data_directory) {
                let message = format!(
                    "failed to create WebView2 data directory {}: {error}",
                    data_directory.display()
                );
                append_ui_log(&message);
                return Err(message);
            }
            let mut context = WebContext::new(Some(data_directory));
            let protocol_model = model.clone();
            let ipc_model = model;
            let bounds = Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(width, height).into(),
            };
            let webview = WebViewBuilder::new_with_web_context(&mut context)
                .with_custom_protocol("synth".into(), move |_id, request| {
                    protocol_response(&protocol_model, request.uri().path())
                })
                .with_ipc_handler(move |request| {
                    if let Err(error) = ipc_model.handle_json(request.body()) {
                        append_ui_log(&format!("IPC error: {error}"));
                    }
                })
                .with_on_page_load_handler(|event, url| {
                    let state = match event {
                        PageLoadEvent::Started => "started",
                        PageLoadEvent::Finished => "finished",
                    };
                    append_ui_log(&format!("page load {state}: {url}"));
                })
                .with_background_color((11, 13, 17, 255))
                .with_devtools(cfg!(debug_assertions))
                .with_bounds(bounds)
                .with_url("synth://localhost/index.html")
                .build_as_child(&parent)
                .map_err(|error| {
                    let message = format!("WebView2 creation failed: {error}");
                    append_ui_log(&message);
                    message
                })?;
            append_ui_log("WebView2 child created");
            if std::env::var_os("CODE_SYNTH_UI_DEVTOOLS").is_some() {
                webview.open_devtools();
            }
            Ok(Self {
                webview,
                _context: context,
                _parent: parent,
            })
        }

        pub fn resize(&self, width: u32, height: u32) -> Result<(), String> {
            let result = self
                .webview
                .set_bounds(Rect {
                    position: LogicalPosition::new(0, 0).into(),
                    size: LogicalSize::new(width, height).into(),
                })
                .map_err(|error| error.to_string());
            if let Err(error) = &result {
                append_ui_log(&format!("WebView2 resize failed: {error}"));
            }
            result
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform::WebViewHost;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_valid_program_and_preserves_last_valid_on_error() {
        let model = UiModel::new(DEFAULT_SOURCE);
        assert!(model.set_expression("wave = 0.25\nl_limit = 1".into()).ok);
        assert!(!model.set_expression("wave = unknown".into()).ok);
        assert_eq!(model.source(), "wave = 0.25\nl_limit = 1");
        assert_eq!(model.waveform_preview(64).samples, vec![0.25; 64]);
    }

    #[test]
    fn every_factory_preset_compiles() {
        for preset in PRESETS {
            Compiler::new()
                .compile(preset.source)
                .unwrap_or_else(|error| panic!("preset {} failed: {error}", preset.name));
        }
    }

    #[test]
    fn exposes_parameters_and_persists_play_layout() {
        let model = UiModel::new("p_tone = param(0.25, 0, 2, 0.01)\nwave = p_tone\nl_limit = 1");
        let snapshot = model.snapshot();
        assert_eq!(snapshot.parameters.len(), 1);
        assert_eq!(snapshot.parameters[0].value, 0.25);
        model
            .handle_json(r#"{"cmd":"setUserParameter","index":0,"value":0.75}"#)
            .unwrap();
        model
            .handle_json(r#"{"cmd":"setMode","mode":"play"}"#)
            .unwrap();
        model
            .handle_json(r#"{"cmd":"setLayout","controls":[{"name":"p_tone","kind":"slider","x":20,"y":10,"width":70,"height":80}]}"#)
            .unwrap();
        let snapshot = model.snapshot();
        assert!((snapshot.parameters[0].value - 1.5).abs() < 0.0001);
        assert!(matches!(snapshot.mode, UiMode::Play));
        assert_eq!(snapshot.controls[0].kind, "slider");
        assert_eq!(snapshot.controls[0].width, 70.0);
        assert_eq!(snapshot.controls[0].height, 80.0);

        // 大きくした後に移動しても、サイズを初期値へ戻してはならない。
        model
            .handle_json(r#"{"cmd":"setLayout","controls":[{"name":"p_tone","kind":"slider","x":5,"y":5,"width":70,"height":80}]}"#)
            .unwrap();
        let moved = model.snapshot();
        assert_eq!(moved.controls[0].x, 5.0);
        assert_eq!(moved.controls[0].y, 5.0);
        assert_eq!(moved.controls[0].width, 70.0);
        assert_eq!(moved.controls[0].height, 80.0);

        let saved = model.layout_json();
        let restored = UiModel::new("p_tone = param(0.25, 0, 2, 0.01)\nwave = p_tone\nl_limit = 1");
        restored.restore_layout_json(&saved).unwrap();
        let restored = restored.snapshot();
        assert_eq!(restored.controls[0].x, 5.0);
        assert_eq!(restored.controls[0].width, 70.0);
        assert_eq!(restored.controls[0].height, 80.0);
    }

    #[test]
    fn exposes_external_midi_notes_for_keyboard_preview() {
        let model = UiModel::new(DEFAULT_SOURCE);
        model.observe_midi_for_preview(MidiEvent::NoteOn {
            channel: 2,
            note: MidiNote::new(64),
            velocity: 0.8,
        });
        let active_notes = model.snapshot().active_notes;
        assert_eq!(active_notes.len(), 1);
        assert_eq!(active_notes[0].note, 64);
        assert!((active_notes[0].velocity - 0.8).abs() < 0.01);

        // 別チャンネルのNote Offでは、演奏中チャンネルのハイライトを消さない。
        model.observe_midi_for_preview(MidiEvent::NoteOff {
            channel: 1,
            note: MidiNote::new(64),
            velocity: 0.0,
        });
        assert_eq!(model.snapshot().active_notes[0].note, 64);

        model.observe_midi_for_preview(MidiEvent::NoteOff {
            channel: 2,
            note: MidiNote::new(64),
            velocity: 0.0,
        });
        assert!(model.snapshot().active_notes.is_empty());
    }

    #[test]
    fn loading_a_preset_resets_parameter_values_and_layout() {
        let model = UiModel::new(DEFAULT_SOURCE);
        assert!(model.load_preset("Parameter Guide").ok);
        let initial = model.snapshot();
        let parameter = &initial.parameters[0];
        let control = &initial.controls[0];

        model.set_user_parameter_normalized(parameter.index, 0.0);
        model
            .handle_json(&format!(
                r#"{{"cmd":"setLayout","controls":[{{"name":"{}","kind":"slider","x":40,"y":40,"width":40,"height":40}}]}}"#,
                control.name
            ))
            .unwrap();

        assert!(model.load_preset("Parameter Guide").ok);
        let reset = model.snapshot();
        let default_normalized =
            (parameter.default - parameter.min) / (parameter.max - parameter.min);
        assert!((reset.parameters[0].normalized - default_normalized).abs() < 0.0001);
        assert_eq!(reset.controls[0].kind, "knob");
        assert_eq!(reset.controls[0].x, 2.0);
        assert_eq!(reset.controls[0].y, 5.0);
    }

    #[test]
    fn compile_status_includes_actionable_hint() {
        let model = UiModel::new(DEFAULT_SOURCE);
        let status = model.set_expression("wave = frqe\nl_limit = 1".into());
        assert!(!status.ok);
        assert!(
            status
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("freq"))
        );
    }
}
