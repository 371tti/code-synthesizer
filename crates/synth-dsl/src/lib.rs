//! Parser and Cranelift JIT compiler for the synth expression language.
//!
//! DSLのスタックIRを音声スレッド外でネイティブコードへコンパイルします。
//! 音声スレッドは生成済み関数を直接呼ぶため、評価中にメモリ確保やlockは行いません。

#[allow(unsafe_code)]
mod jit;

use std::{fmt, sync::Arc};

pub const MAX_USER_PARAMETERS: usize = 32;

// ここは上限ではなく、UI に性能上の注意を表示する目安です。
const VARIABLE_WARNING_THRESHOLD: usize = 64;
const OPERATION_WARNING_THRESHOLD: usize = 512;
const STACK_WARNING_THRESHOLD: usize = 128;

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
    pub cc: [f32; 128],
    /// Normalized `p_*` values in declaration order.
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
        if self.step > 0.0 {
            (self.min + ((value - self.min) / self.step).round() * self.step)
                .clamp(self.min, self.max)
        } else {
            value
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Outputs {
    pub wave: f32,
    pub pan: f32,
    pub l_limit: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompileError {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub hint: Option<String>,
}

impl CompileError {
    fn new(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line,
            column,
            hint: None,
        }
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
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

#[derive(Clone, Copy, Debug)]
enum InputId {
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
}

impl InputId {
    #[cfg(test)]
    fn read(self, input: &Inputs) -> f32 {
        match self {
            Self::T => input.t,
            Self::L => input.l,
            Self::S => input.s,
            Self::Freq => input.freq,
            Self::Note => input.note,
            Self::Ch => input.ch,
            Self::Bend => input.bend,
            Self::BendSt => input.bend_st,
            Self::Mw => input.mw,
            Self::Vol => input.vol,
            Self::MidiPan => input.midi_pan,
            Self::Mexpr => input.mexpr,
            Self::Sustain => input.sustain,
            Self::Pressure => input.pressure,
            Self::PolyPressure => input.poly_pressure,
            Self::Program => input.program,
            Self::Sr => input.sr,
            Self::Tempo => input.tempo,
            Self::Beat => input.beat,
            Self::Bar => input.bar,
            Self::Ppq => input.ppq,
            Self::Playing => input.playing,
            Self::Voice => input.voice,
            Self::Rand => input.rand,
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "t" => Self::T,
            "l" => Self::L,
            "s" => Self::S,
            "freq" => Self::Freq,
            "note" => Self::Note,
            "ch" => Self::Ch,
            "bend" => Self::Bend,
            "bend_st" => Self::BendSt,
            "mw" => Self::Mw,
            "vol" => Self::Vol,
            "midi_pan" => Self::MidiPan,
            "mexpr" => Self::Mexpr,
            "sustain" => Self::Sustain,
            "pressure" => Self::Pressure,
            "poly_pressure" => Self::PolyPressure,
            "program" => Self::Program,
            "sr" => Self::Sr,
            "tempo" => Self::Tempo,
            "beat" => Self::Beat,
            "bar" => Self::Bar,
            "ppq" => Self::Ppq,
            "playing" => Self::Playing,
            "voice" => Self::Voice,
            "rand" => Self::Rand,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum ValueRef {
    Input(InputId),
    Parameter {
        index: usize,
        min: f32,
        span: f32,
        step: f32,
    },
    Variable(usize),
    Constant(f32),
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Push(ValueRef),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Neg,
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
    Noise,
    Saw,
    Square,
    Triangle,
}

#[derive(Clone, Debug)]
struct Assignment {
    slot: usize,
    code: Vec<Op>,
}

#[derive(Clone, Debug)]
pub struct Program {
    #[cfg(test)]
    assignments: Vec<Assignment>,
    #[cfg(test)]
    wave: usize,
    #[cfg(test)]
    pan: Option<usize>,
    #[cfg(test)]
    l_limit: usize,
    variable_count: usize,
    #[cfg(test)]
    max_stack: usize,
    operation_count: usize,
    performance_warnings: Vec<String>,
    parameters: Vec<ParameterSpec>,
    jit: Arc<jit::JitProgram>,
}

/// 繰り返し評価APIとの互換性を保つための評価コンテキストです。
///
/// Cranelift JITは式の一時値をネイティブコード内で管理するため、現在は
/// データを保持しません。音声処理中のallocationやlockは発生しません。
#[derive(Clone, Debug, Default)]
pub struct EvaluationScratch;

impl Program {
    /// 手軽に一回だけ評価するためのメソッドです。
    ///
    /// Craneliftで生成済みのネイティブコードを直接呼び出します。
    pub fn evaluate(&self, input: &Inputs) -> Outputs {
        self.jit.evaluate(input)
    }

    pub fn evaluation_scratch(&self) -> EvaluationScratch {
        EvaluationScratch
    }

    /// 生成済みネイティブコードでプログラムを評価します。
    pub fn evaluate_with(&self, input: &Inputs, _scratch: &mut EvaluationScratch) -> Outputs {
        self.jit.evaluate(input)
    }

    #[cfg(test)]
    fn evaluate_interpreted(&self, input: &Inputs) -> Outputs {
        let mut values = vec![0.0; self.variable_count];
        let mut stack = vec![0.0; self.max_stack.max(1)];
        for assignment in &self.assignments {
            values[assignment.slot] = evaluate_code(&assignment.code, input, &values, &mut stack);
        }
        Outputs {
            wave: finite_or(values[self.wave], 0.0),
            pan: finite_or(self.pan.map_or(0.0, |slot| values[slot]), 0.0).clamp(-1.0, 1.0),
            l_limit: finite_or(values[self.l_limit], 0.0),
        }
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
}

#[cfg(test)]
fn evaluate_code(code: &[Op], input: &Inputs, values: &[f32], stack: &mut [f32]) -> f32 {
    let mut len = 0usize;
    for op in code {
        match *op {
            Op::Push(value) => {
                stack[len] = match value {
                    ValueRef::Input(id) => id.read(input),
                    ValueRef::Parameter {
                        index,
                        min,
                        span,
                        step,
                    } => {
                        let value = min + input.params[index].clamp(0.0, 1.0) * span;
                        if step > 0.0 {
                            (min + ((value - min) / step).round() * step).clamp(min, min + span)
                        } else {
                            value
                        }
                    }
                    ValueRef::Variable(slot) => values[slot],
                    ValueRef::Constant(value) => value,
                };
                len += 1;
            }
            Op::Neg => stack[len - 1] = -stack[len - 1],
            Op::Sin => stack[len - 1] = stack[len - 1].sin(),
            Op::Cos => stack[len - 1] = stack[len - 1].cos(),
            Op::Tan => stack[len - 1] = stack[len - 1].tan(),
            Op::Exp => stack[len - 1] = stack[len - 1].exp(),
            Op::Sqrt => stack[len - 1] = stack[len - 1].sqrt(),
            Op::Abs => stack[len - 1] = stack[len - 1].abs(),
            Op::Tanh => stack[len - 1] = stack[len - 1].tanh(),
            Op::Sinh => stack[len - 1] = stack[len - 1].sinh(),
            Op::Cosh => stack[len - 1] = stack[len - 1].cosh(),
            Op::Cbrt => stack[len - 1] = stack[len - 1].cbrt(),
            Op::Ln => stack[len - 1] = stack[len - 1].ln(),
            Op::Log2 => stack[len - 1] = stack[len - 1].log2(),
            Op::Log10 => stack[len - 1] = stack[len - 1].log10(),
            Op::Floor => stack[len - 1] = stack[len - 1].floor(),
            Op::Ceil => stack[len - 1] = stack[len - 1].ceil(),
            Op::Round => stack[len - 1] = stack[len - 1].round(),
            Op::Fract => stack[len - 1] = stack[len - 1].fract(),
            Op::Sign => stack[len - 1] = stack[len - 1].signum(),
            Op::Asin => stack[len - 1] = stack[len - 1].asin(),
            Op::Acos => stack[len - 1] = stack[len - 1].acos(),
            Op::Atan => stack[len - 1] = stack[len - 1].atan(),
            Op::Cc => {
                let index = stack[len - 1].round().clamp(0.0, 127.0) as usize;
                stack[len - 1] = input.cc[index];
            }
            Op::Noise => {
                stack[len] = input.rand;
                len += 1;
            }
            Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::Pow
            | Op::Min
            | Op::Max
            | Op::Atan2 => {
                let rhs = stack[len - 1];
                let lhs = stack[len - 2];
                stack[len - 2] = match op {
                    Op::Add => lhs + rhs,
                    Op::Sub => lhs - rhs,
                    Op::Mul => lhs * rhs,
                    Op::Div => lhs / rhs,
                    Op::Mod => lhs % rhs,
                    Op::Pow => lhs.powf(rhs),
                    Op::Min => lhs.min(rhs),
                    Op::Max => lhs.max(rhs),
                    Op::Atan2 => lhs.atan2(rhs),
                    _ => unreachable!(),
                };
                len -= 1;
            }
            Op::Clamp | Op::Mix => {
                let third = stack[len - 1];
                let second = stack[len - 2];
                let first = stack[len - 3];
                stack[len - 3] = match op {
                    Op::Clamp => first.clamp(second.min(third), second.max(third)),
                    Op::Mix => first + (second - first) * third,
                    _ => unreachable!(),
                };
                len -= 2;
            }
            Op::Saw | Op::Triangle => {
                let time = stack[len - 1];
                let frequency = stack[len - 2];
                stack[len - 2] = match op {
                    Op::Saw => polyblep_saw(frequency, time, input.sr),
                    Op::Triangle => 1.0 - 4.0 * (positive_mod(frequency * time, 1.0) - 0.5).abs(),
                    _ => unreachable!(),
                };
                len -= 1;
            }
            Op::Square => {
                let duty = stack[len - 1].clamp(0.01, 0.99);
                let time = stack[len - 2];
                let frequency = stack[len - 3];
                stack[len - 3] = polyblep_square(frequency, time, duty, input.sr);
                len -= 2;
            }
        }
    }
    stack[0]
}

pub struct Compiler;

impl Compiler {
    pub fn new() -> Self {
        Self
    }

    pub fn compile(&self, source: &str) -> Result<Program, CompileError> {
        let mut statements: Vec<(usize, String)> = Vec::new();
        for (line_index, line) in source.lines().enumerate() {
            let trimmed = strip_line_comment(line).trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.contains('=') {
                statements.push((line_index + 1, trimmed.to_owned()));
            } else if let Some((_, expression)) = statements.last_mut() {
                expression.push(' ');
                expression.push_str(trimmed);
            } else {
                return Err(
                    CompileError::new("expected an assignment", line_index + 1, 1)
                        .with_hint("Start the line with `name = expression`."),
                );
            }
        }

        let mut parameters = Vec::new();
        for (line, statement) in &statements {
            let (name, expression) = statement.split_once('=').ok_or_else(|| {
                CompileError::new("expected an assignment", *line, 1)
                    .with_hint("For example: `wave = sin(TAU * freq * t)`.")
            })?;
            let name = name.trim();
            if !name.starts_with("p_") {
                continue;
            }
            if !is_identifier(name) || name.len() <= 2 {
                return Err(CompileError::new("invalid parameter name", *line, 1)
                    .with_hint("User parameters must look like `p_tone` or `p_attack`."));
            }
            if parameters
                .iter()
                .any(|parameter: &ParameterSpec| parameter.name == name)
            {
                return Err(
                    CompileError::new("duplicate parameter declaration", *line, 1)
                        .with_hint(format!("`{name}` is already declared.")),
                );
            }
            if parameters.len() >= MAX_USER_PARAMETERS {
                return Err(
                    CompileError::new("too many user parameters", *line, 1).with_hint(format!(
                        "At most {MAX_USER_PARAMETERS} p_* parameters are supported."
                    )),
                );
            }
            let (default, min, max, step) = parse_parameter_declaration(expression.trim(), *line)?;
            parameters.push(ParameterSpec {
                index: parameters.len(),
                name: name.to_owned(),
                default,
                min,
                max,
                step,
            });
        }

        let mut assignments = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut outputs = [None; 3];
        for (line, statement) in statements {
            let (name, expression) = statement.split_once('=').ok_or_else(|| {
                CompileError::new("expected an assignment such as `wave = sin(...)`", line, 1)
            })?;
            let name = name.trim();
            if name.starts_with("p_") {
                continue;
            }
            if !is_identifier(name) {
                return Err(CompileError::new("invalid assignment name", line, 1)
                    .with_hint("Names may contain ASCII letters, digits, and `_`, and cannot start with a digit."));
            }
            if InputId::parse(name).is_some() || constant(name).is_some() {
                return Err(
                    CompileError::new("cannot assign to a built-in name", line, 1)
                        .with_hint(format!("Choose a local name such as `{name}_value`.")),
                );
            }
            let expression = expression.trim();
            if expression.starts_with("param") {
                return Err(CompileError::new("param() requires a p_* name", line, 1)
                    .with_hint("Write `p_name = param(default, min, max, step)`."));
            }
            if expression.is_empty() {
                return Err(
                    CompileError::new("expression is empty", line, statement.len())
                        .with_hint("Add a number, variable, or function after `=`."),
                );
            }
            if names.iter().any(|existing| existing == name) {
                return Err(CompileError::new("duplicate assignment", line, 1)
                    .with_hint(format!("Rename or remove the second `{name}` assignment.")));
            }
            let slot = names.len();
            let mut parser = Parser::new(expression, line, &names, &parameters)?;
            let code = parser.parse()?;
            names.push(name.to_owned());
            assignments.push(Assignment { slot, code });
            match name {
                "wave" => outputs[0] = Some(slot),
                "pan" => outputs[1] = Some(slot),
                "l_limit" => outputs[2] = Some(slot),
                _ => {}
            }
        }
        if outputs[0].is_none() {
            return Err(CompileError::new("program must define `wave`", 1, 1)
                .with_hint("Add `wave = sin(TAU * freq * t)` as an output."));
        }
        if outputs[2].is_none() {
            return Err(
                CompileError::new("program must define `l_limit`", 1, 1).with_hint(
                    "Add `l_limit = 1.0` (seconds after note release) so voices can be retired.",
                ),
            );
        }
        let operation_count = assignments
            .iter()
            .map(|assignment| assignment.code.len())
            .sum();
        let max_stack = assignments
            .iter()
            .map(|assignment| assignment.code.len())
            .max()
            .unwrap_or(1);
        let mut performance_warnings = Vec::new();
        if names.len() > VARIABLE_WARNING_THRESHOLD {
            performance_warnings.push(format!(
                "ローカル変数が {} 個あります。各ボイスで全変数を毎サンプル評価するため、多音時は CPU 負荷が増える可能性があります。",
                names.len()
            ));
        }
        if operation_count > OPERATION_WARNING_THRESHOLD {
            performance_warnings.push(format!(
                "1 サンプルあたりの演算が {operation_count} 個あります。複雑な式は CPU 負荷と発音数に影響する可能性があります。"
            ));
        }
        if max_stack > STACK_WARNING_THRESHOLD {
            performance_warnings.push(format!(
                "1 つの式が {max_stack} 個の演算を含みます。式を分割すると、編集と性能調整がしやすくなる場合があります。"
            ));
        }
        let wave = outputs[0].expect("wave output validated above");
        let pan = outputs[1];
        let l_limit = outputs[2].expect("l_limit output validated above");
        let jit = jit::JitProgram::compile(&assignments, names.len(), wave, pan, l_limit).map_err(
            |message| {
                CompileError::new(format!("Cranelift JIT compilation failed: {message}"), 1, 1)
                    .with_hint("This host must support Cranelift native JIT execution.")
            },
        )?;
        Ok(Program {
            #[cfg(test)]
            assignments,
            #[cfg(test)]
            wave,
            #[cfg(test)]
            pan,
            #[cfg(test)]
            l_limit,
            variable_count: names.len(),
            #[cfg(test)]
            max_stack,
            operation_count,
            performance_warnings,
            parameters,
            jit: Arc::new(jit),
        })
    }
}

#[cfg(test)]
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn strip_line_comment(line: &str) -> &str {
    let hash = line.find('#');
    let slash = line.find("//");
    match (hash, slash) {
        (Some(a), Some(b)) => &line[..a.min(b)],
        (Some(index), None) | (None, Some(index)) => &line[..index],
        (None, None) => line,
    }
}

fn parse_parameter_declaration(
    expression: &str,
    line: usize,
) -> Result<(f32, f32, f32, f32), CompileError> {
    let Some(arguments) = expression
        .strip_prefix("param(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(
            CompileError::new("p_* variables must be declared with param()", line, 1)
                .with_hint("Use `p_tone = param(0.5, 0, 1, 0.01)`."),
        );
    };
    let parts: Vec<_> = arguments.split(',').map(str::trim).collect();
    if !(3..=4).contains(&parts.len()) {
        return Err(CompileError::new(
            "param() expects default, min, max, and optional step",
            line,
            1,
        )
        .with_hint("Use `param(default, min, max)` or `param(default, min, max, step)`."));
    }
    let mut numbers = [0.0f32; 4];
    for (index, part) in parts.iter().enumerate() {
        numbers[index] = part.parse::<f32>().map_err(|_| {
            CompileError::new("param() arguments must be plain numbers", line, 1)
                .with_hint("For example: `p_gain = param(0.8, 0, 1, 0.01)`.")
        })?;
        if !numbers[index].is_finite() {
            return Err(CompileError::new(
                "param() arguments must be finite numbers",
                line,
                1,
            ));
        }
    }
    let (default, min, max) = (numbers[0], numbers[1], numbers[2]);
    let step = if parts.len() == 4 { numbers[3] } else { 0.0 };
    if min >= max {
        return Err(
            CompileError::new("param() minimum must be smaller than maximum", line, 1)
                .with_hint(format!("The current range is {min} to {max}.")),
        );
    }
    if !(min..=max).contains(&default) {
        return Err(
            CompileError::new("param() default must be inside its range", line, 1)
                .with_hint(format!("Choose a default between {min} and {max}.")),
        );
    }
    if step < 0.0 || step > max - min {
        return Err(CompileError::new(
            "param() step must be zero or fit inside its range",
            line,
            1,
        )
        .with_hint("Use 0 for continuous control, or a positive increment."));
    }
    Ok((default, min, max, step))
}

fn positive_mod(value: f32, modulus: f32) -> f32 {
    ((value % modulus) + modulus) % modulus
}

fn poly_blep(phase: f32, phase_step: f32) -> f32 {
    let dt = phase_step.clamp(0.0, 0.5);
    if dt <= f32::EPSILON {
        0.0
    } else if phase < dt {
        let x = phase / dt;
        x + x - x * x - 1.0
    } else if phase > 1.0 - dt {
        let x = (phase - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

fn oscillator_phase(frequency: f32, time: f32, sample_rate: f32) -> (f32, f32) {
    let frequency = frequency.abs();
    let sample_rate = if sample_rate.is_finite() && sample_rate > 1.0 {
        sample_rate
    } else {
        48_000.0
    };
    (
        positive_mod(frequency * time, 1.0),
        (frequency / sample_rate).clamp(0.0, 0.5),
    )
}

fn polyblep_saw(frequency: f32, time: f32, sample_rate: f32) -> f32 {
    let (phase, step) = oscillator_phase(frequency, time, sample_rate);
    2.0 * phase - 1.0 - poly_blep(phase, step)
}

fn polyblep_square(frequency: f32, time: f32, duty: f32, sample_rate: f32) -> f32 {
    let (phase, step) = oscillator_phase(frequency, time, sample_rate);
    let mut value = if phase < duty { 1.0 } else { -1.0 };
    value += poly_blep(phase, step);
    value -= poly_blep(positive_mod(phase - duty, 1.0), step);
    value
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TokenKind {
    Number(f32),
    Identifier,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
    End,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    kind: TokenKind,
    text: &'a str,
    column: usize,
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }

    fn next(&mut self, line: usize) -> Result<Token<'a>, CompileError> {
        while let Some(c) = self.source[self.offset..].chars().next() {
            if c.is_whitespace() {
                self.offset += c.len_utf8();
            } else {
                break;
            }
        }
        let column = self.offset + 1;
        let rest = &self.source[self.offset..];
        let Some(first) = rest.chars().next() else {
            return Ok(Token {
                kind: TokenKind::End,
                text: "",
                column,
            });
        };
        let single = match first {
            '+' => Some(TokenKind::Plus),
            '-' => Some(TokenKind::Minus),
            '*' => Some(TokenKind::Star),
            '/' => Some(TokenKind::Slash),
            '%' => Some(TokenKind::Percent),
            '^' => Some(TokenKind::Caret),
            '(' => Some(TokenKind::LParen),
            ')' => Some(TokenKind::RParen),
            ',' => Some(TokenKind::Comma),
            _ => None,
        };
        if let Some(kind) = single {
            self.offset += first.len_utf8();
            let text = &self.source[self.offset - first.len_utf8()..self.offset];
            return Ok(Token { kind, text, column });
        }
        if first.is_ascii_digit() || first == '.' {
            let start = self.offset;
            let mut digits = 0;
            while let Some(c) = self.source[self.offset..].chars().next() {
                if c.is_ascii_digit() {
                    digits += 1;
                    self.offset += 1;
                } else if c == '.' {
                    self.offset += 1;
                } else {
                    break;
                }
            }
            if matches!(self.source[self.offset..].chars().next(), Some('e' | 'E')) {
                self.offset += 1;
                if matches!(self.source[self.offset..].chars().next(), Some('+' | '-')) {
                    self.offset += 1;
                }
                while let Some(c) = self.source[self.offset..].chars().next() {
                    if c.is_ascii_digit() {
                        digits += 1;
                        self.offset += 1;
                    } else {
                        break;
                    }
                }
            }
            let text = &self.source[start..self.offset];
            let value = text
                .parse()
                .map_err(|_| CompileError::new("invalid number", line, column))?;
            if digits == 0 {
                return Err(CompileError::new("invalid number", line, column));
            }
            return Ok(Token {
                kind: TokenKind::Number(value),
                text,
                column,
            });
        }
        if first == '_' || first.is_ascii_alphabetic() {
            let start = self.offset;
            self.offset += first.len_utf8();
            while let Some(c) = self.source[self.offset..].chars().next() {
                if c == '_' || c.is_ascii_alphanumeric() {
                    self.offset += c.len_utf8();
                } else {
                    break;
                }
            }
            let text = &self.source[start..self.offset];
            return Ok(Token {
                kind: TokenKind::Identifier,
                text,
                column,
            });
        }
        Err(CompileError::new("unexpected character", line, column))
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token<'a>,
    line: usize,
    names: &'a [String],
    parameters: &'a [ParameterSpec],
}

impl<'a> Parser<'a> {
    fn new(
        source: &'a str,
        line: usize,
        names: &'a [String],
        parameters: &'a [ParameterSpec],
    ) -> Result<Self, CompileError> {
        let mut lexer = Lexer::new(source);
        let current = lexer.next(line)?;
        Ok(Self {
            lexer,
            current,
            line,
            names,
            parameters,
        })
    }

    fn advance(&mut self) -> Result<(), CompileError> {
        self.current = self.lexer.next(self.line)?;
        Ok(())
    }

    fn parse(&mut self) -> Result<Vec<Op>, CompileError> {
        let mut code = Vec::new();
        self.expression(&mut code, 0)?;
        if self.current.kind != TokenKind::End {
            return Err(CompileError::new(
                "unexpected token after expression",
                self.line,
                self.current.column,
            ));
        }
        Ok(code)
    }

    fn expression(&mut self, code: &mut Vec<Op>, min_precedence: u8) -> Result<(), CompileError> {
        self.prefix(code)?;
        loop {
            let (precedence, op) = match self.current.kind {
                TokenKind::Plus => (1, Op::Add),
                TokenKind::Minus => (1, Op::Sub),
                TokenKind::Star => (2, Op::Mul),
                TokenKind::Slash => (2, Op::Div),
                TokenKind::Percent => (2, Op::Mod),
                TokenKind::Caret => (3, Op::Pow),
                _ => break,
            };
            if precedence < min_precedence {
                break;
            }
            self.advance()?;
            self.expression(
                code,
                if precedence == 3 {
                    precedence
                } else {
                    precedence + 1
                },
            )?;
            code.push(op);
        }
        Ok(())
    }

    fn prefix(&mut self, code: &mut Vec<Op>) -> Result<(), CompileError> {
        match self.current.kind {
            TokenKind::Number(value) => {
                code.push(Op::Push(ValueRef::Constant(value)));
                self.advance()?;
            }
            TokenKind::Minus => {
                self.advance()?;
                self.prefix(code)?;
                code.push(Op::Neg);
            }
            TokenKind::Plus => {
                self.advance()?;
                self.prefix(code)?;
            }
            TokenKind::Identifier => {
                let token = self.current;
                let name = token.text;
                self.advance()?;
                if self.current.kind == TokenKind::LParen {
                    self.function(name, token.column, code)?;
                } else if let Some(input) = InputId::parse(name) {
                    code.push(Op::Push(ValueRef::Input(input)));
                } else if let Some(value) = constant(name) {
                    code.push(Op::Push(ValueRef::Constant(value)));
                } else if let Some(parameter) = self
                    .parameters
                    .iter()
                    .find(|parameter| parameter.name == name)
                {
                    code.push(Op::Push(ValueRef::Parameter {
                        index: parameter.index,
                        min: parameter.min,
                        span: parameter.max - parameter.min,
                        step: parameter.step,
                    }));
                } else if let Some(slot) = self.names.iter().position(|known| known == name) {
                    code.push(Op::Push(ValueRef::Variable(slot)));
                } else {
                    return Err(CompileError::new(
                        format!("unknown identifier `{name}`"),
                        self.line,
                        token.column,
                    )
                    .with_hint(identifier_hint(
                        name,
                        self.names,
                        self.parameters,
                    )));
                }
            }
            TokenKind::LParen => {
                self.advance()?;
                self.expression(code, 0)?;
                self.expect(TokenKind::RParen, "expected `)`")?;
            }
            _ => {
                return Err(CompileError::new(
                    "expected a number, identifier, or `(`",
                    self.line,
                    self.current.column,
                ));
            }
        }
        Ok(())
    }

    fn function(
        &mut self,
        name: &str,
        column: usize,
        code: &mut Vec<Op>,
    ) -> Result<(), CompileError> {
        self.expect(TokenKind::LParen, "expected `(`")?;
        if name == "noise" {
            self.expect(TokenKind::RParen, "noise() does not take arguments")?;
            code.push(Op::Noise);
            return Ok(());
        }
        let unary = match name {
            "sin" => Some(Op::Sin),
            "cos" => Some(Op::Cos),
            "tan" => Some(Op::Tan),
            "exp" => Some(Op::Exp),
            "sqrt" => Some(Op::Sqrt),
            "abs" => Some(Op::Abs),
            "tanh" => Some(Op::Tanh),
            "sinh" => Some(Op::Sinh),
            "cosh" => Some(Op::Cosh),
            "cbrt" => Some(Op::Cbrt),
            "ln" | "log" => Some(Op::Ln),
            "log2" => Some(Op::Log2),
            "log10" => Some(Op::Log10),
            "floor" => Some(Op::Floor),
            "ceil" => Some(Op::Ceil),
            "round" => Some(Op::Round),
            "fract" => Some(Op::Fract),
            "sign" => Some(Op::Sign),
            "asin" => Some(Op::Asin),
            "acos" => Some(Op::Acos),
            "atan" => Some(Op::Atan),
            "cc" => Some(Op::Cc),
            _ => None,
        };
        if let Some(op) = unary {
            self.expression(code, 0)?;
            self.expect(TokenKind::RParen, "expected `)`")?;
            code.push(op);
            return Ok(());
        }
        let binary = match name {
            "min" => Some(Op::Min),
            "max" => Some(Op::Max),
            "pow" => Some(Op::Pow),
            "atan2" => Some(Op::Atan2),
            "mod" => Some(Op::Mod),
            "saw" => Some(Op::Saw),
            "triangle" => Some(Op::Triangle),
            _ => None,
        };
        if let Some(op) = binary {
            self.expression(code, 0)?;
            self.expect(TokenKind::Comma, "expected `,`")?;
            self.expression(code, 0)?;
            self.expect(TokenKind::RParen, "expected `)`")?;
            code.push(op);
            return Ok(());
        }
        let ternary = match name {
            "clamp" => Some(Op::Clamp),
            "mix" => Some(Op::Mix),
            "square" | "pulse" => Some(Op::Square),
            _ => None,
        };
        if let Some(op) = ternary {
            self.expression(code, 0)?;
            self.expect(TokenKind::Comma, "expected `,`")?;
            self.expression(code, 0)?;
            self.expect(TokenKind::Comma, "expected `,`")?;
            self.expression(code, 0)?;
            self.expect(TokenKind::RParen, "expected `)`")?;
            code.push(op);
            return Ok(());
        }
        Err(
            CompileError::new(format!("unknown function `{name}`"), self.line, column)
                .with_hint(function_hint(name)),
        )
    }

    fn expect(&mut self, expected: TokenKind, message: &str) -> Result<(), CompileError> {
        if self.current.kind != expected {
            return Err(CompileError::new(message, self.line, self.current.column));
        }
        self.advance()
    }
}

const FUNCTION_NAMES: &[&str] = &[
    "sin", "cos", "tan", "exp", "sqrt", "abs", "tanh", "sinh", "cosh", "cbrt", "ln", "log", "log2",
    "log10", "floor", "ceil", "round", "fract", "sign", "asin", "acos", "atan", "cc", "noise",
    "min", "max", "pow", "atan2", "mod", "saw", "triangle", "clamp", "mix", "square", "pulse",
];

fn identifier_hint(name: &str, names: &[String], parameters: &[ParameterSpec]) -> String {
    let builtins = [
        "t",
        "l",
        "s",
        "freq",
        "note",
        "ch",
        "bend",
        "bend_st",
        "mw",
        "vol",
        "midi_pan",
        "mexpr",
        "sustain",
        "pressure",
        "poly_pressure",
        "program",
        "sr",
        "tempo",
        "beat",
        "bar",
        "ppq",
        "playing",
        "voice",
        "rand",
        "TAU",
        "PI",
        "E",
        "PHI",
    ];
    let candidate = builtins
        .iter()
        .copied()
        .chain(names.iter().map(String::as_str))
        .chain(parameters.iter().map(|parameter| parameter.name.as_str()))
        .min_by_key(|candidate| edit_distance(name, candidate));
    match candidate {
        Some(candidate) if edit_distance(name, candidate) <= 3 => {
            format!("Did you mean `{candidate}`?")
        }
        _ if name.starts_with("p_") => {
            format!("Declare it first with `{name} = param(default, min, max)`.")
        }
        _ => "Define the name on an earlier line, or choose a built-in input.".to_owned(),
    }
}

fn function_hint(name: &str) -> String {
    let candidate = FUNCTION_NAMES
        .iter()
        .copied()
        .min_by_key(|candidate| edit_distance(name, candidate));
    match candidate {
        Some(candidate) if edit_distance(name, candidate) <= 3 => {
            format!("Did you mean `{candidate}(...)`?")
        }
        _ => "Supported functions include sin, saw, square, noise, mix, and clamp.".to_owned(),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (a_index, a_byte) in a.bytes().enumerate() {
        current[0] = a_index + 1;
        for (b_index, b_byte) in b.bytes().enumerate() {
            current[b_index + 1] = (previous[b_index + 1] + 1)
                .min(current[b_index] + 1)
                .min(previous[b_index] + usize::from(a_byte != b_byte));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

fn constant(name: &str) -> Option<f32> {
    match name {
        "TAU" => Some(std::f32::consts::TAU),
        "PI" => Some(std::f32::consts::PI),
        "E" => Some(std::f32::consts::E),
        "PHI" => Some(1.618_034),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{hint::black_box, time::Instant};

    fn assert_output_close(jit: Outputs, interpreted: Outputs) {
        for (name, actual, expected) in [
            ("wave", jit.wave, interpreted.wave),
            ("pan", jit.pan, interpreted.pan),
            ("l_limit", jit.l_limit, interpreted.l_limit),
        ] {
            let tolerance = 0.000_01 * (1.0 + expected.abs());
            assert!(
                (actual - expected).abs() <= tolerance,
                "{name}: JIT={actual}, interpreter={expected}"
            );
        }
    }

    fn evaluate_legacy_with(
        program: &Program,
        input: &Inputs,
        values: &mut [f32],
        stack: &mut [f32],
    ) -> Outputs {
        for assignment in &program.assignments {
            values[assignment.slot] = evaluate_code(&assignment.code, input, values, stack);
        }
        Outputs {
            wave: finite_or(values[program.wave], 0.0),
            pan: finite_or(program.pan.map_or(0.0, |slot| values[slot]), 0.0).clamp(-1.0, 1.0),
            l_limit: finite_or(values[program.l_limit], 0.0),
        }
    }

    #[test]
    fn compiles_multiline_program_and_evaluates_assignments() {
        let source =
            "env = exp(-3*t)\n  * exp(-5*l)\nwave = env * sin(TAU*freq*t)\npan = -2\nl_limit = 1.5";
        let program = Compiler::new().compile(source).unwrap();
        let output = program.evaluate(&Inputs {
            t: 0.25,
            freq: 1.0,
            ..Inputs::default()
        });
        assert!((output.wave - 0.47236657).abs() < 0.0001);
        assert_eq!(output.pan, -1.0);
        assert_eq!(output.l_limit, 1.5);
    }

    #[test]
    fn rejects_unknown_names_and_missing_required_outputs() {
        assert!(
            Compiler::new()
                .compile("wave = nope(t)\nl_limit = 1")
                .is_err()
        );
        assert!(Compiler::new().compile("pan = 0").is_err());
        let error = Compiler::new().compile("wave = 0").unwrap_err();
        assert_eq!(error.message, "program must define `l_limit`");
    }

    #[test]
    fn supports_function_arguments_and_precedence() {
        let program = Compiler::new()
            .compile("wave = min(1 + 2 * 3, pow(2, 3))\nl_limit = 1")
            .unwrap();
        assert_eq!(program.evaluate(&Inputs::default()).wave, 7.0);
    }

    #[test]
    fn supports_cc_modulo_clamp_and_inline_comments() {
        let program = Compiler::new()
            .compile("wave = clamp(cc(74) + 5 % 2, 0, 1) # brightness\nl_limit = 1")
            .unwrap();
        let mut input = Inputs::default();
        input.cc[74] = 0.25;
        assert_eq!(program.evaluate(&input).wave, 1.0);
    }

    #[test]
    fn rejects_assignment_to_inputs() {
        assert!(
            Compiler::new()
                .compile("t = 1\nwave = t\nl_limit = 1")
                .is_err()
        );
    }

    #[test]
    fn user_parameters_keep_ranges_and_drive_evaluation() {
        let program = Compiler::new()
            .compile("p_tone = param(0.5, -2, 2, 0.1)\nwave = p_tone\nl_limit = 1")
            .unwrap();
        let spec = &program.parameter_specs()[0];
        assert_eq!(spec.name, "p_tone");
        assert_eq!(spec.default_normalized(), 0.625);
        let mut input = Inputs::default();
        input.params[0] = 1.0;
        assert_eq!(program.evaluate(&input).wave, 2.0);
        input.params[0] = 0.64;
        assert!((program.evaluate(&input).wave - 0.6).abs() < 0.0001);
    }

    #[test]
    fn accepts_many_local_variables_and_reports_a_performance_warning() {
        let mut source = String::new();
        for index in 0..80 {
            if index == 0 {
                source.push_str("v0 = 0\n");
            } else {
                source.push_str(&format!("v{index} = v{} + 1\n", index - 1));
            }
        }
        source.push_str("wave = v79\nl_limit = 1");

        let program = Compiler::new().compile(&source).unwrap();
        assert_eq!(program.variable_count(), 82);
        assert!(
            program
                .performance_warnings()
                .iter()
                .any(|warning| warning.contains("ローカル変数"))
        );
        let mut scratch = program.evaluation_scratch();
        assert_eq!(
            program.evaluate_with(&Inputs::default(), &mut scratch).wave,
            79.0
        );
    }

    #[test]
    fn accepts_long_expressions_and_reports_a_performance_warning() {
        let mut expression = String::from("1");
        for _ in 0..600 {
            expression.push_str(" + 1");
        }
        let program = Compiler::new()
            .compile(&format!("wave = {expression}\nl_limit = 1"))
            .unwrap();

        assert!(program.operation_count() > OPERATION_WARNING_THRESHOLD);
        assert!(
            program
                .performance_warnings()
                .iter()
                .any(|warning| warning.contains("演算"))
        );
    }

    #[test]
    fn supports_math_synth_oscillators_and_helpful_diagnostics() {
        let program = Compiler::new()
            .compile(
                "wave = saw(440,t) + square(220,t,0.25) + triangle(110,t) + noise()\nl_limit = 1",
            )
            .unwrap();
        assert!(
            program
                .evaluate(&Inputs {
                    sr: 48_000.0,
                    ..Inputs::default()
                })
                .wave
                .is_finite()
        );
        let error = Compiler::new()
            .compile("wave = frqe\nl_limit = 1")
            .unwrap_err();
        assert!(
            error
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("freq"))
        );
        assert!(
            Compiler::new()
                .compile("p_bad = param(2, 0, 1)\nwave = p_bad\nl_limit = 1")
                .is_err()
        );
    }

    #[test]
    fn cranelift_jit_matches_the_interpreter_for_all_operations() {
        let source = r#"
p_shape = param(0.4, -2, 2, 0.1)
inputs = t+l+s+freq+note+ch+bend+bend_st+mw+vol+midi_pan+mexpr+sustain+pressure+poly_pressure+program+sr+tempo+beat+bar+ppq+playing+voice+noise()+cc(74)+p_shape
basic = (8+2-3)*0.25/2 + 7%3 + 2^3
unary_a = sin(0.2)+cos(0.3)+tan(0.1)+exp(0.2)+sqrt(4)+abs(-3)
unary_b = tanh(0.2)+sinh(0.1)+cosh(0.1)+cbrt(8)+ln(2)+log2(8)+log10(100)
unary_c = floor(1.8)+ceil(1.2)+round(-1.5)+fract(-1.25)+sign(-3)+asin(0.2)+acos(0.2)+atan(0.2)
multi = min(3,4)+max(3,4)+pow(2,3)+atan2(1,2)+mod(-7,3)+clamp(3,2,-1)+mix(2,4,0.25)
osc = saw(110,t)+square(55,t,0.3)+triangle(27.5,t)
wave = inputs*0.0001 + basic + unary_a + unary_b + unary_c + multi + osc
pan = clamp(midi_pan*2-1,-1,1)
l_limit = max(0.1, p_shape+2.5)
"#;
        let program = Compiler::new().compile(source).unwrap();
        assert_eq!(program.execution_backend(), "Cranelift JIT");
        let mut input = Inputs {
            t: 0.123,
            l: 0.2,
            s: 1.7,
            freq: 440.0,
            note: 69.0,
            ch: 2.0,
            bend: -0.1,
            bend_st: -0.2,
            mw: 0.3,
            vol: 0.8,
            midi_pan: 0.7,
            mexpr: 0.6,
            sustain: 1.0,
            pressure: 0.4,
            poly_pressure: 0.5,
            program: 3.0,
            sr: 48_000.0,
            tempo: 128.0,
            beat: 2.5,
            bar: 4.0,
            ppq: 14.25,
            playing: 1.0,
            voice: 7.0,
            rand: -0.25,
            ..Inputs::default()
        };
        input.cc[74] = 0.65;
        input.params[0] = 0.7;

        assert_output_close(
            program.evaluate(&input),
            program.evaluate_interpreted(&input),
        );
    }

    #[test]
    fn cranelift_program_is_send_sync_and_sanitizes_outputs() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Program>();

        let program = Compiler::new()
            .compile("wave = 0/0\npan = 10\nl_limit = 1/0")
            .unwrap();
        assert_eq!(
            program.evaluate(&Inputs::default()),
            Outputs {
                wave: 0.0,
                pan: 1.0,
                l_limit: 0.0,
            }
        );
    }

    #[test]
    fn repeatedly_compiles_executes_and_frees_jit_programs() {
        for index in 0..128 {
            let program = Compiler::new()
                .compile(&format!(
                    "value = sin(TAU*freq*t) + {index}\nwave = value\nl_limit = 1"
                ))
                .unwrap();
            assert!(
                program
                    .evaluate(&Inputs {
                        t: 0.01,
                        freq: 220.0,
                        ..Inputs::default()
                    })
                    .wave
                    .is_finite()
            );
        }
    }

    #[test]
    #[ignore = "manual release-mode throughput check"]
    fn benchmark_cranelift_against_legacy_interpreter() {
        let source = r#"
p_tone = param(0.55, 0, 1, 0.01)
p_gain = param(0.8, 0, 1, 0.01)
fundamental = sin(TAU*freq*t)
harmonics = 0.35*sin(TAU*freq*2*t) + 0.15*sin(TAU*freq*3*t)
edge = 0.12*saw(freq*0.5,t)
env = min(t*180,1)*exp(-3*l)
wave = (fundamental + p_tone*harmonics + edge)*env*s*p_gain
pan = 0.25*sin(TAU*0.2*t)
l_limit = 2
"#;
        let program = Compiler::new().compile(source).unwrap();
        let mut input = Inputs {
            freq: 220.0,
            s: 0.8,
            sr: 48_000.0,
            ..Inputs::default()
        };
        input.params[0] = 0.55;
        input.params[1] = 0.8;
        let iterations = 2_000_000;

        let started = Instant::now();
        let mut jit_sum = 0.0f32;
        for index in 0..iterations {
            input.t = black_box(index as f32 / input.sr);
            jit_sum += black_box(program.evaluate(&input).wave);
        }
        let jit_time = started.elapsed();

        let mut values = vec![0.0; program.variable_count];
        let mut stack = vec![0.0; program.max_stack.max(1)];
        let started = Instant::now();
        let mut interpreter_sum = 0.0f32;
        for index in 0..iterations {
            input.t = black_box(index as f32 / input.sr);
            interpreter_sum +=
                black_box(evaluate_legacy_with(&program, &input, &mut values, &mut stack).wave);
        }
        let interpreter_time = started.elapsed();

        assert!((jit_sum - interpreter_sum).abs() < 0.01);
        eprintln!(
            "Cranelift: {jit_time:?}, interpreter: {interpreter_time:?}, speedup: {:.2}x",
            interpreter_time.as_secs_f64() / jit_time.as_secs_f64()
        );
    }
}
