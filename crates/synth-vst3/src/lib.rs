#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

//! VST3 component, controller, MIDI mapping, and WebView editor adapter.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::MaybeUninit;
use std::ptr;
use std::slice;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use synth_core::{MidiEvent, MidiNote, SynthEngine};
use synth_dsl::{Inputs, MAX_USER_PARAMETERS};
use synth_ui::{DEFAULT_SOURCE, UiModel};
use vst3::{Class, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*, uid};

const PLUGIN_NAME: &str = "Code Synthesizer";
const VENDOR: &str = "Code Synthesizer";
const VENDOR_URL: &str = "";
const VENDOR_EMAIL: &str = "";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const MASTER_GAIN_ID: ParamID = 0;
const USER_PARAM_BASE: ParamID = 100;
const MIDI_PARAM_BASE: ParamID = 1_000;
const MIDI_CONTROLLERS: u32 = 131;
const MIDI_CHANNELS: u32 = 16;
const PARAMETER_COUNT: i32 =
    1 + MAX_USER_PARAMETERS as i32 + (MIDI_CONTROLLERS * MIDI_CHANNELS) as i32;
const DEFAULT_GAIN_NORMALIZED: f64 = 60.0 / 66.0;
const STATE_MAGIC: &[u8; 4] = b"CSYN";
const STATE_VERSION: u32 = 2;
const MAX_STATE_SOURCE: usize = 1024 * 1024;
const MAX_STATE_LAYOUT: usize = 1024 * 1024;
const EDITOR_WIDTH: i32 = 1050;
const EDITOR_HEIGHT: i32 = 680;

struct ProcessorState {
    engine: SynthEngine,
    transport: Inputs,
}

struct SynthPlugin {
    processor: RefCell<ProcessorState>,
    ui: Arc<UiModel>,
    master_gain: AtomicU64,
}

impl Class for SynthPlugin {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IEditController,
        IMidiMapping,
        IProcessContextRequirements,
    );
}

impl SynthPlugin {
    const CID: TUID = uid(0x9A3D1F6C, 0x2B7E4A15, 0xB8C0D2E4, 0xF617293B);

    fn new() -> Self {
        let ui = UiModel::new(DEFAULT_SOURCE);
        let mut engine = SynthEngine::with_exchange(48_000.0, ui.initial_program(), ui.exchange());
        engine.attach_runtime_state(ui.parameter_store(), ui.waveform_monitor());
        Self {
            processor: RefCell::new(ProcessorState {
                engine,
                transport: Inputs::default(),
            }),
            ui,
            master_gain: AtomicU64::new(DEFAULT_GAIN_NORMALIZED.to_bits()),
        }
    }

    unsafe fn process_audio(&self, data: *mut ProcessData) -> tresult {
        if data.is_null() {
            return kInvalidArgument;
        }
        let data = unsafe { &mut *data };
        if data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 {
            return kResultFalse;
        }
        if data.numOutputs != 1 || data.outputs.is_null() || data.numSamples < 0 {
            return kResultOk;
        }
        let buses = unsafe { slice::from_raw_parts_mut(data.outputs, 1) };
        if buses[0].numChannels != 2 || unsafe { buses[0].__field0.channelBuffers32 }.is_null() {
            return kResultOk;
        }
        let channels = unsafe { slice::from_raw_parts_mut(buses[0].__field0.channelBuffers32, 2) };
        if channels[0].is_null() || channels[1].is_null() {
            return kResultOk;
        }
        let sample_count = data.numSamples as usize;
        let left = unsafe { slice::from_raw_parts_mut(channels[0], sample_count) };
        let right = unsafe { slice::from_raw_parts_mut(channels[1], sample_count) };
        let mut state = match self.processor.try_borrow_mut() {
            Ok(state) => state,
            Err(_) => {
                left.fill(0.0);
                right.fill(0.0);
                return kResultFalse;
            }
        };

        state.engine.begin_block();
        while let Some(event) = self.ui.pop_midi_audio() {
            state.engine.handle_midi(event);
        }
        unsafe { self.apply_parameter_changes(data.inputParameterChanges, &mut state.engine) };
        if !data.processContext.is_null() {
            state.transport = unsafe { transport_inputs(&*data.processContext) };
        }

        let events = unsafe { ComRef::from_raw(data.inputEvents) };
        let event_count = events
            .as_ref()
            .map_or(0, |list| unsafe { list.getEventCount() });
        let mut event_index = 0;
        let mut next_event = events
            .as_ref()
            .and_then(|list| unsafe { read_event(list, 0) });
        let gain = normalized_gain(f64::from_bits(self.master_gain.load(Ordering::Relaxed))) as f32;
        let ppq_step = if state.transport.tempo > 0.0 {
            state.transport.tempo / (60.0 * state.engine.sample_rate())
        } else {
            0.0
        };

        let mut silent = true;
        for sample in 0..sample_count {
            while let Some(event) = next_event.as_ref() {
                if event.sampleOffset.max(0) as usize > sample {
                    break;
                }
                if let Some(event) = unsafe { translate_event(event) } {
                    self.ui.observe_midi_for_preview(event);
                    state.engine.handle_midi(event);
                }
                event_index += 1;
                next_event = if event_index < event_count {
                    events
                        .as_ref()
                        .and_then(|list| unsafe { read_event(list, event_index) })
                } else {
                    None
                };
            }
            let input = state.transport;
            let (sample_l, sample_r) = state.engine.render_sample(input);
            left[sample] = sample_l * gain;
            right[sample] = sample_r * gain;
            silent &= left[sample] == 0.0 && right[sample] == 0.0;
            state.transport.ppq += ppq_step;
        }
        buses[0].silenceFlags = if silent { 0b11 } else { 0 };
        kResultOk
    }

    unsafe fn apply_parameter_changes(
        &self,
        changes: *mut IParameterChanges,
        engine: &mut SynthEngine,
    ) {
        let Some(changes) = (unsafe { ComRef::from_raw(changes) }) else {
            return;
        };
        let count = unsafe { changes.getParameterCount() };
        for index in 0..count {
            let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(index)) }) else {
                continue;
            };
            let point_count = unsafe { queue.getPointCount() };
            if point_count <= 0 {
                continue;
            }
            let mut offset = 0;
            let mut value = 0.0;
            if unsafe { queue.getPoint(point_count - 1, &mut offset, &mut value) } != kResultTrue {
                continue;
            }
            let id = unsafe { queue.getParameterId() };
            if id == MASTER_GAIN_ID {
                self.master_gain
                    .store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            } else if let Some(index) = user_parameter_index(id) {
                self.ui.set_user_parameter_normalized(index, value as f32);
            } else if let Some((channel, controller)) = decode_midi_param(id) {
                apply_midi_parameter(engine, channel, controller, value as f32);
            }
        }
    }
}

impl IPluginBaseTrait for SynthPlugin {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for SynthPlugin {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        if !class_id.is_null() {
            unsafe { *class_id = [0; 16] };
        }
        kResultFalse
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, direction: BusDirection) -> i32 {
        match (media_type as MediaTypes, direction as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) => 1,
            (MediaTypes_::kEvent, BusDirections_::kInput) => 1,
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        direction: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if bus.is_null() || index != 0 {
            return kInvalidArgument;
        }
        let bus = unsafe { &mut *bus };
        match (media_type as MediaTypes, direction as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) => {
                bus.mediaType = MediaTypes_::kAudio as MediaType;
                bus.direction = BusDirections_::kOutput as BusDirection;
                bus.channelCount = 2;
                copy_wstring("Stereo Output", &mut bus.name);
                bus.busType = BusTypes_::kMain as BusType;
                bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
                kResultOk
            }
            (MediaTypes_::kEvent, BusDirections_::kInput) => {
                bus.mediaType = MediaTypes_::kEvent as MediaType;
                bus.direction = BusDirections_::kInput as BusDirection;
                bus.channelCount = 16;
                copy_wstring("MIDI Input", &mut bus.name);
                bus.busType = BusTypes_::kMain as BusType;
                bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn getRoutingInfo(
        &self,
        _in_info: *mut RoutingInfo,
        _out_info: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media_type: MediaType,
        _direction: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        if state == 0
            && let Ok(mut processor) = self.processor.try_borrow_mut()
        {
            processor.engine.all_sound_off();
        }
        kResultOk
    }

    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        match unsafe { read_state(stream) } {
            Ok(saved) => {
                self.master_gain
                    .store(saved.gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                let status = self.ui.set_expression(saved.source);
                if !status.ok {
                    return kResultFalse;
                }
                if let Some(parameters) = saved.parameters {
                    for (index, value) in parameters.into_iter().enumerate() {
                        self.ui.set_user_parameter_normalized(index, value);
                    }
                }
                if let Some(layout) = saved.layout {
                    let _ = self.ui.restore_layout_json(&layout);
                }
                kResultOk
            }
            Err(result) => result,
        }
    }

    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        let gain = f64::from_bits(self.master_gain.load(Ordering::Relaxed));
        let parameters: [f32; MAX_USER_PARAMETERS] =
            std::array::from_fn(|index| self.ui.user_parameter_normalized(index));
        unsafe {
            write_state(
                stream,
                gain,
                &self.ui.source(),
                &parameters,
                &self.ui.layout_json(),
            )
        }
    }
}

impl IAudioProcessorTrait for SynthPlugin {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        num_inputs: i32,
        outputs: *mut SpeakerArrangement,
        num_outputs: i32,
    ) -> tresult {
        if num_inputs != 0 || num_outputs != 1 || outputs.is_null() {
            return kResultFalse;
        }
        if unsafe { *outputs } == SpeakerArr::kStereo {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getBusArrangement(
        &self,
        direction: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        if direction as BusDirections != BusDirections_::kOutput
            || index != 0
            || arrangement.is_null()
        {
            return kInvalidArgument;
        }
        unsafe { *arrangement = SpeakerArr::kStereo };
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if setup.is_null() {
            return kInvalidArgument;
        }
        if let Ok(mut processor) = self.processor.try_borrow_mut() {
            processor
                .engine
                .set_sample_rate(unsafe { (*setup).sampleRate } as f32);
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        if state == 0
            && let Ok(mut processor) = self.processor.try_borrow_mut()
        {
            processor.engine.all_sound_off();
        }
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        unsafe { self.process_audio(data) }
    }

    unsafe fn getTailSamples(&self) -> u32 {
        u32::MAX
    }
}

impl IProcessContextRequirementsTrait for SynthPlugin {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        (ProcessContext_::StatesAndFlags_::kTempoValid
            | ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid
            | ProcessContext_::StatesAndFlags_::kBarPositionValid
            | ProcessContext_::StatesAndFlags_::kTimeSigValid
            | ProcessContext_::StatesAndFlags_::kPlaying) as u32
    }
}

impl IEditControllerTrait for SynthPlugin {
    unsafe fn setComponentState(&self, stream: *mut IBStream) -> tresult {
        unsafe { <Self as IComponentTrait>::setState(self, stream) }
    }

    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        unsafe { <Self as IComponentTrait>::setState(self, stream) }
    }

    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        unsafe { <Self as IComponentTrait>::getState(self, stream) }
    }

    unsafe fn getParameterCount(&self) -> i32 {
        PARAMETER_COUNT
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        if info.is_null() || !(0..PARAMETER_COUNT).contains(&index) {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        if index == 0 {
            info.id = MASTER_GAIN_ID;
            copy_wstring("Master Gain", &mut info.title);
            copy_wstring("Gain", &mut info.shortTitle);
            copy_wstring("dB", &mut info.units);
            info.stepCount = 0;
            info.defaultNormalizedValue = DEFAULT_GAIN_NORMALIZED;
            info.unitId = 0;
            info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
            return kResultOk;
        }
        if index <= MAX_USER_PARAMETERS as i32 {
            let parameter_index = index as usize - 1;
            info.id = user_parameter_id(parameter_index);
            info.unitId = 0;
            if let Some(spec) = self.ui.user_parameter_spec(parameter_index) {
                let title = spec
                    .name
                    .strip_prefix("p_")
                    .unwrap_or(&spec.name)
                    .replace('_', " ");
                copy_wstring(&title, &mut info.title);
                copy_wstring(&title, &mut info.shortTitle);
                copy_wstring("", &mut info.units);
                info.stepCount = if spec.step > 0.0 {
                    ((spec.max - spec.min) / spec.step)
                        .round()
                        .clamp(1.0, i32::MAX as f32) as i32
                } else {
                    0
                };
                info.defaultNormalizedValue = spec.default_normalized() as f64;
                info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
            } else {
                let title = format!("User Parameter {:02} (unused)", parameter_index + 1);
                copy_wstring(&title, &mut info.title);
                copy_wstring("Unused", &mut info.shortTitle);
                copy_wstring("", &mut info.units);
                info.stepCount = 0;
                info.defaultNormalizedValue = 0.0;
                info.flags = ParameterInfo_::ParameterFlags_::kIsHidden;
            }
            return kResultOk;
        }
        let ordinal = index as u32 - 1 - MAX_USER_PARAMETERS as u32;
        let channel = ordinal / MIDI_CONTROLLERS;
        let controller = ordinal % MIDI_CONTROLLERS;
        info.id = midi_param_id(channel, controller);
        let name = midi_parameter_name(channel, controller);
        copy_wstring(&name, &mut info.title);
        copy_wstring(&name, &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = if controller == ControllerNumbers_::kCtrlProgramChange as u32 {
            127
        } else {
            0
        };
        info.defaultNormalizedValue = midi_parameter_default(controller);
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate
            | ParameterInfo_::ParameterFlags_::kIsHidden;
        kResultOk
    }

    unsafe fn getParamStringByValue(
        &self,
        id: ParamID,
        normalized: ParamValue,
        string: *mut String128,
    ) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let display = if id == MASTER_GAIN_ID {
            let db = normalized_to_db(normalized);
            if db <= -59.95 {
                "-inf dB".to_owned()
            } else {
                format!("{db:.1} dB")
            }
        } else if let Some(index) = user_parameter_index(id) {
            let Some(spec) = self.ui.user_parameter_spec(index) else {
                return kInvalidArgument;
            };
            format_parameter_value(spec.denormalize(normalized as f32), spec.step)
        } else if let Some((_channel, controller)) = decode_midi_param(id) {
            if controller == ControllerNumbers_::kPitchBend as u32 {
                format!("{:+.3}", normalized * 2.0 - 1.0)
            } else {
                format!("{:.0}", normalized.clamp(0.0, 1.0) * 127.0)
            }
        } else {
            return kInvalidArgument;
        };
        copy_wstring(&display, unsafe { &mut *string });
        kResultOk
    }

    unsafe fn getParamValueByString(
        &self,
        id: ParamID,
        string: *mut TChar,
        normalized: *mut ParamValue,
    ) -> tresult {
        if string.is_null() || normalized.is_null() {
            return kInvalidArgument;
        }
        let Some(text) = (unsafe { wstring_to_string(string) }) else {
            return kInvalidArgument;
        };
        let cleaned = text.trim().trim_end_matches("dB").trim();
        let Ok(value) = f64::from_str(cleaned) else {
            return kInvalidArgument;
        };
        let value = if id == MASTER_GAIN_ID {
            db_to_normalized(value)
        } else if let Some(index) = user_parameter_index(id) {
            let Some(spec) = self.ui.user_parameter_spec(index) else {
                return kInvalidArgument;
            };
            spec.normalize(value as f32) as f64
        } else if decode_midi_param(id).is_some() {
            if value.abs() > 1.0 {
                value / 127.0
            } else {
                value
            }
        } else {
            return kInvalidArgument;
        };
        unsafe { *normalized = value.clamp(0.0, 1.0) };
        kResultOk
    }

    unsafe fn normalizedParamToPlain(&self, id: ParamID, normalized: ParamValue) -> ParamValue {
        if id == MASTER_GAIN_ID {
            normalized_to_db(normalized)
        } else if let Some(index) = user_parameter_index(id) {
            self.ui
                .user_parameter_spec(index)
                .map_or(normalized, |spec| {
                    spec.denormalize(normalized as f32) as f64
                })
        } else {
            normalized
        }
    }

    unsafe fn plainParamToNormalized(&self, id: ParamID, plain: ParamValue) -> ParamValue {
        if id == MASTER_GAIN_ID {
            db_to_normalized(plain)
        } else if let Some(index) = user_parameter_index(id) {
            self.ui
                .user_parameter_spec(index)
                .map_or(plain.clamp(0.0, 1.0), |spec| {
                    spec.normalize(plain as f32) as f64
                })
        } else {
            plain.clamp(0.0, 1.0)
        }
    }

    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        if id == MASTER_GAIN_ID {
            f64::from_bits(self.master_gain.load(Ordering::Relaxed))
        } else if let Some(index) = user_parameter_index(id) {
            self.ui.user_parameter_normalized(index) as f64
        } else if let Some((_channel, controller)) = decode_midi_param(id) {
            midi_parameter_default(controller)
        } else {
            0.0
        }
    }

    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        if id == MASTER_GAIN_ID {
            self.master_gain
                .store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            return kResultOk;
        }
        if let Some(index) = user_parameter_index(id) {
            self.ui.set_user_parameter_normalized(index, value as f32);
            return kResultOk;
        }
        let Some((channel, controller)) = decode_midi_param(id) else {
            return kInvalidArgument;
        };
        if let Ok(mut processor) = self.processor.try_borrow_mut() {
            apply_midi_parameter(&mut processor.engine, channel, controller, value as f32);
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, name: *const c_char) -> *mut IPlugView {
        if name.is_null() || unsafe { CStr::from_ptr(name) }.to_bytes() != b"editor" {
            return ptr::null_mut();
        }
        synth_ui::write_ui_diagnostic("VST3 createView(editor)");
        ComWrapper::new(SynthView::new(self.ui.clone()))
            .to_com_ptr::<IPlugView>()
            .map_or(ptr::null_mut(), |view| view.into_raw())
    }
}

impl IMidiMappingTrait for SynthPlugin {
    unsafe fn getMidiControllerAssignment(
        &self,
        bus_index: i32,
        channel: i16,
        midi_controller_number: CtrlNumber,
        id: *mut ParamID,
    ) -> tresult {
        if bus_index != 0
            || !(0..MIDI_CHANNELS as i16).contains(&channel)
            || midi_controller_number < 0
            || midi_controller_number as u32 >= MIDI_CONTROLLERS
            || id.is_null()
        {
            return kResultFalse;
        }
        unsafe { *id = midi_param_id(channel as u32, midi_controller_number as u32) };
        kResultTrue
    }
}

struct SynthView {
    model: Arc<UiModel>,
    width: Cell<i32>,
    height: Cell<i32>,
    #[cfg(target_os = "windows")]
    host: RefCell<Option<synth_ui::WebViewHost>>,
}

impl SynthView {
    fn new(model: Arc<UiModel>) -> Self {
        Self {
            model,
            width: Cell::new(EDITOR_WIDTH),
            height: Cell::new(EDITOR_HEIGHT),
            #[cfg(target_os = "windows")]
            host: RefCell::new(None),
        }
    }
}

impl Class for SynthView {
    type Interfaces = (IPlugView,);
}

impl IPlugViewTrait for SynthView {
    unsafe fn isPlatformTypeSupported(&self, platform: FIDString) -> tresult {
        #[cfg(target_os = "windows")]
        {
            if fid_string_eq(platform, b"HWND") {
                kResultTrue
            } else {
                kResultFalse
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = platform;
            kResultFalse
        }
    }

    unsafe fn attached(&self, parent: *mut c_void, platform: FIDString) -> tresult {
        synth_ui::write_ui_diagnostic("VST3 IPlugView::attached");
        if unsafe { self.isPlatformTypeSupported(platform) } != kResultTrue || parent.is_null() {
            synth_ui::write_ui_diagnostic(
                "VST3 attached rejected: unsupported platform or null parent",
            );
            return kInvalidArgument;
        }
        #[cfg(target_os = "windows")]
        {
            if self.host.borrow().is_some() {
                return kResultFalse;
            }
            match unsafe {
                synth_ui::WebViewHost::attach(
                    parent,
                    self.width.get().max(1) as u32,
                    self.height.get().max(1) as u32,
                    self.model.clone(),
                )
            } {
                Ok(host) => {
                    *self.host.borrow_mut() = Some(host);
                    kResultOk
                }
                Err(_) => kResultFalse,
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            kResultFalse
        }
    }

    unsafe fn removed(&self) -> tresult {
        synth_ui::write_ui_diagnostic("VST3 IPlugView::removed");
        #[cfg(target_os = "windows")]
        {
            self.host.borrow_mut().take();
        }
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            (*size).left = 0;
            (*size).top = 0;
            (*size).right = self.width.get();
            (*size).bottom = self.height.get();
        }
        kResultOk
    }

    unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        let rect = unsafe { &*new_size };
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        self.width.set(width);
        self.height.set(height);
        #[cfg(target_os = "windows")]
        if let Some(host) = self.host.borrow().as_ref()
            && host.resize(width as u32, height as u32).is_err()
        {
            return kResultFalse;
        }
        kResultOk
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, _frame: *mut IPlugFrame) -> tresult {
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultTrue
    }

    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        let rect = unsafe { &mut *rect };
        let width = (rect.right - rect.left).clamp(720, 1920);
        let height = (rect.bottom - rect.top).clamp(480, 1200);
        rect.right = rect.left + width;
        rect.bottom = rect.top + height;
        kResultTrue
    }
}

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(VENDOR_URL, &mut info.url);
        copy_cstring(VENDOR_EMAIL, &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        1
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        info.cid = SynthPlugin::CID;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances;
        copy_cstring("Audio Module Class", &mut info.category);
        copy_cstring(PLUGIN_NAME, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        object: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || object.is_null() {
            return kInvalidArgument;
        }
        unsafe { *object = ptr::null_mut() };
        if unsafe { *(cid as *const TUID) } != SynthPlugin::CID {
            return kInvalidArgument;
        }
        let Some(instance) = ComWrapper::new(SynthPlugin::new()).to_com_ptr::<FUnknown>() else {
            return kInternalError;
        };
        let raw = instance.as_ptr();
        unsafe { ((*(*raw).vtbl).queryInterface)(raw, iid as *mut TUID, object) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        info.cid = SynthPlugin::CID;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances;
        copy_cstring("Audio Module Class", &mut info.category);
        copy_cstring(PLUGIN_NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring("Instrument|Synth", &mut info.subCategories);
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(VERSION, &mut info.version);
        copy_cstring("VST 3.8.0", &mut info.sdkVersion);
        kResultOk
    }
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn BundleEntry(_bundle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn BundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_module: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .map_or(ptr::null_mut(), |factory| factory.into_raw())
}

fn transport_inputs(context: &ProcessContext) -> Inputs {
    let state = context.state;
    let tempo = if state & ProcessContext_::StatesAndFlags_::kTempoValid as u32 != 0 {
        context.tempo.max(1.0) as f32
    } else {
        120.0
    };
    let ppq = if state & ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid as u32 != 0 {
        context.projectTimeMusic as f32
    } else {
        0.0
    };
    let bar = if state & ProcessContext_::StatesAndFlags_::kBarPositionValid as u32 != 0 {
        context.barPositionMusic as f32
    } else {
        0.0
    };
    let beats_per_bar = if state & ProcessContext_::StatesAndFlags_::kTimeSigValid as u32 != 0 {
        context.timeSigNumerator.max(1) as f32
    } else {
        4.0
    };
    Inputs {
        tempo,
        ppq,
        bar,
        beat: (ppq - bar).rem_euclid(beats_per_bar),
        playing: if state & ProcessContext_::StatesAndFlags_::kPlaying as u32 != 0 {
            1.0
        } else {
            0.0
        },
        sr: context.sampleRate as f32,
        ..Inputs::default()
    }
}

unsafe fn read_event(list: &ComRef<'_, IEventList>, index: i32) -> Option<Event> {
    let mut event = MaybeUninit::<Event>::zeroed();
    if unsafe { list.getEvent(index, event.as_mut_ptr()) } == kResultTrue {
        Some(unsafe { event.assume_init() })
    } else {
        None
    }
}

unsafe fn translate_event(event: &Event) -> Option<MidiEvent> {
    if event.busIndex != 0 {
        return None;
    }
    match event.r#type as i32 {
        Event_::EventTypes_::kNoteOnEvent => {
            let note = unsafe { event.__field0.noteOn };
            Some(MidiEvent::NoteOn {
                channel: note.channel.clamp(0, 15) as u8,
                note: MidiNote::new(note.pitch.clamp(0, 127) as u8),
                velocity: note.velocity.clamp(0.0, 1.0),
            })
        }
        Event_::EventTypes_::kNoteOffEvent => {
            let note = unsafe { event.__field0.noteOff };
            Some(MidiEvent::NoteOff {
                channel: note.channel.clamp(0, 15) as u8,
                note: MidiNote::new(note.pitch.clamp(0, 127) as u8),
                velocity: note.velocity.clamp(0.0, 1.0),
            })
        }
        Event_::EventTypes_::kPolyPressureEvent => {
            let pressure = unsafe { event.__field0.polyPressure };
            Some(MidiEvent::PolyPressure {
                channel: pressure.channel.clamp(0, 15) as u8,
                note: MidiNote::new(pressure.pitch.clamp(0, 127) as u8),
                value: pressure.pressure.clamp(0.0, 1.0),
            })
        }
        _ => None,
    }
}

const fn midi_param_id(channel: u32, controller: u32) -> ParamID {
    MIDI_PARAM_BASE + channel * MIDI_CONTROLLERS + controller
}

const fn user_parameter_id(index: usize) -> ParamID {
    USER_PARAM_BASE + index as ParamID
}

fn user_parameter_index(id: ParamID) -> Option<usize> {
    let index = id.checked_sub(USER_PARAM_BASE)? as usize;
    (index < MAX_USER_PARAMETERS).then_some(index)
}

fn decode_midi_param(id: ParamID) -> Option<(u8, u32)> {
    let ordinal = id.checked_sub(MIDI_PARAM_BASE)?;
    if ordinal >= MIDI_CHANNELS * MIDI_CONTROLLERS {
        return None;
    }
    Some((
        (ordinal / MIDI_CONTROLLERS) as u8,
        ordinal % MIDI_CONTROLLERS,
    ))
}

fn midi_parameter_default(controller: u32) -> f64 {
    match controller {
        7 | 11 => 1.0,
        10 => 0.5,
        controller if controller == ControllerNumbers_::kPitchBend as u32 => 0.5,
        _ => 0.0,
    }
}

fn midi_parameter_name(channel: u32, controller: u32) -> String {
    let control = match controller {
        controller if controller < 128 => format!("CC {controller}"),
        controller if controller == ControllerNumbers_::kAfterTouch as u32 => {
            "Channel Pressure".into()
        }
        controller if controller == ControllerNumbers_::kPitchBend as u32 => "Pitch Bend".into(),
        controller if controller == ControllerNumbers_::kCtrlProgramChange as u32 => {
            "Program Change".into()
        }
        _ => "MIDI".into(),
    };
    format!("Ch {} {control}", channel + 1)
}

fn apply_midi_parameter(engine: &mut SynthEngine, channel: u8, controller: u32, value: f32) {
    let value = value.clamp(0.0, 1.0);
    match controller {
        controller if controller < 128 => engine.handle_midi(MidiEvent::ControlChange {
            channel,
            controller: controller as u8,
            value,
        }),
        controller if controller == ControllerNumbers_::kAfterTouch as u32 => {
            engine.handle_midi(MidiEvent::ChannelPressure { channel, value })
        }
        controller if controller == ControllerNumbers_::kPitchBend as u32 => {
            engine.handle_midi(MidiEvent::PitchBend {
                channel,
                value: value * 2.0 - 1.0,
            })
        }
        controller if controller == ControllerNumbers_::kCtrlProgramChange as u32 => engine
            .handle_midi(MidiEvent::ProgramChange {
                channel,
                program: (value * 127.0).round() as u8,
            }),
        _ => {}
    }
}

fn normalized_to_db(normalized: f64) -> f64 {
    -60.0 + 66.0 * normalized.clamp(0.0, 1.0)
}

fn db_to_normalized(db: f64) -> f64 {
    ((db + 60.0) / 66.0).clamp(0.0, 1.0)
}

fn normalized_gain(normalized: f64) -> f64 {
    if normalized <= 0.0 {
        0.0
    } else {
        10.0f64.powf(normalized_to_db(normalized) / 20.0)
    }
}

fn format_parameter_value(value: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{value:.0}")
    } else if step >= 0.1 {
        format!("{value:.1}")
    } else if step >= 0.01 {
        format!("{value:.2}")
    } else {
        format!("{value:.3}")
    }
}

struct SavedState {
    gain: f64,
    source: String,
    parameters: Option<[f32; MAX_USER_PARAMETERS]>,
    layout: Option<String>,
}

unsafe fn read_state(stream: *mut IBStream) -> Result<SavedState, tresult> {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return Err(kInvalidArgument);
    };
    let mut magic = [0_u8; 4];
    unsafe { stream_read_exact(&stream, &mut magic) }?;
    if &magic != STATE_MAGIC {
        return Err(kResultFalse);
    }
    let mut version = [0_u8; 4];
    unsafe { stream_read_exact(&stream, &mut version) }?;
    let version = u32::from_le_bytes(version);
    if version != 1 && version != STATE_VERSION {
        return Err(kResultFalse);
    }
    let mut gain = [0_u8; 8];
    unsafe { stream_read_exact(&stream, &mut gain) }?;
    let mut source_len = [0_u8; 4];
    unsafe { stream_read_exact(&stream, &mut source_len) }?;
    let source_len = u32::from_le_bytes(source_len) as usize;
    if source_len > MAX_STATE_SOURCE {
        return Err(kResultFalse);
    }
    let mut source = vec![0_u8; source_len];
    unsafe { stream_read_exact(&stream, &mut source) }?;
    let source = String::from_utf8(source).map_err(|_| kResultFalse)?;
    if version == 1 {
        return Ok(SavedState {
            gain: f64::from_le_bytes(gain),
            source,
            parameters: None,
            layout: None,
        });
    }
    let mut parameters = [0.0; MAX_USER_PARAMETERS];
    for parameter in &mut parameters {
        let mut bytes = [0_u8; 4];
        unsafe { stream_read_exact(&stream, &mut bytes) }?;
        *parameter = f32::from_le_bytes(bytes).clamp(0.0, 1.0);
    }
    let mut layout_len = [0_u8; 4];
    unsafe { stream_read_exact(&stream, &mut layout_len) }?;
    let layout_len = u32::from_le_bytes(layout_len) as usize;
    if layout_len > MAX_STATE_LAYOUT {
        return Err(kResultFalse);
    }
    let mut layout = vec![0_u8; layout_len];
    unsafe { stream_read_exact(&stream, &mut layout) }?;
    let layout = String::from_utf8(layout).map_err(|_| kResultFalse)?;
    Ok(SavedState {
        gain: f64::from_le_bytes(gain),
        source,
        parameters: Some(parameters),
        layout: Some(layout),
    })
}

unsafe fn write_state(
    stream: *mut IBStream,
    gain: f64,
    source: &str,
    parameters: &[f32; MAX_USER_PARAMETERS],
    layout: &str,
) -> tresult {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return kInvalidArgument;
    };
    if source.len() > MAX_STATE_SOURCE || layout.len() > MAX_STATE_LAYOUT {
        return kResultFalse;
    }
    for bytes in [
        STATE_MAGIC.as_slice(),
        STATE_VERSION.to_le_bytes().as_slice(),
        gain.to_le_bytes().as_slice(),
        (source.len() as u32).to_le_bytes().as_slice(),
        source.as_bytes(),
    ] {
        if let Err(result) = unsafe { stream_write_all(&stream, bytes) } {
            return result;
        }
    }
    for parameter in parameters {
        if let Err(result) =
            unsafe { stream_write_all(&stream, &parameter.clamp(0.0, 1.0).to_le_bytes()) }
        {
            return result;
        }
    }
    for bytes in [
        (layout.len() as u32).to_le_bytes().as_slice(),
        layout.as_bytes(),
    ] {
        if let Err(result) = unsafe { stream_write_all(&stream, bytes) } {
            return result;
        }
    }
    kResultOk
}

unsafe fn stream_read_exact(
    stream: &ComRef<'_, IBStream>,
    mut bytes: &mut [u8],
) -> Result<(), tresult> {
    while !bytes.is_empty() {
        let amount = bytes.len().min(i32::MAX as usize) as i32;
        let mut read = 0;
        let result = unsafe { stream.read(bytes.as_mut_ptr().cast(), amount, &mut read) };
        if result != kResultOk && result != kResultTrue {
            return Err(result);
        }
        if read <= 0 || read > amount {
            return Err(kResultFalse);
        }
        bytes = &mut bytes[read as usize..];
    }
    Ok(())
}

unsafe fn stream_write_all(stream: &ComRef<'_, IBStream>, mut bytes: &[u8]) -> Result<(), tresult> {
    while !bytes.is_empty() {
        let amount = bytes.len().min(i32::MAX as usize) as i32;
        let mut written = 0;
        let result =
            unsafe { stream.write(bytes.as_ptr().cast_mut().cast(), amount, &mut written) };
        if result != kResultOk && result != kResultTrue {
            return Err(result);
        }
        if written <= 0 || written > amount {
            return Err(kResultFalse);
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

fn copy_cstring(source: &str, destination: &mut [c_char]) {
    destination.fill(0);
    let source = CString::new(source).unwrap_or_default();
    for (source, destination) in source.as_bytes().iter().zip(destination.iter_mut()) {
        *destination = *source as c_char;
    }
}

fn copy_wstring(source: &str, destination: &mut [TChar]) {
    destination.fill(0);
    if destination.is_empty() {
        return;
    }
    let capacity = destination.len() - 1;
    for (source, destination) in source
        .encode_utf16()
        .zip(destination[..capacity].iter_mut())
    {
        *destination = source as TChar;
    }
}

fn fid_string_eq(value: FIDString, expected: &[u8]) -> bool {
    if value.is_null() {
        return false;
    }
    unsafe { CStr::from_ptr(value) }.to_bytes() == expected
}

unsafe fn wstring_to_string(value: *const TChar) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut length = 0_usize;
    while length < 4096 && unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    if length == 4096 {
        return None;
    }
    String::from_utf16(unsafe { slice::from_raw_parts(value.cast::<u16>(), length) }).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gain_is_unity() {
        assert!((normalized_gain(DEFAULT_GAIN_NORMALIZED) - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn midi_parameter_ids_round_trip() {
        for channel in 0..MIDI_CHANNELS {
            for controller in 0..MIDI_CONTROLLERS {
                assert_eq!(
                    decode_midi_param(midi_param_id(channel, controller)),
                    Some((channel as u8, controller)),
                );
            }
        }
    }

    #[test]
    fn user_parameter_ids_are_stable_and_do_not_overlap_midi() {
        for index in 0..MAX_USER_PARAMETERS {
            let id = user_parameter_id(index);
            assert_eq!(user_parameter_index(id), Some(index));
            assert!(decode_midi_param(id).is_none());
        }
        assert!(user_parameter_index(MIDI_PARAM_BASE).is_none());
    }

    #[test]
    fn factory_creates_the_single_component() {
        let mut object = ptr::null_mut();
        let result = unsafe {
            Factory.createInstance(
                SynthPlugin::CID.as_ptr(),
                FUnknown_iid.as_ptr(),
                &mut object,
            )
        };
        assert_eq!(result, kResultOk);
        assert!(!object.is_null());
        let unknown = object.cast::<FUnknown>();
        unsafe { ((*(*unknown).vtbl).release)(unknown) };
    }
}
