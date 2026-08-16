//! Code Synthesizer DSL v2 frontend, state preparation, and Cranelift backend.
//!
//! Source is compiled off the audio thread. ProgramInstance owns persistent memory,
//! so sample evaluation performs no allocation or lock.

#[allow(unsafe_code)]
mod dsp;
#[allow(unsafe_code)]
mod jit;
#[allow(unsafe_code)]
mod state;
mod syntax;

use dsp::{BiquadKind, DspKind, StandardOp, biquad_kind, dsp_kind, standard_operation};
pub use state::{RUNTIME_STATE_SLOTS, StateMigrationHandle};
use state::{RingCapacity, RuntimeState, StorageDomain, StorageKind, StorageSpec};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};
use syntax::{BinaryOp, Expr, Function, LiteralUnit, ProgramAst, Span, Statement, UnaryOp};

pub const MAX_USER_PARAMETERS: usize = 32;
const VARIABLE_WARNING_THRESHOLD: usize = 64;
const OPERATION_WARNING_THRESHOLD: usize = 512;
const STACK_WARNING_THRESHOLD: usize = 128;
const STORAGE_WARNING_THRESHOLD: usize = 32;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Inputs {
    pub t: f32,
    pub l: f32,
    pub s: f32,
    pub freq: f32,
    pub note: f32,
    pub ch: f32,
    pub bend: f32,
    pub bend_st: f32,
    pub mw: f32,
    pub vol: f32,
    pub midi_pan: f32,
    pub mexpr: f32,
    pub sustain: f32,
    pub pressure: f32,
    pub poly_pressure: f32,
    pub program: f32,
    pub sr: f32,
    pub tempo: f32,
    pub beat: f32,
    pub bar: f32,
    pub ppq: f32,
    pub playing: f32,
    pub voice: f32,
    pub rand: f32,
    pub wave_l: f32,
    pub wave_r: f32,
    pub cc: [f32; 128],
    pub params: [f32; MAX_USER_PARAMETERS],
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            t: 0.0,
            l: 0.0,
            s: 0.0,
            freq: 0.0,
            note: 0.0,
            ch: 0.0,
            bend: 0.0,
            bend_st: 0.0,
            mw: 0.0,
            vol: 0.0,
            midi_pan: 0.0,
            mexpr: 0.0,
            sustain: 0.0,
            pressure: 0.0,
            poly_pressure: 0.0,
            program: 0.0,
            sr: 0.0,
            tempo: 0.0,
            beat: 0.0,
            bar: 0.0,
            ppq: 0.0,
            playing: 0.0,
            voice: 0.0,
            rand: 0.0,
            wave_l: 0.0,
            wave_r: 0.0,
            cc: [0.0; 128],
            params: [0.0; MAX_USER_PARAMETERS],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterSpec {
    pub index: usize,
    pub name: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub cc_link: Option<u8>,
}

impl ParameterSpec {
    pub fn default_normalized(&self) -> f32 {
        self.normalize(self.default)
    }
    pub fn normalize(&self, value: f32) -> f32 {
        ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }
    pub fn denormalize(&self, normalized: f32) -> f32 {
        let value = self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min);
        (self.min + ((value - self.min) / self.step).round() * self.step).clamp(self.min, self.max)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Outputs {
    pub wave: f32,
    pub pan: f32,
    pub l_limit: f32,
    pub wave_l: f32,
    pub wave_r: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteOutputMode {
    Mono,
    Stereo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompileError {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub hint: Option<String>,
}

impl CompileError {
    pub(crate) fn new(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line,
            column,
            hint: None,
        }
    }
    pub(crate) fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}
impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " ({hint})")?;
        }
        Ok(())
    }
}
impl std::error::Error for CompileError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputId {
    T,
    L,
    S,
    Freq,
    Note,
    Ch,
    Bend,
    BendSt,
    Mw,
    Vol,
    MidiPan,
    Mexpr,
    Sustain,
    Pressure,
    PolyPressure,
    Program,
    Sr,
    Tempo,
    Beat,
    Bar,
    Ppq,
    Playing,
    Voice,
    Rand,
    WaveL,
    WaveR,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ValueRef {
    Input(InputId),
    Parameter {
        index: usize,
        min: f32,
        span: f32,
        step: f32,
    },
    Variable(usize),
    State(usize),
    Constant(f32),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    Push(ValueRef),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Neg,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    Sin,
    Cos,
    Tan,
    Exp,
    Sqrt,
    Abs,
    Tanh,
    Sinh,
    Cosh,
    Cbrt,
    Ln,
    Log2,
    Log10,
    Floor,
    Ceil,
    Round,
    Fract,
    Sign,
    Asin,
    Acos,
    Atan,
    Cc,
    Min,
    Max,
    Atan2,
    Clamp,
    Mix,
    Step,
    SmoothStep,
    Select,
    Mtof,
    Ftom,
    DbToA,
    AToDb,
    CentRatio,
    SemitoneRatio,
    Noise,
    Saw,
    Square,
    Pulse,
    Triangle,
    Standard(StandardOp),
    BiquadCoefficient {
        kind: BiquadKind,
        coefficient: u8,
        arity: u8,
    },
    Dsp {
        index: usize,
        arity: u8,
    },
    RingPeek {
        index: usize,
        linear: bool,
    },
    RingLen {
        index: usize,
    },
    RingDuration {
        index: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AssignmentTarget {
    Variable(usize),
    State(usize),
}

#[derive(Clone, Debug)]
pub(crate) struct Assignment {
    pub target: AssignmentTarget,
    pub code: Vec<Op>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum EntryOutputs {
    NoteMono {
        wave: usize,
        pan: Option<usize>,
        l_limit: usize,
    },
    NoteStereo {
        wave_l: usize,
        wave_r: usize,
        l_limit: usize,
    },
    Filter {
        wave_l: usize,
        wave_r: usize,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct EntryPlan {
    pub assignments: Vec<Assignment>,
    pub variable_count: usize,
    pub outputs: EntryOutputs,
}

#[derive(Clone, Debug)]
pub struct Program {
    has_filter: bool,
    state_schema: Arc<[StorageSpec]>,
    variable_count: usize,
    operation_count: usize,
    performance_warnings: Vec<String>,
    parameters: Vec<ParameterSpec>,
    note_output_mode: NoteOutputMode,
    parallel_voice_safe: bool,
    jit: Arc<jit::JitProgram>,
}

impl Program {
    pub fn instantiate(
        &self,
        sample_rate: f32,
        previous: Option<&StateMigrationHandle>,
    ) -> Result<ProgramInstance, String> {
        let sample_rate = sanitize_sample_rate(sample_rate)?;
        Ok(ProgramInstance {
            program: self.clone(),
            state: RuntimeState::prepare(self.state_schema.clone(), sample_rate, previous)?,
            sample_rate,
        })
    }
    /// Worker用instanceです。Voice stateとGlobal scalarは共有し、Global
    /// RingBuf/DSPはworkerごとのrelaxed shardとして準備されます。
    pub fn instantiate_worker(
        &self,
        sample_rate: f32,
        previous: Option<&StateMigrationHandle>,
    ) -> Result<ProgramInstance, String> {
        let sample_rate = sanitize_sample_rate(sample_rate)?;
        Ok(ProgramInstance {
            program: self.clone(),
            state: RuntimeState::prepare_worker(self.state_schema.clone(), sample_rate, previous)?,
            sample_rate,
        })
    }
    pub fn evaluate(&self, input: &Inputs) -> Outputs {
        let mut instance = self
            .instantiate(input.sr.max(48_000.0), None)
            .expect("compiled program must prepare");
        instance.evaluate_note(input, 0, 0)
    }
    pub fn evaluation_scratch(&self) -> EvaluationScratch {
        EvaluationScratch::default()
    }
    pub fn evaluate_with(&self, input: &Inputs, scratch: &mut EvaluationScratch) -> Outputs {
        let identity = Arc::as_ptr(&self.jit) as usize;
        let sample_rate = input.sr.max(48_000.0);
        let recreate = scratch.identity != identity
            || scratch
                .instance
                .as_ref()
                .is_none_or(|i| i.sample_rate.to_bits() != sample_rate.to_bits());
        if recreate {
            scratch.instance = self.instantiate(sample_rate, None).ok();
            scratch.identity = identity;
        }
        scratch
            .instance
            .as_mut()
            .map_or_else(Outputs::default, |instance| {
                let output = instance.evaluate_note(input, 0, 0);
                instance.commit_voice(0);
                instance.commit_note(0);
                instance.commit_global();
                output
            })
    }
    pub fn execution_backend(&self) -> &'static str {
        "Cranelift JIT"
    }
    pub fn variable_count(&self) -> usize {
        self.variable_count
    }
    pub fn parameter_specs(&self) -> &[ParameterSpec] {
        &self.parameters
    }
    pub fn operation_count(&self) -> usize {
        self.operation_count
    }
    pub fn performance_warnings(&self) -> &[String] {
        &self.performance_warnings
    }
    pub fn note_output_mode(&self) -> NoteOutputMode {
        self.note_output_mode
    }
    pub fn has_filter(&self) -> bool {
        self.has_filter
    }
    /// `note` がworkerから同時評価できるかを表します。Global Ring/DSPは
    /// workerごとのrelaxed shardとして扱われ、同一noteで共有されるNote
    /// stateだけが直列fallbackになります。
    pub fn parallel_voice_safe(&self) -> bool {
        self.parallel_voice_safe
    }
}

#[derive(Debug, Default)]
pub struct EvaluationScratch {
    identity: usize,
    instance: Option<ProgramInstance>,
}

#[derive(Debug)]
pub struct ProgramInstance {
    program: Program,
    state: RuntimeState,
    sample_rate: f32,
}

impl ProgramInstance {
    #[inline]
    pub fn evaluate_note(
        &mut self,
        input: &Inputs,
        voice_slot: usize,
        note_slot: usize,
    ) -> Outputs {
        let mut context = self.state.context(voice_slot, note_slot);
        self.program.jit.evaluate_note(input, &mut context)
    }
    #[inline]
    pub fn evaluate_filter(&mut self, input: &Inputs) -> Outputs {
        if !self.program.has_filter() {
            return Outputs {
                wave_l: input.wave_l,
                wave_r: input.wave_r,
                ..Outputs::default()
            };
        }
        let mut context = self.state.context(0, 0);
        self.program.jit.evaluate_filter(input, &mut context)
    }
    #[inline]
    pub fn commit_voice(&mut self, slot: usize) {
        self.state.commit_voice(slot);
    }
    #[inline]
    pub fn commit_note(&mut self, slot: usize) {
        self.state.commit_note(slot);
    }
    #[inline]
    pub fn commit_global(&mut self) {
        self.state.commit_global();
    }
    pub fn reset_voice(&mut self, slot: usize) {
        self.state.reset_voice(slot);
    }
    pub fn reset_note(&mut self, slot: usize) {
        self.state.reset_note(slot);
    }
    pub fn reset_all(&mut self) {
        self.state.reset_all();
    }
    pub fn migration_handle(&self) -> StateMigrationHandle {
        self.state.migration_handle()
    }
    pub fn program(&self) -> &Program {
        &self.program
    }
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
}

fn sanitize_sample_rate(sample_rate: f32) -> Result<f32, String> {
    if sample_rate.is_finite() && sample_rate >= 1_000.0 {
        Ok(sample_rate)
    } else {
        Err("sample rateは1000 Hz以上の有限値である必要があります".into())
    }
}

pub struct Compiler;
impl Compiler {
    pub fn new() -> Self {
        Self
    }
    pub fn compile(&self, source: &str) -> Result<Program, CompileError> {
        compile_ast(&syntax::parse(source)?)
    }
}
impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

fn compile_ast(ast: &ProgramAst) -> Result<Program, CompileError> {
    let parameters = compile_parameters(ast)?;
    let parameter_indices = parameters
        .iter()
        .map(|p| (p.name.clone(), p.index))
        .collect::<HashMap<_, _>>();
    let mut function_indices = HashMap::new();
    for (index, function) in ast.functions.iter().enumerate() {
        if is_builtin(&function.name) || function.name == "param" {
            return Err(error_at(
                function.span,
                format!("built-in名 '{}' は関数名に使用できません", function.name),
            ));
        }
        if function_indices
            .insert(function.name.clone(), index)
            .is_some()
        {
            return Err(error_at(
                function.span,
                format!("関数 '{}' が重複しています", function.name),
            ));
        }
        validate_function_signature(function)?;
    }
    validate_call_graph(ast, &function_indices)?;
    let note_index = function_indices.get("note").copied().ok_or_else(|| {
        CompileError::new("fn note(in, p) -> out が必要です", 1, 1)
            .with_hint("すべてのprogramにnote Entry Pointを1つ定義してください")
    })?;
    let filter_index = function_indices.get("filter").copied();
    let (schema, storage_indices, dsp_indices) =
        collect_storage(ast, &function_indices, note_index, filter_index)?;
    let state_schema: Arc<[StorageSpec]> = schema.into();

    let mut lowerer = Lowerer::new(
        ast,
        &function_indices,
        &parameter_indices,
        &parameters,
        &storage_indices,
        &dsp_indices,
        &state_schema,
        EntryKind::Note,
    );
    let note = lowerer.lower_entry(note_index)?;
    let filter = if let Some(index) = filter_index {
        let mut lowerer = Lowerer::new(
            ast,
            &function_indices,
            &parameter_indices,
            &parameters,
            &storage_indices,
            &dsp_indices,
            &state_schema,
            EntryKind::Filter,
        );
        Some(lowerer.lower_entry(index)?)
    } else {
        None
    };
    let note_output_mode = match note.outputs {
        EntryOutputs::NoteMono { .. } => NoteOutputMode::Mono,
        EntryOutputs::NoteStereo { .. } => NoteOutputMode::Stereo,
        EntryOutputs::Filter { .. } => unreachable!(),
    };
    let variable_count = note.variable_count + filter.as_ref().map_or(0, |e| e.variable_count);
    let operation_count = note
        .assignments
        .iter()
        .chain(filter.iter().flat_map(|e| e.assignments.iter()))
        .map(|a| a.code.len())
        .sum::<usize>();
    let max_stack = note
        .assignments
        .iter()
        .chain(filter.iter().flat_map(|e| e.assignments.iter()))
        .map(|a| expression_stack_depth(&a.code))
        .max()
        .unwrap_or(1);
    let mut performance_warnings = Vec::new();
    if variable_count > VARIABLE_WARNING_THRESHOLD {
        performance_warnings.push(format!(
            "ローカル値が {variable_count} 個あります。多音時のCPU負荷に影響する可能性があります。"
        ));
    }
    if operation_count > OPERATION_WARNING_THRESHOLD {
        performance_warnings.push(format!(
            "1 sampleのJIT IRに {operation_count} 演算あります。CPU負荷と最大発音数に影響する可能性があります。"
        ));
    }
    if max_stack > STACK_WARNING_THRESHOLD {
        performance_warnings.push(format!(
            "最大expression stackが {max_stack} です。長い式は分割すると調整しやすくなります。"
        ));
    }
    if state_schema.len() > STORAGE_WARNING_THRESHOLD {
        performance_warnings.push(format!(
            "persistent storageが {} 個あります。特にVoice/Note RingBufはメモリ使用量へ影響します。",
            state_schema.len()
        ));
    }
    for storage in state_schema.iter() {
        if let StorageKind::Ring {
            capacity: RingCapacity::Seconds(seconds),
        } = storage.kind
        {
            let instances = match storage.domain {
                StorageDomain::Voice | StorageDomain::Note => RUNTIME_STATE_SLOTS,
                StorageDomain::Global => 1,
            };
            let bytes = f64::from(seconds) * 48_000.0 * instances as f64 * 4.0;
            if bytes >= 16.0 * 1024.0 * 1024.0 {
                performance_warnings.push(format!(
                    "RingBuf '{}' は48 kHz時に約 {:.1} MiBを使用します。",
                    storage.source_name,
                    bytes / (1024.0 * 1024.0)
                ));
            }
        }
        if let StorageKind::Dsp { kind } = storage.kind {
            let instances = match storage.domain {
                StorageDomain::Voice | StorageDomain::Note => RUNTIME_STATE_SLOTS,
                StorageDomain::Global => 1,
            };
            let seconds = kind.ring_durations().iter().sum::<f32>();
            let bytes = f64::from(seconds) * 48_000.0 * instances as f64 * 4.0;
            if bytes >= 16.0 * 1024.0 * 1024.0 {
                performance_warnings.push(format!(
                    "標準DSP '{}' は48 kHz時に約 {:.1} MiBの事前確保memoryを使用します。",
                    storage.source_name,
                    bytes / (1024.0 * 1024.0)
                ));
            }
        }
    }
    let jit = jit::JitProgram::compile(&note, filter.as_ref()).map_err(|message| {
        CompileError::new(format!("Cranelift JIT compilation failed: {message}"), 1, 1)
            .with_hint("実行環境がCranelift native JITに対応しているか確認してください")
    })?;
    Ok(Program {
        has_filter: filter.is_some(),
        parallel_voice_safe: note_parallel_safe(&note, &state_schema),
        state_schema,
        variable_count,
        operation_count,
        performance_warnings,
        parameters,
        note_output_mode,
        jit: Arc::new(jit),
    })
}

fn note_parallel_safe(note: &EntryPlan, schema: &[StorageSpec]) -> bool {
    let permits = |index: usize| {
        schema
            .get(index)
            .is_some_and(|storage| !matches!(storage.domain, StorageDomain::Note))
    };
    note.assignments.iter().all(|assignment| {
        let target_safe = match assignment.target {
            AssignmentTarget::Variable(_) => true,
            AssignmentTarget::State(index) => permits(index),
        };
        target_safe
            && assignment.code.iter().all(|operation| match operation {
                Op::Push(ValueRef::State(index))
                | Op::Dsp { index, .. }
                | Op::RingPeek { index, .. }
                | Op::RingLen { index }
                | Op::RingDuration { index } => permits(*index),
                _ => true,
            })
    })
}

fn validate_call_graph(
    ast: &ProgramAst,
    function_indices: &HashMap<String, usize>,
) -> Result<(), CompileError> {
    fn visit(
        index: usize,
        ast: &ProgramAst,
        function_indices: &HashMap<String, usize>,
        states: &mut [u8],
        path: &mut Vec<usize>,
    ) -> Result<(), CompileError> {
        if states[index] == 2 {
            return Ok(());
        }
        if states[index] == 1 {
            let start = path
                .iter()
                .position(|candidate| *candidate == index)
                .unwrap_or(0);
            let mut names = path[start..]
                .iter()
                .map(|candidate| ast.functions[*candidate].name.as_str())
                .collect::<Vec<_>>();
            names.push(&ast.functions[index].name);
            return Err(error_at(
                ast.functions[index].span,
                format!("再帰呼び出しは禁止されています: {}", names.join(" -> ")),
            ));
        }
        states[index] = 1;
        path.push(index);
        for statement in &ast.functions[index].statements {
            if let Statement::Assignment { value, .. } = statement
                && let Expr::Call { name, .. } = value
                && let Some(next) = function_indices.get(name)
            {
                visit(*next, ast, function_indices, states, path)?;
            }
        }
        path.pop();
        states[index] = 2;
        Ok(())
    }

    let mut states = vec![0; ast.functions.len()];
    let mut path = Vec::new();
    for index in 0..ast.functions.len() {
        visit(index, ast, function_indices, &mut states, &mut path)?;
    }
    Ok(())
}

fn compile_parameters(ast: &ProgramAst) -> Result<Vec<ParameterSpec>, CompileError> {
    let mut parameters = Vec::new();
    let mut names = HashSet::new();
    for declaration in &ast.parameters {
        if declaration.name == "p" || !declaration.name.starts_with("p.") {
            return Err(error_at(
                declaration.span,
                "parameter名は p.name の形で指定してください",
            ));
        }
        if !names.insert(declaration.name.clone()) {
            return Err(error_at(
                declaration.span,
                format!("parameter '{}' が重複しています", declaration.name),
            ));
        }
        if parameters.len() >= MAX_USER_PARAMETERS {
            return Err(error_at(
                declaration.span,
                format!("parameterは最大{MAX_USER_PARAMETERS}個です"),
            ));
        }
        if !(4..=5).contains(&declaration.arguments.len()) {
            return Err(error_at(
                declaration.span,
                "param() は default, min, max, step と省略可能なcc_linkを受け取ります",
            ));
        }
        let values = declaration
            .arguments
            .iter()
            .map(evaluate_constant)
            .collect::<Result<Vec<_>, _>>()?;
        let (default, min, max, step) = (values[0], values[1], values[2], values[3]);
        if min >= max {
            return Err(error_at(
                declaration.span,
                "param()のminはmaxより小さくする必要があります",
            ));
        }
        if !(min..=max).contains(&default) {
            return Err(error_at(
                declaration.span,
                "param()のdefaultはmin..max範囲内にする必要があります",
            ));
        }
        if step <= 0.0 || step > max - min {
            return Err(error_at(
                declaration.span,
                "param()のstepは0より大きくrange以下にする必要があります",
            ));
        }
        let cc_link = if values.len() == 5 {
            let value = values[4];
            if value.fract() != 0.0 || !(0.0..=127.0).contains(&value) {
                return Err(error_at(
                    declaration.arguments[4].span(),
                    "cc_linkは0..127の整数MIDI CC番号にしてください",
                ));
            }
            Some(value as u8)
        } else {
            None
        };
        parameters.push(ParameterSpec {
            index: parameters.len(),
            name: declaration.name.clone(),
            default,
            min,
            max,
            step,
            cc_link,
        });
    }
    Ok(parameters)
}

fn validate_function_signature(function: &Function) -> Result<(), CompileError> {
    let expected: &[&str] = if matches!(function.name.as_str(), "note" | "filter") {
        &["in", "p"]
    } else {
        &["in"]
    };
    if function
        .parameters
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        Ok(())
    } else {
        Err(error_at(
            function.span,
            if matches!(function.name.as_str(), "note" | "filter") {
                format!(
                    "Entry Point '{}' の引数は (in, p) に固定されています",
                    function.name
                )
            } else {
                format!(
                    "通常関数 '{}' の引数は (in) に固定されています",
                    function.name
                )
            },
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EntryKind {
    Note,
    Filter,
}

type StorageLookup = HashMap<(String, String), usize>;
type DspLookup = HashMap<(EntryKind, usize, usize, usize, usize), usize>;

fn collect_storage(
    ast: &ProgramAst,
    function_indices: &HashMap<String, usize>,
    note_index: usize,
    filter_index: Option<usize>,
) -> Result<(Vec<StorageSpec>, StorageLookup, DspLookup), CompileError> {
    let mut schema = Vec::new();
    let mut indices = HashMap::new();
    for function in &ast.functions {
        for statement in &function.statements {
            let (domain_text, name, kind, span) = match statement {
                Statement::ScalarStorage {
                    domain,
                    name,
                    initializer,
                    span,
                } => (
                    domain,
                    name,
                    StorageKind::Scalar {
                        initial: evaluate_constant(initializer)?,
                    },
                    *span,
                ),
                Statement::RingStorage {
                    domain,
                    name,
                    size,
                    span,
                } => {
                    if size.value <= 0.0 {
                        return Err(error_at(size.span, "RingBuf容量は0より大きくしてください"));
                    }
                    let capacity = match size.unit {
                        LiteralUnit::Seconds => RingCapacity::Seconds(size.value),
                        LiteralUnit::Plain if size.integral_without_suffix => {
                            RingCapacity::Samples(size.value as usize)
                        }
                        LiteralUnit::Plain => {
                            return Err(error_at(
                                size.span,
                                "suffixなしのRingBuf容量は正の整数にしてください",
                            ));
                        }
                    };
                    (domain, name, StorageKind::Ring { capacity }, *span)
                }
                Statement::Assignment { .. } => continue,
            };
            let domain = StorageDomain::parse(domain_text).ok_or_else(|| {
                error_at(
                    span,
                    format!("不明なstorage domain '{domain_text}' です（voice / note / global）"),
                )
            })?;
            if name.starts_with("in.") || name.starts_with("out.") || name.starts_with("p.") {
                return Err(error_at(span, "storage名に予約prefixは使用できません"));
            }
            let lookup_key = (function.name.clone(), name.clone());
            if indices.contains_key(&lookup_key) {
                return Err(error_at(
                    span,
                    format!("storage '{name}' が関数内で重複しています"),
                ));
            }
            let index = schema.len();
            indices.insert(lookup_key, index);
            schema.push(StorageSpec {
                key: format!("{}::{name}", function.name),
                source_name: name.clone(),
                domain,
                kind,
            });
        }
    }

    let mut dsp_indices = HashMap::new();
    let mut visited = HashSet::new();
    collect_function_dsp(
        ast,
        function_indices,
        note_index,
        EntryKind::Note,
        StorageDomain::Voice,
        &mut visited,
        &mut schema,
        &mut dsp_indices,
    );
    if let Some(filter_index) = filter_index {
        collect_function_dsp(
            ast,
            function_indices,
            filter_index,
            EntryKind::Filter,
            StorageDomain::Global,
            &mut visited,
            &mut schema,
            &mut dsp_indices,
        );
    }
    Ok((schema, indices, dsp_indices))
}

#[allow(clippy::too_many_arguments)]
fn collect_function_dsp(
    ast: &ProgramAst,
    function_indices: &HashMap<String, usize>,
    function_index: usize,
    entry_kind: EntryKind,
    domain: StorageDomain,
    visited: &mut HashSet<(EntryKind, usize)>,
    schema: &mut Vec<StorageSpec>,
    indices: &mut DspLookup,
) {
    if !visited.insert((entry_kind, function_index)) {
        return;
    }
    for statement in &ast.functions[function_index].statements {
        let Statement::Assignment { value, .. } = statement else {
            continue;
        };
        collect_expression_dsp(value, function_index, entry_kind, domain, schema, indices);
        if let Expr::Call { name, .. } = value
            && let Some(&callee) = function_indices.get(name)
        {
            collect_function_dsp(
                ast,
                function_indices,
                callee,
                entry_kind,
                domain,
                visited,
                schema,
                indices,
            );
        }
    }
}

fn collect_expression_dsp(
    expression: &Expr,
    function_index: usize,
    entry_kind: EntryKind,
    domain: StorageDomain,
    schema: &mut Vec<StorageSpec>,
    indices: &mut DspLookup,
) {
    match expression {
        Expr::Call {
            name,
            arguments,
            span,
        } => {
            if let Some(kind) = dsp_kind(name, arguments.len()) {
                insert_implicit_dsp(
                    entry_kind,
                    function_index,
                    *span,
                    0,
                    name,
                    kind,
                    domain,
                    schema,
                    indices,
                );
            } else if name == "delay.multitap" && (2..=9).contains(&arguments.len()) {
                for tap in 0..arguments.len() - 1 {
                    insert_implicit_dsp(
                        entry_kind,
                        function_index,
                        *span,
                        tap,
                        name,
                        DspKind::DelayVariable,
                        domain,
                        schema,
                        indices,
                    );
                }
            }
            for argument in arguments {
                collect_expression_dsp(
                    argument,
                    function_index,
                    entry_kind,
                    domain,
                    schema,
                    indices,
                );
            }
        }
        Expr::Unary { value, .. } => {
            collect_expression_dsp(value, function_index, entry_kind, domain, schema, indices)
        }
        Expr::Binary { left, right, .. } => {
            collect_expression_dsp(left, function_index, entry_kind, domain, schema, indices);
            collect_expression_dsp(right, function_index, entry_kind, domain, schema, indices);
        }
        Expr::Literal(_) | Expr::Name(_, _) => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_implicit_dsp(
    entry_kind: EntryKind,
    function_index: usize,
    span: Span,
    slot: usize,
    source_name: &str,
    kind: DspKind,
    domain: StorageDomain,
    schema: &mut Vec<StorageSpec>,
    indices: &mut DspLookup,
) {
    let key = (entry_kind, function_index, span.line, span.column, slot);
    if indices.contains_key(&key) {
        return;
    }
    let index = schema.len();
    indices.insert(key, index);
    schema.push(StorageSpec {
        key: format!(
            "@dsp::{entry_kind:?}::{function_index}::{}::{}::{slot}",
            span.line, span.column
        ),
        source_name: format!("{source_name}@{}:{}", span.line, span.column),
        domain,
        kind: StorageKind::Dsp { kind },
    });
}

struct Scope {
    function_index: usize,
    input: HashMap<String, ValueRef>,
    locals: HashMap<String, usize>,
    outputs: HashMap<String, usize>,
    has_parameters: bool,
    is_entry: bool,
}

struct Lowerer<'a> {
    ast: &'a ProgramAst,
    function_indices: &'a HashMap<String, usize>,
    parameter_indices: &'a HashMap<String, usize>,
    parameters: &'a [ParameterSpec],
    storage_indices: &'a HashMap<(String, String), usize>,
    dsp_indices: &'a DspLookup,
    schema: &'a [StorageSpec],
    entry_kind: EntryKind,
    assignments: Vec<Assignment>,
    next_slot: usize,
    call_stack: Vec<usize>,
}

impl<'a> Lowerer<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ast: &'a ProgramAst,
        function_indices: &'a HashMap<String, usize>,
        parameter_indices: &'a HashMap<String, usize>,
        parameters: &'a [ParameterSpec],
        storage_indices: &'a HashMap<(String, String), usize>,
        dsp_indices: &'a DspLookup,
        schema: &'a [StorageSpec],
        entry_kind: EntryKind,
    ) -> Self {
        Self {
            ast,
            function_indices,
            parameter_indices,
            parameters,
            storage_indices,
            dsp_indices,
            schema,
            entry_kind,
            assignments: Vec::new(),
            next_slot: 0,
            call_stack: Vec::new(),
        }
    }

    fn lower_entry(&mut self, function_index: usize) -> Result<EntryPlan, CompileError> {
        let outputs =
            self.lower_function(function_index, entry_input_map(self.entry_kind), true, true)?;
        let span = self.ast.functions[function_index].span;
        let outputs = match self.entry_kind {
            EntryKind::Note => validate_note_outputs(outputs, span)?,
            EntryKind::Filter => validate_filter_outputs(outputs, span)?,
        };
        Ok(EntryPlan {
            assignments: std::mem::take(&mut self.assignments),
            variable_count: self.next_slot,
            outputs,
        })
    }

    fn lower_function(
        &mut self,
        function_index: usize,
        input: HashMap<String, ValueRef>,
        has_parameters: bool,
        is_entry: bool,
    ) -> Result<HashMap<String, usize>, CompileError> {
        let function = &self.ast.functions[function_index];
        if self.call_stack.contains(&function_index) {
            let mut path = self
                .call_stack
                .iter()
                .map(|i| self.ast.functions[*i].name.as_str())
                .collect::<Vec<_>>();
            path.push(&function.name);
            return Err(error_at(
                function.span,
                format!("再帰呼び出しは禁止されています: {}", path.join(" -> ")),
            ));
        }
        self.call_stack.push(function_index);
        let mut scope = Scope {
            function_index,
            input,
            locals: HashMap::new(),
            outputs: HashMap::new(),
            has_parameters,
            is_entry,
        };
        for statement in &function.statements {
            if let Statement::Assignment {
                targets,
                value,
                span,
            } = statement
            {
                if let Expr::Call {
                    name,
                    arguments,
                    span: call_span,
                } = value
                {
                    if let Some(&callee) = self.function_indices.get(name) {
                        self.lower_user_call(
                            &mut scope, targets, callee, arguments, *span, *call_span,
                        )?;
                    } else if !self.lower_standard_bundle(
                        &mut scope, targets, name, arguments, *span, *call_span,
                    )? {
                        self.lower_assignment(&mut scope, targets, value, *span)?;
                    }
                } else {
                    self.lower_assignment(&mut scope, targets, value, *span)?;
                }
            }
        }
        self.call_stack.pop();
        Ok(scope.outputs)
    }

    fn lower_user_call(
        &mut self,
        scope: &mut Scope,
        targets: &[String],
        callee: usize,
        arguments: &[Expr],
        span: Span,
        call_span: Span,
    ) -> Result<(), CompileError> {
        if targets.len() != 1 {
            return Err(error_at(
                span,
                "user function callの代入先は1つのresult prefixにしてください",
            ));
        }
        let result_prefix = &targets[0];
        if result_prefix.starts_with("in.")
            || result_prefix.starts_with("out.")
            || result_prefix.starts_with("p.")
            || self.storage_index(scope, result_prefix).is_some()
        {
            return Err(error_at(
                span,
                "この名前はcall result prefixに使用できません",
            ));
        }
        if arguments.len() != 1 {
            return Err(error_at(
                call_span,
                "user functionは1つのinput bundleを受け取ります",
            ));
        }
        let Expr::Name(input_prefix, argument_span) = &arguments[0] else {
            return Err(error_at(
                arguments[0].span(),
                "user function引数にはbundle prefixを指定してください",
            ));
        };
        let input = if input_prefix == "in" {
            scope.input.clone()
        } else {
            let prefix = format!("{input_prefix}.");
            scope
                .locals
                .iter()
                .filter_map(|(name, slot)| {
                    name.strip_prefix(&prefix)
                        .map(|field| (field.to_owned(), ValueRef::Variable(*slot)))
                })
                .collect()
        };
        if input.is_empty() {
            return Err(error_at(
                *argument_span,
                format!("input bundle '{input_prefix}.*' にfieldがありません"),
            ));
        }
        let callee_outputs = self.lower_function(callee, input, false, false)?;
        for (field, slot) in callee_outputs {
            let name = format!("{result_prefix}.{field}");
            if scope.locals.contains_key(&name) {
                return Err(error_at(
                    span,
                    format!("call result '{name}' が既存localと衝突します"),
                ));
            }
            scope.locals.insert(name, slot);
        }
        Ok(())
    }

    fn lower_standard_bundle(
        &mut self,
        scope: &mut Scope,
        targets: &[String],
        name: &str,
        arguments: &[Expr],
        span: Span,
        call_span: Span,
    ) -> Result<bool, CompileError> {
        let is_biquad = biquad_kind(name, arguments.len());
        let is_pan = name == "pan.equal_power" && arguments.len() == 2;
        let is_width = name == "stereo.width" && arguments.len() == 3;
        let is_multitap = name == "delay.multitap" && (2..=9).contains(&arguments.len());
        if is_biquad.is_none() && !is_pan && !is_width && !is_multitap {
            return Ok(false);
        }
        if targets.len() != 1 {
            return Err(error_at(
                span,
                "bundleを返す標準関数の代入先は1つのresult prefixにしてください",
            ));
        }
        let prefix = &targets[0];
        if prefix == "in"
            || prefix == "out"
            || prefix == "p"
            || prefix.starts_with("in.")
            || prefix.starts_with("out.")
            || prefix.starts_with("p.")
            || self.storage_index(scope, prefix).is_some()
        {
            return Err(error_at(
                span,
                "この名前はstandard bundle result prefixに使用できません",
            ));
        }

        if let Some(kind) = is_biquad {
            for (coefficient, field) in ["b0", "b1", "b2", "a1", "a2"].iter().enumerate() {
                let mut code = Vec::new();
                for argument in arguments {
                    self.lower_expression(scope, argument, &mut code)?;
                }
                code.push(Op::BiquadCoefficient {
                    kind,
                    coefficient: coefficient as u8,
                    arity: arguments.len() as u8,
                });
                self.insert_bundle_field(scope, prefix, field, code, span)?;
            }
        } else if is_pan {
            for (field, operation) in [
                ("left", StandardOp::PanSignalL),
                ("right", StandardOp::PanSignalR),
            ] {
                let mut code = Vec::new();
                for argument in arguments {
                    self.lower_expression(scope, argument, &mut code)?;
                }
                code.push(Op::Standard(operation));
                self.insert_bundle_field(scope, prefix, field, code, span)?;
            }
        } else if is_width {
            for (field, operation) in [
                ("left", StandardOp::StereoWidthL),
                ("right", StandardOp::StereoWidthR),
            ] {
                let mut code = Vec::new();
                for argument in arguments {
                    self.lower_expression(scope, argument, &mut code)?;
                }
                code.push(Op::Standard(operation));
                self.insert_bundle_field(scope, prefix, field, code, span)?;
            }
        } else {
            for tap in 0..arguments.len() - 1 {
                let index = self.implicit_dsp_index(scope, call_span, tap)?;
                let mut code = Vec::new();
                self.lower_expression(scope, &arguments[0], &mut code)?;
                self.lower_expression(scope, &arguments[tap + 1], &mut code)?;
                code.push(Op::Dsp { index, arity: 2 });
                self.insert_bundle_field(scope, prefix, &format!("tap{}", tap + 1), code, span)?;
            }
        }
        Ok(true)
    }

    fn insert_bundle_field(
        &mut self,
        scope: &mut Scope,
        prefix: &str,
        field: &str,
        code: Vec<Op>,
        span: Span,
    ) -> Result<(), CompileError> {
        let name = format!("{prefix}.{field}");
        if scope.locals.contains_key(&name) {
            return Err(error_at(
                span,
                format!("bundle result '{name}' が既存localと衝突します"),
            ));
        }
        let slot = self.allocate_slot();
        self.assignments.push(Assignment {
            target: AssignmentTarget::Variable(slot),
            code,
        });
        scope.locals.insert(name, slot);
        Ok(())
    }

    fn lower_assignment(
        &mut self,
        scope: &mut Scope,
        targets: &[String],
        value: &Expr,
        span: Span,
    ) -> Result<(), CompileError> {
        let mut code = Vec::new();
        self.lower_expression(scope, value, &mut code)?;
        let slot = self.allocate_slot();
        self.assignments.push(Assignment {
            target: AssignmentTarget::Variable(slot),
            code,
        });
        for target in targets.iter().rev() {
            if target == "in" || target.starts_with("in.") {
                return Err(error_at(span, "in.* はread-onlyです"));
            }
            if target == "p" || target.starts_with("p.") {
                return Err(error_at(
                    span,
                    "p.* はトップレベルのparam宣言以外ではread-onlyです",
                ));
            }
            if let Some(field) = target.strip_prefix("out.") {
                if field.is_empty() {
                    return Err(error_at(span, "out field名が必要です"));
                }
                if scope.outputs.insert(field.to_owned(), slot).is_some() {
                    return Err(error_at(
                        span,
                        format!("out.{field} へ複数回writeしています"),
                    ));
                }
            } else if target == "out" {
                return Err(error_at(span, "outにはfield名が必要です"));
            } else if let Some(index) = self.storage_index(scope, target) {
                self.ensure_storage_domain(index, span)?;
                self.assignments.push(Assignment {
                    target: AssignmentTarget::State(index),
                    code: vec![Op::Push(ValueRef::Variable(slot))],
                });
            } else {
                if is_constant_name(target) {
                    return Err(error_at(span, "定数へ代入できません"));
                }
                scope.locals.insert(target.clone(), slot);
            }
        }
        Ok(())
    }

    fn lower_expression(
        &self,
        scope: &Scope,
        expression: &Expr,
        code: &mut Vec<Op>,
    ) -> Result<(), CompileError> {
        match expression {
            Expr::Literal(value) => code.push(Op::Push(ValueRef::Constant(value.value))),
            Expr::Name(name, span) => code.push(Op::Push(self.resolve_name(scope, name, *span)?)),
            Expr::Unary { op, value, .. } => {
                self.lower_expression(scope, value, code)?;
                if matches!(op, UnaryOp::Negative) {
                    code.push(Op::Neg);
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                if op.is_comparison()
                    && (matches!(&**left, Expr::Binary { op, .. } if op.is_comparison())
                        || matches!(&**right, Expr::Binary { op, .. } if op.is_comparison()))
                {
                    return Err(error_at(*span, "比較演算の連鎖は使用できません"));
                }
                self.lower_expression(scope, left, code)?;
                self.lower_expression(scope, right, code)?;
                code.push(match op {
                    BinaryOp::Add => Op::Add,
                    BinaryOp::Subtract => Op::Sub,
                    BinaryOp::Multiply => Op::Mul,
                    BinaryOp::Divide => Op::Div,
                    BinaryOp::Modulo => Op::Mod,
                    BinaryOp::Power => Op::Pow,
                    BinaryOp::Less => Op::Less,
                    BinaryOp::LessEqual => Op::LessEqual,
                    BinaryOp::Greater => Op::Greater,
                    BinaryOp::GreaterEqual => Op::GreaterEqual,
                    BinaryOp::Equal => Op::Equal,
                    BinaryOp::NotEqual => Op::NotEqual,
                });
            }
            Expr::Call {
                name,
                arguments,
                span,
            } => {
                if self.function_indices.contains_key(name) {
                    return Err(error_at(
                        *span,
                        "user function callは result = function(input_bundle) statementとして記述してください",
                    ));
                }
                if name == "param" {
                    return Err(error_at(
                        *span,
                        "param()はファイル先頭の p.name 宣言でのみ使用できます",
                    ));
                }
                if name == "in.cc" {
                    if !scope.is_entry {
                        return Err(error_at(
                            *span,
                            "in.cc()はEntry Point内で使用し、通常関数へは値をbundleで渡してください",
                        ));
                    }
                    expect_arity(name, arguments, 1, *span)?;
                    self.lower_expression(scope, &arguments[0], code)?;
                    code.push(Op::Cc);
                } else if let Some((receiver, method)) = name.rsplit_once('.')
                    && matches!(method, "peek" | "peek_linear" | "len" | "duration")
                {
                    let index = self.storage_index(scope, receiver).ok_or_else(|| {
                        error_at(
                            *span,
                            format!("RingBuf '{receiver}' がこの関数内にありません"),
                        )
                    })?;
                    self.ensure_storage_domain(index, *span)?;
                    if !matches!(self.schema[index].kind, StorageKind::Ring { .. }) {
                        return Err(error_at(
                            *span,
                            format!("'{receiver}' はRingBufではありません"),
                        ));
                    }
                    match method {
                        "peek" | "peek_linear" => {
                            expect_arity(name, arguments, 1, *span)?;
                            self.lower_expression(scope, &arguments[0], code)?;
                            code.push(Op::RingPeek {
                                index,
                                linear: method == "peek_linear",
                            });
                        }
                        "len" => {
                            expect_arity(name, arguments, 0, *span)?;
                            code.push(Op::RingLen { index });
                        }
                        "duration" => {
                            expect_arity(name, arguments, 0, *span)?;
                            code.push(Op::RingDuration { index });
                        }
                        _ => unreachable!(),
                    }
                } else if let Some(operation) = standard_operation(name, arguments.len()) {
                    for argument in arguments {
                        self.lower_expression(scope, argument, code)?;
                    }
                    code.push(Op::Standard(operation));
                } else if let Some(kind) = dsp_kind(name, arguments.len()) {
                    for argument in arguments {
                        self.lower_expression(scope, argument, code)?;
                    }
                    let index = self.implicit_dsp_index(scope, *span, 0)?;
                    code.push(Op::Dsp {
                        index,
                        arity: kind.arity() as u8,
                    });
                } else {
                    let operation = builtin_operation(name, arguments.len()).ok_or_else(|| {
                        error_at(
                            *span,
                            format!("不明な関数またはarityです: {name}({})", arguments.len()),
                        )
                    })?;
                    for argument in arguments {
                        self.lower_expression(scope, argument, code)?;
                    }
                    code.push(operation);
                }
            }
        }
        Ok(())
    }

    fn resolve_name(
        &self,
        scope: &Scope,
        name: &str,
        span: Span,
    ) -> Result<ValueRef, CompileError> {
        if let Some(field) = name.strip_prefix("in.") {
            return scope.input.get(field).copied().ok_or_else(|| {
                let mut fields = scope
                    .input
                    .keys()
                    .map(|field| format!("in.{field}"))
                    .collect::<Vec<_>>();
                fields.sort();
                error_at(
                    span,
                    format!("この関数のinput bundleに '{name}' はありません"),
                )
                .with_hint(format!("利用可能な入力: {}", fields.join(", ")))
            });
        }
        if name.starts_with("out.") || name == "out" {
            return Err(error_at(span, "out.* はwrite-onlyです"));
        }
        if name.starts_with("p.") {
            if !scope.has_parameters {
                return Err(error_at(
                    span,
                    "通常関数からp.*は直接参照できません。in.* bundleで値を渡してください",
                ));
            }
            let index = self
                .parameter_indices
                .get(name)
                .copied()
                .ok_or_else(|| error_at(span, format!("未宣言のparameterです: {name}")))?;
            let parameter = &self.parameters[index];
            return Ok(ValueRef::Parameter {
                index,
                min: parameter.min,
                span: parameter.max - parameter.min,
                step: parameter.step,
            });
        }
        if let Some(slot) = scope.locals.get(name) {
            return Ok(ValueRef::Variable(*slot));
        }
        if let Some(index) = self.storage_index(scope, name) {
            self.ensure_storage_domain(index, span)?;
            return Ok(ValueRef::State(index));
        }
        if let Some(value) = constant_value(name) {
            return Ok(ValueRef::Constant(value));
        }
        Err(
            error_at(span, format!("未定義の名前です: {name}")).with_hint(
                "localは最初の代入より後で読み、inputはin.*、parameterはp.*で参照してください",
            ),
        )
    }

    fn storage_index(&self, scope: &Scope, name: &str) -> Option<usize> {
        self.storage_indices
            .get(&(
                self.ast.functions[scope.function_index].name.clone(),
                name.to_owned(),
            ))
            .copied()
    }
    fn implicit_dsp_index(
        &self,
        scope: &Scope,
        span: Span,
        slot: usize,
    ) -> Result<usize, CompileError> {
        self.dsp_indices
            .get(&(
                self.entry_kind,
                scope.function_index,
                span.line,
                span.column,
                slot,
            ))
            .copied()
            .ok_or_else(|| error_at(span, "標準DSPのpersistent stateを解決できませんでした"))
    }
    fn ensure_storage_domain(&self, index: usize, span: Span) -> Result<(), CompileError> {
        if self.entry_kind == EntryKind::Filter
            && self.schema[index].domain != StorageDomain::Global
        {
            Err(error_at(
                span,
                "filter call treeからvoice/note storageへアクセスできません",
            ))
        } else {
            Ok(())
        }
    }
    fn allocate_slot(&mut self) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }
}

fn entry_input_map(kind: EntryKind) -> HashMap<String, ValueRef> {
    let common = [
        ("sr", InputId::Sr),
        ("tempo", InputId::Tempo),
        ("beat", InputId::Beat),
        ("bar", InputId::Bar),
        ("ppq", InputId::Ppq),
        ("playing", InputId::Playing),
        ("mw", InputId::Mw),
        ("vol", InputId::Vol),
        ("midi_pan", InputId::MidiPan),
        ("mexpr", InputId::Mexpr),
        ("sustain", InputId::Sustain),
        ("program", InputId::Program),
    ];
    let mut result = common
        .into_iter()
        .map(|(name, id)| (name.to_owned(), ValueRef::Input(id)))
        .collect::<HashMap<_, _>>();
    let specific: &[(&str, InputId)] = match kind {
        EntryKind::Note => &[
            ("t", InputId::T),
            ("l", InputId::L),
            ("s", InputId::S),
            ("freq", InputId::Freq),
            ("note", InputId::Note),
            ("ch", InputId::Ch),
            ("bend", InputId::Bend),
            ("bend_st", InputId::BendSt),
            ("pressure", InputId::Pressure),
            ("poly_pressure", InputId::PolyPressure),
            ("voice", InputId::Voice),
            ("rand", InputId::Rand),
        ],
        EntryKind::Filter => &[("wave_l", InputId::WaveL), ("wave_r", InputId::WaveR)],
    };
    result.extend(
        specific
            .iter()
            .map(|(name, id)| ((*name).to_owned(), ValueRef::Input(*id))),
    );
    result
}

fn validate_note_outputs(
    mut outputs: HashMap<String, usize>,
    span: Span,
) -> Result<EntryOutputs, CompileError> {
    let l_limit = outputs.remove("l_limit").ok_or_else(|| {
        error_at(span, "noteは out.l_limit を必ず定義してください")
            .with_hint("Voiceを安全に終了するためrelease後の保持秒数が必要です")
    })?;
    let wave = outputs.remove("wave");
    let wave_l = outputs.remove("wave_l");
    let wave_r = outputs.remove("wave_r");
    let pan = outputs.remove("pan");
    if !outputs.is_empty() {
        return Err(error_at(
            span,
            format!(
                "noteに未対応のoutput fieldがあります: {}",
                outputs.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    match (wave, wave_l, wave_r) {
        (Some(wave), None, None) => Ok(EntryOutputs::NoteMono { wave, pan, l_limit }),
        (None, Some(wave_l), Some(wave_r)) if pan.is_none() => Ok(EntryOutputs::NoteStereo {
            wave_l,
            wave_r,
            l_limit,
        }),
        (None, Some(_), Some(_)) => Err(error_at(
            span,
            "true stereo noteでは out.pan を定義できません",
        )),
        _ => Err(error_at(
            span,
            "noteはout.wave、またはout.wave_l/out.wave_rのどちらか一方を完全に定義してください",
        )),
    }
}

fn validate_filter_outputs(
    mut outputs: HashMap<String, usize>,
    span: Span,
) -> Result<EntryOutputs, CompileError> {
    let wave_l = outputs.remove("wave_l");
    let wave_r = outputs.remove("wave_r");
    if !outputs.is_empty() || wave_l.is_none() || wave_r.is_none() {
        return Err(error_at(
            span,
            "filterは out.wave_l と out.wave_r だけを両方定義してください",
        ));
    }
    Ok(EntryOutputs::Filter {
        wave_l: wave_l.unwrap(),
        wave_r: wave_r.unwrap(),
    })
}

fn builtin_operation(name: &str, arity: usize) -> Option<Op> {
    Some(match (name, arity) {
        ("sin", 1) => Op::Sin,
        ("cos", 1) => Op::Cos,
        ("tan", 1) => Op::Tan,
        ("asin", 1) => Op::Asin,
        ("acos", 1) => Op::Acos,
        ("atan", 1) => Op::Atan,
        ("atan2", 2) => Op::Atan2,
        ("sinh", 1) => Op::Sinh,
        ("cosh", 1) => Op::Cosh,
        ("exp", 1) => Op::Exp,
        ("sqrt", 1) => Op::Sqrt,
        ("cbrt", 1) => Op::Cbrt,
        ("abs", 1) => Op::Abs,
        ("tanh", 1) => Op::Tanh,
        ("ln" | "log", 1) => Op::Ln,
        ("log2", 1) => Op::Log2,
        ("log10", 1) => Op::Log10,
        ("floor", 1) => Op::Floor,
        ("ceil", 1) => Op::Ceil,
        ("round", 1) => Op::Round,
        ("fract", 1) => Op::Fract,
        ("sign", 1) => Op::Sign,
        ("min", 2) => Op::Min,
        ("max", 2) => Op::Max,
        ("pow", 2) => Op::Pow,
        ("mod", 2) => Op::Mod,
        ("clamp", 3) => Op::Clamp,
        ("mix", 3) => Op::Mix,
        ("step", 2) => Op::Step,
        ("smoothstep", 3) => Op::SmoothStep,
        ("select", 3) => Op::Select,
        ("mtof", 1) => Op::Mtof,
        ("ftom", 1) => Op::Ftom,
        ("dbtoa", 1) => Op::DbToA,
        ("atodb", 1) => Op::AToDb,
        ("cent_ratio", 1) => Op::CentRatio,
        ("semitone_ratio", 1) => Op::SemitoneRatio,
        ("saw", 2) => Op::Saw,
        ("square", 2) => Op::Square,
        ("pulse", 3) => Op::Pulse,
        ("triangle", 2) => Op::Triangle,
        ("noise", 0) => Op::Noise,
        _ => return None,
    })
}
fn is_builtin(name: &str) -> bool {
    (0..=9).any(|arity| {
        builtin_operation(name, arity).is_some()
            || standard_operation(name, arity).is_some()
            || dsp_kind(name, arity).is_some()
            || biquad_kind(name, arity).is_some()
            || (name == "delay.multitap" && (2..=9).contains(&arity))
    }) || matches!(name, "in.cc" | "pan.equal_power" | "stereo.width")
}
fn expect_arity(
    name: &str,
    args: &[Expr],
    expected: usize,
    span: Span,
) -> Result<(), CompileError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(error_at(span, format!("{name}() は{expected}引数です")))
    }
}
fn constant_value(name: &str) -> Option<f32> {
    Some(match name {
        "TAU" => std::f32::consts::TAU,
        "PI" => std::f32::consts::PI,
        "E" => std::f32::consts::E,
        "PHI" => 1.618_034,
        _ => return None,
    })
}
fn is_constant_name(name: &str) -> bool {
    constant_value(name).is_some()
}

fn evaluate_constant(expression: &Expr) -> Result<f32, CompileError> {
    let value = match expression {
        Expr::Literal(value) => value.value,
        Expr::Name(name, span) => constant_value(name)
            .ok_or_else(|| error_at(*span, "constant expressionからruntime値は参照できません"))?,
        Expr::Unary { op, value, .. } => {
            let value = evaluate_constant(value)?;
            if matches!(op, UnaryOp::Negative) {
                -value
            } else {
                value
            }
        }
        Expr::Binary {
            op, left, right, ..
        } => {
            let (a, b) = (evaluate_constant(left)?, evaluate_constant(right)?);
            match op {
                BinaryOp::Add => a + b,
                BinaryOp::Subtract => a - b,
                BinaryOp::Multiply => a * b,
                BinaryOp::Divide => a / b,
                BinaryOp::Modulo => a % b,
                BinaryOp::Power => a.powf(b),
                BinaryOp::Less => f32::from(a < b),
                BinaryOp::LessEqual => f32::from(a <= b),
                BinaryOp::Greater => f32::from(a > b),
                BinaryOp::GreaterEqual => f32::from(a >= b),
                BinaryOp::Equal => f32::from(a == b),
                BinaryOp::NotEqual => f32::from(a != b),
            }
        }
        Expr::Call {
            name,
            arguments,
            span,
        } => {
            let values = arguments
                .iter()
                .map(evaluate_constant)
                .collect::<Result<Vec<_>, _>>()?;
            evaluate_constant_call(name, &values)
                .ok_or_else(|| error_at(*span, "この関数はconstant expressionで使用できません"))?
        }
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(error_at(
            expression.span(),
            "constant expressionの結果が有限値ではありません",
        ))
    }
}

fn evaluate_constant_call(name: &str, v: &[f32]) -> Option<f32> {
    if let Some(operation) = standard_operation(name, v.len()) {
        let mut arguments = [0.0; 5];
        arguments[..v.len()].copy_from_slice(v);
        return Some(dsp::jit_standard(
            operation as u32,
            arguments[0],
            arguments[1],
            arguments[2],
            arguments[3],
            arguments[4],
        ));
    }
    Some(match (name, v) {
        ("sin", [x]) => x.sin(),
        ("cos", [x]) => x.cos(),
        ("tan", [x]) => x.tan(),
        ("asin", [x]) => x.asin(),
        ("acos", [x]) => x.acos(),
        ("atan", [x]) => x.atan(),
        ("atan2", [x, y]) => x.atan2(*y),
        ("sinh", [x]) => x.sinh(),
        ("cosh", [x]) => x.cosh(),
        ("exp", [x]) => x.exp(),
        ("sqrt", [x]) => x.sqrt(),
        ("cbrt", [x]) => x.cbrt(),
        ("abs", [x]) => x.abs(),
        ("tanh", [x]) => x.tanh(),
        ("ln" | "log", [x]) => x.ln(),
        ("log2", [x]) => x.log2(),
        ("log10", [x]) => x.log10(),
        ("floor", [x]) => x.floor(),
        ("ceil", [x]) => x.ceil(),
        ("round", [x]) => x.round(),
        ("fract", [x]) => x.fract(),
        ("sign", [x]) => x.signum(),
        ("min", [x, y]) => x.min(*y),
        ("max", [x, y]) => x.max(*y),
        ("pow", [x, y]) => x.powf(*y),
        ("mod", [x, y]) => x % y,
        ("clamp", [x, a, b]) => x.clamp(a.min(*b), a.max(*b)),
        ("mix", [x, y, t]) => x + (y - x) * t,
        ("step", [edge, x]) => f32::from(x >= edge),
        ("smoothstep", [a, b, x]) => {
            let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
        ("select", [c, a, b]) => {
            if *c == 0.0 {
                *b
            } else {
                *a
            }
        }
        ("mtof", [n]) => 440.0 * 2.0f32.powf((n - 69.0) / 12.0),
        ("ftom", [f]) => 69.0 + 12.0 * (f / 440.0).log2(),
        ("dbtoa", [d]) => 10.0f32.powf(d / 20.0),
        ("atodb", [a]) => 20.0 * a.abs().log10(),
        ("cent_ratio", [c]) => 2.0f32.powf(c / 1200.0),
        ("semitone_ratio", [s]) => 2.0f32.powf(s / 12.0),
        _ => return None,
    })
}

fn expression_stack_depth(code: &[Op]) -> usize {
    let mut depth = 0usize;
    let mut max = 0;
    for op in code {
        let pops = match op {
            Op::Push(_) | Op::Noise | Op::RingLen { .. } | Op::RingDuration { .. } => 0,
            Op::Standard(operation) => operation.arity(),
            Op::BiquadCoefficient { arity, .. } | Op::Dsp { arity, .. } => *arity as usize,
            Op::RingPeek { .. } => 1,
            Op::Clamp | Op::Mix | Op::SmoothStep | Op::Select | Op::Pulse => 3,
            Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::Pow
            | Op::Less
            | Op::LessEqual
            | Op::Greater
            | Op::GreaterEqual
            | Op::Equal
            | Op::NotEqual
            | Op::Min
            | Op::Max
            | Op::Atan2
            | Op::Step
            | Op::Saw
            | Op::Square
            | Op::Triangle => 2,
            _ => 1,
        };
        depth = depth.saturating_sub(pops) + 1;
        max = max.max(depth);
    }
    max
}
fn error_at(span: Span, message: impl Into<String>) -> CompileError {
    CompileError::new(message, span.line, span.column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_v2_parameter_and_mono_entry() {
        let source = "p.gain = param(0.5, 0, 2, 0.01, 7)\nfn note(in, p) -> out {\nout.wave = p.gain\nout.l_limit = 100ms\n}";
        let program = Compiler::new().compile(source).unwrap();
        assert_eq!(program.note_output_mode(), NoteOutputMode::Mono);
        assert_eq!(program.parameter_specs()[0].cc_link, Some(7));
        let mut input = Inputs {
            sr: 48_000.0,
            ..Inputs::default()
        };
        input.params[0] = 0.25;
        assert!((program.evaluate(&input).wave - 0.5).abs() < 0.01);
    }

    #[test]
    fn state_and_ring_are_continuous() {
        let source = "fn note(in, p) -> out {\nf32 voice counter = 0\nRingBuf<f32, 2> voice delay\nold = delay\ndelay = counter\ncounter = counter + 1\nout.wave = old\nout.l_limit = 1\n}";
        let program = Compiler::new().compile(source).unwrap();
        let mut instance = program.instantiate(48_000.0, None).unwrap();
        let input = Inputs {
            sr: 48_000.0,
            ..Inputs::default()
        };
        let mut values = Vec::new();
        for _ in 0..4 {
            values.push(instance.evaluate_note(&input, 0, 0).wave);
            instance.commit_voice(0);
        }
        assert_eq!(values, vec![0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn rejects_legacy_source() {
        assert!(Compiler::new().compile("wave = 0\nl_limit = 1").is_err());
    }

    #[test]
    fn standard_primitives_and_bundles_execute_in_jit() {
        let source = r#"
fn note(in, p) -> out {
    coeff = biquad.lowpass(1200, 0.707, in.sr)
    pan = pan.equal_power(1, 0)
    width = stereo.width(pan.left, pan.right, 1.5)
    primitive = exp2(3) + wrap(5, 0, 4) + hypot(3, 4)
        + sinc(0) + fold(3, -1, 1) + window.hann(0.5)
        + onepole_coeff(1000, in.sr)
    shaped = waveshaper(bitcrush(0.25, 8), 2, 0.5)
    out.wave_l = width.left + primitive + shaped
        + 0 * (coeff.b0 + coeff.b1 + coeff.b2 + coeff.a1 + coeff.a2)
    out.wave_r = width.right
    out.l_limit = 1
}
"#;
        let program = Compiler::new().compile(source).unwrap();
        let output = program.evaluate(&Inputs {
            sr: 48_000.0,
            ..Inputs::default()
        });
        assert!(output.wave_l.is_finite());
        assert!(output.wave_l > 10.0);
        assert!(output.wave_r.is_finite());
    }

    #[test]
    fn ring_methods_peek_without_consuming_and_report_size() {
        let source = r#"
fn note(in, p) -> out {
    RingBuf<f32, 4> voice delay
    old = delay.peek_linear(2 / in.sr)
    size = delay.len()
    seconds = delay.duration()
    delay = in.t * in.sr
    out.wave = old + 0 * (size + seconds)
    out.l_limit = 1
}
"#;
        let program = Compiler::new().compile(source).unwrap();
        let mut instance = program.instantiate(1_000.0, None).unwrap();
        let mut input = Inputs {
            sr: 1_000.0,
            ..Inputs::default()
        };
        let mut values = Vec::new();
        for sample in 0..4 {
            input.t = sample as f32 / input.sr;
            values.push(instance.evaluate_note(&input, 0, 0).wave);
            instance.commit_voice(0);
        }
        assert_eq!(values, vec![0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn stateful_standard_dsp_library_compiles_and_runs() {
        let source = r#"
fn note(in, p) -> out {
    out.wave_l = 0
    out.wave_r = 0
    out.l_limit = 1
}
fn filter(in, p) -> out {
    a1 = filter.onepole.lp(in.wave_l, 1000, in.sr)
    a2 = filter.onepole.hp(a1, 120, in.sr)
    a3 = filter.svf.lp(a2, 1500, 0.8, in.sr)
    a4 = filter.svf.hp(a3, 80, 0.8, in.sr)
    a5 = filter.svf.bp(a4, 600, 1.2, in.sr)
    a6 = filter.svf.notch(a5, 900, 1, in.sr)
    b1 = filter.biquad.lp(a6, 4000, 0.707, in.sr)
    b2 = filter.biquad.hp(b1, 30, 0.707, in.sr)
    b3 = filter.biquad.bp(b2, 800, 1, in.sr)
    b4 = filter.biquad.notch(b3, 1000, 1, in.sr)
    b5 = filter.biquad.allpass(b4, 1200, 0.7, in.sr)
    b6 = filter.biquad.peak(b5, 900, 1, 3, in.sr)
    b7 = filter.biquad.lowshelf(b6, 200, 2, in.sr)
    b8 = filter.biquad.highshelf(b7, 5000, -2, in.sr)
    c1 = dc_block(b8)
    c2 = delay.fixed(c1, 10ms)
    c3 = delay.variable(c2, 12ms)
    c4 = delay.feedback(c3, 15ms, 0.2)
    taps = delay.multitap(c4, 5ms, 9ms, 13ms)
    c5 = comb.feedforward(taps.tap1, 7ms, 0.2)
    c6 = comb.feedback(c5, 11ms, 0.2)
    c7 = allpass(c6, 3ms, 0.4)
    d1 = resonator(c7, 440, 0.4)
    d2 = resonator.q(d1, 660, 2)
    d3 = modal(d2, 880, 0.3, 0.2)
    d4 = string.karplus(d3, 220, 1, 0.5)
    d5 = waveguide(d4, 5ms, 0.3, 0.5)
    e1 = chorus(d5, 0.3, 3ms, 15ms)
    e2 = flanger(e1, 0.2, 2ms, 0.2)
    e3 = phaser(e2, 0.2, 0.5, 0.1)
    e4 = tremolo(e3, 4, 0.3)
    e5 = vibrato(e4, 5, 2ms)
    f1 = drive(e5, 1.5)
    f2 = saturate(f1, 1)
    f3 = wavefold(f2, 1.2)
    f4 = downsample(f3, 2)
    g1 = compressor(f4, 0.5, 4, 10ms, 100ms)
    g2 = limiter(g1, 0.9, 1ms, 50ms)
    g3 = gate(g2, 0.001, 1ms, 50ms)
    g4 = envelope_follower(g3, 10ms, 100ms)
    h1 = slew(g4, 10ms, 20ms)
    h2 = smooth(h1, 10ms)
    h3 = sample_hold(h2, 1000)
    h4 = track_hold(h3, 1)
    r1 = reverb.early(h4, 1)
    r2 = reverb.schroeder(r1, 1, 1.5, 0.4)
    r3 = reverb.fdn(r2, 1, 2, 0.5)
    out.wave_l = r3
    out.wave_r = r3
}
"#;
        let program = Compiler::new().compile(source).unwrap();
        let mut instance = program.instantiate(48_000.0, None).unwrap();
        let output = instance.evaluate_filter(&Inputs {
            wave_l: 0.25,
            wave_r: -0.25,
            sr: 48_000.0,
            ..Inputs::default()
        });
        assert!(output.wave_l.is_finite());
        assert!(output.wave_r.is_finite());
    }
}
