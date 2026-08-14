//! Cranelift backend for the parsed stack IR.
//!
//! JIT compilation happens on the editor/UI thread. The audio thread only calls the
//! finalized function pointer; it does not allocate, compile, or lock.

use super::{Assignment, InputId, Inputs, Op, Outputs, ValueRef};
use cranelift::{
    codegen::ir::{FuncRef, MemFlagsData, UserFuncName},
    prelude::*,
};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};
use std::{fmt, mem, sync::Mutex};

type EntryFn = unsafe extern "C" fn(*const Inputs, *mut Outputs);

/// Owns the executable allocation for one DSL program.
///
/// `JITModule` is `Send` but not `Sync`, so it is kept behind a mutex. Evaluation never
/// touches the mutex: the module is immutable after finalization and exists only to keep
/// executable memory alive until the last cloned `Program` has gone away.
pub(super) struct JitProgram {
    entry: EntryFn,
    module: Mutex<Option<JITModule>>,
}

impl fmt::Debug for JitProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JitProgram")
            .field("entry", &(self.entry as *const ()))
            .finish_non_exhaustive()
    }
}

impl JitProgram {
    pub(super) fn compile(
        assignments: &[Assignment],
        variable_count: usize,
        wave: usize,
        pan: Option<usize>,
        l_limit: usize,
    ) -> Result<Self, String> {
        let mut jit_builder =
            JITBuilder::with_flags(&[("opt_level", "speed")], default_libcall_names())
                .map_err(|error| error.to_string())?;
        for helper in Helper::ALL.iter().copied() {
            jit_builder.symbol(helper.symbol(), helper.address());
        }

        let mut module = JITModule::new(jit_builder);
        let frontend_config = module.target_config();
        let pointer_type = frontend_config.pointer_type();
        let mut helper_ids = Vec::with_capacity(Helper::ALL.len());
        for helper in Helper::ALL.iter().copied() {
            let signature = helper.signature(&module, pointer_type);
            let id = module
                .declare_function(helper.symbol(), Linkage::Import, &signature)
                .map_err(|error| error.to_string())?;
            helper_ids.push(id);
        }

        let mut signature = module.make_signature();
        signature.params.push(AbiParam::new(pointer_type));
        signature.params.push(AbiParam::new(pointer_type));
        let entry_id = module
            .declare_function("code_synth_evaluate", Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;

        let mut context = module.make_context();
        context.func.signature = signature;
        context.func.name = UserFuncName::user(0, entry_id.as_u32());
        let mut function_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut function_context);
            let block = builder.create_block();
            builder.switch_to_block(block);
            builder.append_block_params_for_function_params(block);
            let input_pointer = builder.block_params(block)[0];
            let output_pointer = builder.block_params(block)[1];

            let mut helper_refs = Vec::with_capacity(helper_ids.len());
            for id in helper_ids.iter().copied() {
                helper_refs.push(module.declare_func_in_func(id, builder.func));
            }

            let mut variables = vec![None; variable_count];
            for assignment in assignments {
                let value = lower_expression(
                    &mut builder,
                    &helper_refs,
                    input_pointer,
                    &variables,
                    &assignment.code,
                )?;
                variables[assignment.slot] = Some(value);
            }

            let wave = variable(&variables, wave)?;
            let wave = call_helper(&mut builder, &helper_refs, Helper::Finite, &[wave]);
            let pan = match pan {
                Some(slot) => variable(&variables, slot)?,
                None => f32_constant(&mut builder, 0.0),
            };
            let pan = call_helper(&mut builder, &helper_refs, Helper::Pan, &[pan]);
            let l_limit = variable(&variables, l_limit)?;
            let l_limit = call_helper(&mut builder, &helper_refs, Helper::Finite, &[l_limit]);

            let flags = MemFlagsData::trusted();
            builder.ins().store(
                flags,
                wave,
                output_pointer,
                output_offset(mem::offset_of!(Outputs, wave))?,
            );
            builder.ins().store(
                flags,
                pan,
                output_pointer,
                output_offset(mem::offset_of!(Outputs, pan))?,
            );
            builder.ins().store(
                flags,
                l_limit,
                output_pointer,
                output_offset(mem::offset_of!(Outputs, l_limit))?,
            );
            builder.ins().return_(&[]);
            builder.seal_all_blocks();
            builder.finalize(frontend_config);
        }

        module
            .define_function(entry_id, &mut context)
            .map_err(|error| error.to_string())?;
        module.clear_context(&mut context);
        module
            .finalize_definitions()
            .map_err(|error| error.to_string())?;
        let code = module.get_finalized_function(entry_id);

        // SAFETY: `code_synth_evaluate` is emitted above with exactly two native pointer
        // parameters and no return values. `module` is retained by `JitProgram`, so the
        // executable allocation outlives every call through this pointer.
        let entry = unsafe { mem::transmute::<*const u8, EntryFn>(code) };
        Ok(Self {
            entry,
            module: Mutex::new(Some(module)),
        })
    }

    #[inline]
    pub(super) fn evaluate(&self, input: &Inputs) -> Outputs {
        let mut outputs = Outputs::default();
        // SAFETY: both pointers are valid and correctly aligned for the duration of the call.
        // The generated function only reads `Inputs` and writes the three `Outputs` fields.
        unsafe { (self.entry)(input, &mut outputs) };
        outputs
    }
}

impl Drop for JitProgram {
    fn drop(&mut self) {
        let module = match self.module.get_mut() {
            Ok(module) => module,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(module) = module.take() {
            // SAFETY: `JitProgram` is being dropped, so no safe borrow can still be executing
            // `evaluate`. The entry pointer is private and cannot be called after this point.
            unsafe { module.free_memory() };
        }
    }
}

fn lower_expression(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    input_pointer: Value,
    variables: &[Option<Value>],
    code: &[Op],
) -> Result<Value, String> {
    let mut stack = Vec::with_capacity(code.len().max(1));
    for operation in code.iter().copied() {
        match operation {
            Op::Push(value) => {
                let value = match value {
                    ValueRef::Input(id) => load_input(builder, input_pointer, id)?,
                    ValueRef::Parameter {
                        index,
                        min,
                        span,
                        step,
                    } => {
                        let normalized = builder.ins().load(
                            types::F32,
                            input_load_flags(),
                            input_pointer,
                            parameter_offset(index)?,
                        );
                        let min = f32_constant(builder, min);
                        let span = f32_constant(builder, span);
                        let step = f32_constant(builder, step);
                        call_helper(
                            builder,
                            helpers,
                            Helper::Parameter,
                            &[normalized, min, span, step],
                        )
                    }
                    ValueRef::Variable(slot) => variable(variables, slot)?,
                    ValueRef::Constant(value) => f32_constant(builder, value),
                };
                stack.push(value);
            }
            Op::Noise => stack.push(load_input(builder, input_pointer, InputId::Rand)?),
            Op::Neg => {
                let value = pop(&mut stack)?;
                stack.push(builder.ins().fneg(value));
            }
            Op::Sqrt => {
                let value = pop(&mut stack)?;
                stack.push(builder.ins().sqrt(value));
            }
            Op::Abs => {
                let value = pop(&mut stack)?;
                stack.push(builder.ins().fabs(value));
            }
            Op::Floor => {
                let value = pop(&mut stack)?;
                stack.push(builder.ins().floor(value));
            }
            Op::Ceil => {
                let value = pop(&mut stack)?;
                stack.push(builder.ins().ceil(value));
            }
            Op::Sin
            | Op::Cos
            | Op::Tan
            | Op::Exp
            | Op::Tanh
            | Op::Sinh
            | Op::Cosh
            | Op::Cbrt
            | Op::Ln
            | Op::Log2
            | Op::Log10
            | Op::Round
            | Op::Fract
            | Op::Sign
            | Op::Asin
            | Op::Acos
            | Op::Atan => {
                let value = pop(&mut stack)?;
                let helper = match operation {
                    Op::Sin => Helper::Sin,
                    Op::Cos => Helper::Cos,
                    Op::Tan => Helper::Tan,
                    Op::Exp => Helper::Exp,
                    Op::Tanh => Helper::Tanh,
                    Op::Sinh => Helper::Sinh,
                    Op::Cosh => Helper::Cosh,
                    Op::Cbrt => Helper::Cbrt,
                    Op::Ln => Helper::Ln,
                    Op::Log2 => Helper::Log2,
                    Op::Log10 => Helper::Log10,
                    Op::Round => Helper::Round,
                    Op::Fract => Helper::Fract,
                    Op::Sign => Helper::Sign,
                    Op::Asin => Helper::Asin,
                    Op::Acos => Helper::Acos,
                    Op::Atan => Helper::Atan,
                    _ => unreachable!(),
                };
                stack.push(call_helper(builder, helpers, helper, &[value]));
            }
            Op::Cc => {
                let index = pop(&mut stack)?;
                stack.push(call_helper(
                    builder,
                    helpers,
                    Helper::Cc,
                    &[input_pointer, index],
                ));
            }
            Op::Add | Op::Sub | Op::Mul | Op::Div => {
                let rhs = pop(&mut stack)?;
                let lhs = pop(&mut stack)?;
                let result = match operation {
                    Op::Add => builder.ins().fadd(lhs, rhs),
                    Op::Sub => builder.ins().fsub(lhs, rhs),
                    Op::Mul => builder.ins().fmul(lhs, rhs),
                    Op::Div => builder.ins().fdiv(lhs, rhs),
                    _ => unreachable!(),
                };
                stack.push(result);
            }
            Op::Mod | Op::Pow | Op::Min | Op::Max | Op::Atan2 => {
                let rhs = pop(&mut stack)?;
                let lhs = pop(&mut stack)?;
                let helper = match operation {
                    Op::Mod => Helper::Mod,
                    Op::Pow => Helper::Pow,
                    Op::Min => Helper::Min,
                    Op::Max => Helper::Max,
                    Op::Atan2 => Helper::Atan2,
                    _ => unreachable!(),
                };
                stack.push(call_helper(builder, helpers, helper, &[lhs, rhs]));
            }
            Op::Clamp | Op::Mix => {
                let third = pop(&mut stack)?;
                let second = pop(&mut stack)?;
                let first = pop(&mut stack)?;
                let helper = if matches!(operation, Op::Clamp) {
                    Helper::Clamp
                } else {
                    Helper::Mix
                };
                stack.push(call_helper(
                    builder,
                    helpers,
                    helper,
                    &[first, second, third],
                ));
            }
            Op::Saw | Op::Triangle => {
                let time = pop(&mut stack)?;
                let frequency = pop(&mut stack)?;
                let result = if matches!(operation, Op::Saw) {
                    let sample_rate = load_input(builder, input_pointer, InputId::Sr)?;
                    call_helper(
                        builder,
                        helpers,
                        Helper::Saw,
                        &[frequency, time, sample_rate],
                    )
                } else {
                    call_helper(builder, helpers, Helper::Triangle, &[frequency, time])
                };
                stack.push(result);
            }
            Op::Square => {
                let duty = pop(&mut stack)?;
                let time = pop(&mut stack)?;
                let frequency = pop(&mut stack)?;
                let sample_rate = load_input(builder, input_pointer, InputId::Sr)?;
                stack.push(call_helper(
                    builder,
                    helpers,
                    Helper::Square,
                    &[frequency, time, duty, sample_rate],
                ));
            }
        }
    }
    if stack.len() == 1 {
        Ok(stack[0])
    } else {
        Err(format!(
            "invalid expression stack after lowering: {} values",
            stack.len()
        ))
    }
}

fn pop(stack: &mut Vec<Value>) -> Result<Value, String> {
    stack
        .pop()
        .ok_or_else(|| "invalid expression stack while lowering".to_owned())
}

fn variable(variables: &[Option<Value>], slot: usize) -> Result<Value, String> {
    variables
        .get(slot)
        .copied()
        .flatten()
        .ok_or_else(|| format!("IR references unavailable variable slot {slot}"))
}

fn call_helper(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    helper: Helper,
    arguments: &[Value],
) -> Value {
    let call = builder.ins().call(helpers[helper as usize], arguments);
    builder.inst_results(call)[0]
}

fn f32_constant(builder: &mut FunctionBuilder<'_>, value: f32) -> Value {
    builder.ins().f32const(Ieee32::with_float(value))
}

fn input_load_flags() -> MemFlagsData {
    MemFlagsData::trusted().with_readonly()
}

fn output_offset(offset: usize) -> Result<i32, String> {
    i32::try_from(offset).map_err(|_| "Outputs layout exceeds Cranelift offset range".to_owned())
}

fn parameter_offset(index: usize) -> Result<i32, String> {
    let offset = mem::offset_of!(Inputs, params)
        .checked_add(index.saturating_mul(mem::size_of::<f32>()))
        .ok_or_else(|| "parameter input offset overflow".to_owned())?;
    input_offset(offset)
}

fn input_offset(offset: usize) -> Result<i32, String> {
    i32::try_from(offset).map_err(|_| "Inputs layout exceeds Cranelift offset range".to_owned())
}

fn load_input(
    builder: &mut FunctionBuilder<'_>,
    input_pointer: Value,
    input: InputId,
) -> Result<Value, String> {
    let offset = match input {
        InputId::T => mem::offset_of!(Inputs, t),
        InputId::L => mem::offset_of!(Inputs, l),
        InputId::S => mem::offset_of!(Inputs, s),
        InputId::Freq => mem::offset_of!(Inputs, freq),
        InputId::Note => mem::offset_of!(Inputs, note),
        InputId::Ch => mem::offset_of!(Inputs, ch),
        InputId::Bend => mem::offset_of!(Inputs, bend),
        InputId::BendSt => mem::offset_of!(Inputs, bend_st),
        InputId::Mw => mem::offset_of!(Inputs, mw),
        InputId::Vol => mem::offset_of!(Inputs, vol),
        InputId::MidiPan => mem::offset_of!(Inputs, midi_pan),
        InputId::Mexpr => mem::offset_of!(Inputs, mexpr),
        InputId::Sustain => mem::offset_of!(Inputs, sustain),
        InputId::Pressure => mem::offset_of!(Inputs, pressure),
        InputId::PolyPressure => mem::offset_of!(Inputs, poly_pressure),
        InputId::Program => mem::offset_of!(Inputs, program),
        InputId::Sr => mem::offset_of!(Inputs, sr),
        InputId::Tempo => mem::offset_of!(Inputs, tempo),
        InputId::Beat => mem::offset_of!(Inputs, beat),
        InputId::Bar => mem::offset_of!(Inputs, bar),
        InputId::Ppq => mem::offset_of!(Inputs, ppq),
        InputId::Playing => mem::offset_of!(Inputs, playing),
        InputId::Voice => mem::offset_of!(Inputs, voice),
        InputId::Rand => mem::offset_of!(Inputs, rand),
    };
    Ok(builder.ins().load(
        types::F32,
        input_load_flags(),
        input_pointer,
        input_offset(offset)?,
    ))
}

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
enum Helper {
    Sin,
    Cos,
    Tan,
    Exp,
    Tanh,
    Sinh,
    Cosh,
    Cbrt,
    Ln,
    Log2,
    Log10,
    Round,
    Fract,
    Sign,
    Asin,
    Acos,
    Atan,
    Mod,
    Pow,
    Min,
    Max,
    Atan2,
    Clamp,
    Mix,
    Cc,
    Parameter,
    Saw,
    Square,
    Triangle,
    Finite,
    Pan,
}

impl Helper {
    const ALL: &'static [Self] = &[
        Self::Sin,
        Self::Cos,
        Self::Tan,
        Self::Exp,
        Self::Tanh,
        Self::Sinh,
        Self::Cosh,
        Self::Cbrt,
        Self::Ln,
        Self::Log2,
        Self::Log10,
        Self::Round,
        Self::Fract,
        Self::Sign,
        Self::Asin,
        Self::Acos,
        Self::Atan,
        Self::Mod,
        Self::Pow,
        Self::Min,
        Self::Max,
        Self::Atan2,
        Self::Clamp,
        Self::Mix,
        Self::Cc,
        Self::Parameter,
        Self::Saw,
        Self::Square,
        Self::Triangle,
        Self::Finite,
        Self::Pan,
    ];

    fn symbol(self) -> &'static str {
        match self {
            Self::Sin => "code_synth_jit_sin",
            Self::Cos => "code_synth_jit_cos",
            Self::Tan => "code_synth_jit_tan",
            Self::Exp => "code_synth_jit_exp",
            Self::Tanh => "code_synth_jit_tanh",
            Self::Sinh => "code_synth_jit_sinh",
            Self::Cosh => "code_synth_jit_cosh",
            Self::Cbrt => "code_synth_jit_cbrt",
            Self::Ln => "code_synth_jit_ln",
            Self::Log2 => "code_synth_jit_log2",
            Self::Log10 => "code_synth_jit_log10",
            Self::Round => "code_synth_jit_round",
            Self::Fract => "code_synth_jit_fract",
            Self::Sign => "code_synth_jit_sign",
            Self::Asin => "code_synth_jit_asin",
            Self::Acos => "code_synth_jit_acos",
            Self::Atan => "code_synth_jit_atan",
            Self::Mod => "code_synth_jit_mod",
            Self::Pow => "code_synth_jit_pow",
            Self::Min => "code_synth_jit_min",
            Self::Max => "code_synth_jit_max",
            Self::Atan2 => "code_synth_jit_atan2",
            Self::Clamp => "code_synth_jit_clamp",
            Self::Mix => "code_synth_jit_mix",
            Self::Cc => "code_synth_jit_cc",
            Self::Parameter => "code_synth_jit_parameter",
            Self::Saw => "code_synth_jit_saw",
            Self::Square => "code_synth_jit_square",
            Self::Triangle => "code_synth_jit_triangle",
            Self::Finite => "code_synth_jit_finite",
            Self::Pan => "code_synth_jit_pan",
        }
    }

    fn address(self) -> *const u8 {
        match self {
            Self::Sin => jit_sin as *const u8,
            Self::Cos => jit_cos as *const u8,
            Self::Tan => jit_tan as *const u8,
            Self::Exp => jit_exp as *const u8,
            Self::Tanh => jit_tanh as *const u8,
            Self::Sinh => jit_sinh as *const u8,
            Self::Cosh => jit_cosh as *const u8,
            Self::Cbrt => jit_cbrt as *const u8,
            Self::Ln => jit_ln as *const u8,
            Self::Log2 => jit_log2 as *const u8,
            Self::Log10 => jit_log10 as *const u8,
            Self::Round => jit_round as *const u8,
            Self::Fract => jit_fract as *const u8,
            Self::Sign => jit_sign as *const u8,
            Self::Asin => jit_asin as *const u8,
            Self::Acos => jit_acos as *const u8,
            Self::Atan => jit_atan as *const u8,
            Self::Mod => jit_mod as *const u8,
            Self::Pow => jit_pow as *const u8,
            Self::Min => jit_min as *const u8,
            Self::Max => jit_max as *const u8,
            Self::Atan2 => jit_atan2 as *const u8,
            Self::Clamp => jit_clamp as *const u8,
            Self::Mix => jit_mix as *const u8,
            Self::Cc => jit_cc as *const u8,
            Self::Parameter => jit_parameter as *const u8,
            Self::Saw => jit_saw as *const u8,
            Self::Square => jit_square as *const u8,
            Self::Triangle => jit_triangle as *const u8,
            Self::Finite => jit_finite as *const u8,
            Self::Pan => jit_pan as *const u8,
        }
    }

    fn signature(self, module: &JITModule, pointer_type: Type) -> Signature {
        let mut signature = module.make_signature();
        match self {
            Self::Cc => {
                signature.params.push(AbiParam::new(pointer_type));
                signature.params.push(AbiParam::new(types::F32));
            }
            Self::Parameter | Self::Square => {
                for _ in 0..4 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::Clamp | Self::Mix | Self::Saw => {
                for _ in 0..3 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::Mod | Self::Pow | Self::Min | Self::Max | Self::Atan2 | Self::Triangle => {
                signature.params.push(AbiParam::new(types::F32));
                signature.params.push(AbiParam::new(types::F32));
            }
            _ => signature.params.push(AbiParam::new(types::F32)),
        }
        signature.returns.push(AbiParam::new(types::F32));
        signature
    }
}

macro_rules! unary_helper {
    ($name:ident, $method:ident) => {
        extern "C" fn $name(value: f32) -> f32 {
            value.$method()
        }
    };
}

unary_helper!(jit_sin, sin);
unary_helper!(jit_cos, cos);
unary_helper!(jit_tan, tan);
unary_helper!(jit_exp, exp);
unary_helper!(jit_tanh, tanh);
unary_helper!(jit_sinh, sinh);
unary_helper!(jit_cosh, cosh);
unary_helper!(jit_cbrt, cbrt);
unary_helper!(jit_ln, ln);
unary_helper!(jit_log2, log2);
unary_helper!(jit_log10, log10);
unary_helper!(jit_round, round);
unary_helper!(jit_fract, fract);
unary_helper!(jit_sign, signum);
unary_helper!(jit_asin, asin);
unary_helper!(jit_acos, acos);
unary_helper!(jit_atan, atan);

extern "C" fn jit_mod(lhs: f32, rhs: f32) -> f32 {
    lhs % rhs
}

extern "C" fn jit_pow(lhs: f32, rhs: f32) -> f32 {
    lhs.powf(rhs)
}

extern "C" fn jit_min(lhs: f32, rhs: f32) -> f32 {
    lhs.min(rhs)
}

extern "C" fn jit_max(lhs: f32, rhs: f32) -> f32 {
    lhs.max(rhs)
}

extern "C" fn jit_atan2(lhs: f32, rhs: f32) -> f32 {
    lhs.atan2(rhs)
}

extern "C" fn jit_clamp(value: f32, first_bound: f32, second_bound: f32) -> f32 {
    let lower = first_bound.min(second_bound);
    let upper = first_bound.max(second_bound);
    if lower.is_nan() || upper.is_nan() {
        f32::NAN
    } else {
        value.clamp(lower, upper)
    }
}

extern "C" fn jit_mix(first: f32, second: f32, amount: f32) -> f32 {
    first + (second - first) * amount
}

extern "C" fn jit_cc(input: *const Inputs, index: f32) -> f32 {
    // SAFETY: generated code forwards the valid `Inputs` pointer from its entry ABI.
    let input = unsafe { &*input };
    let index = index.round().clamp(0.0, 127.0) as usize;
    input.cc[index]
}

extern "C" fn jit_parameter(normalized: f32, min: f32, span: f32, step: f32) -> f32 {
    let value = min + normalized.clamp(0.0, 1.0) * span;
    if step > 0.0 {
        (min + ((value - min) / step).round() * step).clamp(min, min + span)
    } else {
        value
    }
}

extern "C" fn jit_saw(frequency: f32, time: f32, sample_rate: f32) -> f32 {
    super::polyblep_saw(frequency, time, sample_rate)
}

extern "C" fn jit_square(frequency: f32, time: f32, duty: f32, sample_rate: f32) -> f32 {
    super::polyblep_square(frequency, time, duty.clamp(0.01, 0.99), sample_rate)
}

extern "C" fn jit_triangle(frequency: f32, time: f32) -> f32 {
    1.0 - 4.0 * (super::positive_mod(frequency * time, 1.0) - 0.5).abs()
}

extern "C" fn jit_finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

extern "C" fn jit_pan(value: f32) -> f32 {
    jit_finite(value).clamp(-1.0, 1.0)
}
