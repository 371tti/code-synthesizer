//! Cranelift lowering for DSL v2 typed stack IR.

use super::{
    AssignmentTarget, EntryOutputs, EntryPlan, InputId, Inputs, Op, Outputs, ValueRef,
    dsp::{jit_biquad_coefficient, jit_standard},
    state::{
        EvalContext, jit_commit_global, jit_commit_voice, jit_dsp_process, jit_ring_duration,
        jit_ring_len, jit_ring_peek, jit_state_read, jit_state_write,
    },
};
use cranelift::{
    codegen::ir::{BlockArg, FuncRef, MemFlagsData, UserFuncName},
    prelude::*,
};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};
use std::{fmt, mem, sync::Mutex};

type EntryFn = unsafe extern "C" fn(*const Inputs, *mut Outputs, *mut EvalContext);
type BlockEntryFn = unsafe extern "C" fn(*const Inputs, *mut Outputs, *mut EvalContext, usize);

pub(super) struct JitProgram {
    note: EntryFn,
    note_block: BlockEntryFn,
    filter: Option<EntryFn>,
    filter_block: Option<BlockEntryFn>,
    module: Mutex<Option<JITModule>>,
}

impl fmt::Debug for JitProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JitProgram")
            .field("note", &(self.note as *const ()))
            .field("filter", &self.filter.map(|entry| entry as *const ()))
            .finish_non_exhaustive()
    }
}

impl JitProgram {
    pub(super) fn compile(note: &EntryPlan, filter: Option<&EntryPlan>) -> Result<Self, String> {
        let mut builder =
            JITBuilder::with_flags(&[("opt_level", "speed")], default_libcall_names())
                .map_err(|error| error.to_string())?;
        for helper in Helper::ALL.iter().copied() {
            builder.symbol(helper.symbol(), helper.address());
        }
        let mut module = JITModule::new(builder);
        let pointer_type = module.target_config().pointer_type();
        let helper_ids = Helper::ALL
            .iter()
            .copied()
            .map(|helper| {
                let signature = helper.signature(&module, pointer_type);
                module
                    .declare_function(helper.symbol(), Linkage::Import, &signature)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;

        let note_id = define_entry(&mut module, &helper_ids, note, "code_synth_note", 0)?;
        let filter_id = filter
            .map(|entry| define_entry(&mut module, &helper_ids, entry, "code_synth_filter", 1))
            .transpose()?;
        let note_block_id = define_block_entry(
            &mut module,
            &helper_ids,
            note_id,
            "code_synth_note_block",
            2,
            Helper::CommitVoice,
        )?;
        let filter_block_id = filter_id
            .map(|id| {
                define_block_entry(
                    &mut module,
                    &helper_ids,
                    id,
                    "code_synth_filter_block",
                    3,
                    Helper::CommitGlobal,
                )
            })
            .transpose()?;
        module
            .finalize_definitions()
            .map_err(|error| error.to_string())?;
        let note_code = module.get_finalized_function(note_id);
        let filter_code = filter_id.map(|id| module.get_finalized_function(id));
        let note_block_code = module.get_finalized_function(note_block_id);
        let filter_block_code = filter_block_id.map(|id| module.get_finalized_function(id));
        // SAFETY: define_entry emits exactly the EntryFn native signature.
        let note = unsafe { mem::transmute::<*const u8, EntryFn>(note_code) };
        // SAFETY: same signature is used for the optional filter entry.
        let filter = filter_code.map(|code| unsafe { mem::transmute::<*const u8, EntryFn>(code) });
        let note_block = unsafe { mem::transmute::<*const u8, BlockEntryFn>(note_block_code) };
        let filter_block = filter_block_code
            .map(|code| unsafe { mem::transmute::<*const u8, BlockEntryFn>(code) });
        Ok(Self {
            note,
            note_block,
            filter,
            filter_block,
            module: Mutex::new(Some(module)),
        })
    }

    #[inline]
    pub(super) fn evaluate_note(&self, input: &Inputs, context: &mut EvalContext) -> Outputs {
        let mut output = Outputs::default();
        // SAFETY: pointers are valid for the native call and match EntryFn.
        unsafe { (self.note)(input, &mut output, context) };
        output
    }

    #[inline]
    pub(super) fn evaluate_filter(&self, input: &Inputs, context: &mut EvalContext) -> Outputs {
        let mut output = Outputs::default();
        if let Some(filter) = self.filter {
            // SAFETY: pointers are valid for the native call and match EntryFn.
            unsafe { filter(input, &mut output, context) };
        }
        output
    }
    pub(super) fn evaluate_note_block(
        &self,
        inputs: &[Inputs],
        outputs: &mut [Outputs],
        context: &mut EvalContext,
    ) {
        unsafe { (self.note_block)(inputs.as_ptr(), outputs.as_mut_ptr(), context, inputs.len()) };
    }
    pub(super) fn evaluate_filter_block(
        &self,
        inputs: &[Inputs],
        outputs: &mut [Outputs],
        context: &mut EvalContext,
    ) {
        if let Some(filter) = self.filter_block {
            unsafe { filter(inputs.as_ptr(), outputs.as_mut_ptr(), context, inputs.len()) };
        }
    }
}

impl Drop for JitProgram {
    fn drop(&mut self) {
        let module = match self.module.get_mut() {
            Ok(module) => module,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(module) = module.take() {
            // SAFETY: no native call can exist after the last Arc owner enters Drop.
            unsafe { module.free_memory() };
        }
    }
}

fn define_entry(
    module: &mut JITModule,
    helper_ids: &[cranelift_module::FuncId],
    plan: &EntryPlan,
    name: &str,
    namespace: u32,
) -> Result<cranelift_module::FuncId, String> {
    let pointer_type = module.target_config().pointer_type();
    let mut signature = module.make_signature();
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    signature.params.push(AbiParam::new(pointer_type));
    let id = module
        .declare_function(name, Linkage::Local, &signature)
        .map_err(|error| error.to_string())?;
    let mut context = module.make_context();
    context.func.signature = signature;
    context.func.name = UserFuncName::user(namespace, id.as_u32());
    let frontend_config = module.target_config();
    let mut function_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut function_context);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.append_block_params_for_function_params(block);
        let input = builder.block_params(block)[0];
        let output = builder.block_params(block)[1];
        let runtime = builder.block_params(block)[2];
        let helpers = helper_ids
            .iter()
            .map(|id| module.declare_func_in_func(*id, builder.func))
            .collect::<Vec<_>>();
        let mut variables = vec![None; plan.variable_count];
        for assignment in &plan.assignments {
            let value = lower_expression(
                &mut builder,
                &helpers,
                input,
                runtime,
                &variables,
                &assignment.code,
            )?;
            match assignment.target {
                AssignmentTarget::Variable(slot) => variables[slot] = Some(value),
                AssignmentTarget::State(index) => {
                    let index = builder.ins().iconst(types::I32, index as i64);
                    call_void(
                        &mut builder,
                        &helpers,
                        Helper::StateWrite,
                        &[runtime, index, value],
                    );
                }
            }
        }
        store_outputs(&mut builder, &helpers, output, &variables, plan.outputs)?;
        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize(frontend_config);
    }
    module
        .define_function(id, &mut context)
        .map_err(|error| error.to_string())?;
    module.clear_context(&mut context);
    Ok(id)
}

fn define_block_entry(
    module: &mut JITModule,
    helpers: &[cranelift_module::FuncId],
    scalar_id: cranelift_module::FuncId,
    name: &str,
    namespace: u32,
    commit: Helper,
) -> Result<cranelift_module::FuncId, String> {
    let pointer = module.target_config().pointer_type();
    let mut signature = module.make_signature();
    for _ in 0..4 {
        signature.params.push(AbiParam::new(pointer));
    }
    let id = module
        .declare_function(name, Linkage::Local, &signature)
        .map_err(|e| e.to_string())?;
    let mut context = module.make_context();
    context.func.signature = signature;
    context.func.name = UserFuncName::user(namespace, id.as_u32());
    let mut fbctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut context.func, &mut fbctx);
        let entry = b.create_block();
        let loop_block = b.create_block();
        let done = b.create_block();
        b.switch_to_block(entry);
        b.append_block_params_for_function_params(entry);
        let p = b.block_params(entry);
        let input = p[0];
        let output = p[1];
        let runtime = p[2];
        let count = p[3];
        let scalar = module.declare_func_in_func(scalar_id, b.func);
        let commit_ref = module.declare_func_in_func(helpers[commit as usize], b.func);
        let zero = b.ins().iconst(pointer, 0);
        let active = b.ins().icmp(IntCC::NotEqual, count, zero);
        let initial = [
            BlockArg::Value(input),
            BlockArg::Value(output),
            BlockArg::Value(runtime),
            BlockArg::Value(count),
        ];
        b.ins().brif(active, loop_block, &initial, done, &[]);
        b.switch_to_block(loop_block);
        b.append_block_param(loop_block, pointer);
        b.append_block_param(loop_block, pointer);
        b.append_block_param(loop_block, pointer);
        b.append_block_param(loop_block, pointer);
        let q = b.block_params(loop_block);
        let (qi, qo, qr, qc) = (q[0], q[1], q[2], q[3]);
        b.ins().call(scalar, &[qi, qo, qr]);
        b.ins().call(commit_ref, &[qr]);
        let ni = b.ins().iadd_imm_u(qi, std::mem::size_of::<Inputs>() as i64);
        let no = b
            .ins()
            .iadd_imm_u(qo, std::mem::size_of::<Outputs>() as i64);
        let nc = b.ins().iadd_imm_s(qc, -1);
        let more = b.ins().icmp_imm_u(IntCC::NotEqual, nc, 0);
        let next = [
            BlockArg::Value(ni),
            BlockArg::Value(no),
            BlockArg::Value(qr),
            BlockArg::Value(nc),
        ];
        b.ins().brif(more, loop_block, &next, done, &[]);
        b.switch_to_block(done);
        b.ins().return_(&[]);
        b.seal_all_blocks();
        b.finalize(module.target_config());
    }
    module
        .define_function(id, &mut context)
        .map_err(|e| e.to_string())?;
    module.clear_context(&mut context);
    Ok(id)
}

fn store_outputs(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    output: Value,
    variables: &[Option<Value>],
    outputs: EntryOutputs,
) -> Result<(), String> {
    let finite = |builder: &mut FunctionBuilder<'_>, value| {
        call_value(builder, helpers, Helper::Finite, &[value])
    };
    let store = |builder: &mut FunctionBuilder<'_>, offset: usize, value| -> Result<(), String> {
        builder
            .ins()
            .store(MemFlagsData::trusted(), value, output, offset_i32(offset)?);
        Ok(())
    };
    match outputs {
        EntryOutputs::NoteMono { wave, pan, l_limit } => {
            let wave = finite(builder, variable(variables, wave)?);
            let limit = finite(builder, variable(variables, l_limit)?);
            store(builder, mem::offset_of!(Outputs, wave), wave)?;
            if let Some(pan) = pan {
                let pan = finite(builder, variable(variables, pan)?);
                store(builder, mem::offset_of!(Outputs, pan), pan)?;
            }
            store(builder, mem::offset_of!(Outputs, l_limit), limit)?;
        }
        EntryOutputs::NoteStereo {
            wave_l,
            wave_r,
            l_limit,
        } => {
            let left = finite(builder, variable(variables, wave_l)?);
            let right = finite(builder, variable(variables, wave_r)?);
            let limit = finite(builder, variable(variables, l_limit)?);
            store(builder, mem::offset_of!(Outputs, wave_l), left)?;
            store(builder, mem::offset_of!(Outputs, wave_r), right)?;
            store(builder, mem::offset_of!(Outputs, l_limit), limit)?;
        }
        EntryOutputs::FilterMono { wave } => {
            let wave = finite(builder, variable(variables, wave)?);
            store(builder, mem::offset_of!(Outputs, wave), wave)?;
        }
        EntryOutputs::FilterStereo { wave_l, wave_r } => {
            let left = finite(builder, variable(variables, wave_l)?);
            let right = finite(builder, variable(variables, wave_r)?);
            store(builder, mem::offset_of!(Outputs, wave_l), left)?;
            store(builder, mem::offset_of!(Outputs, wave_r), right)?;
        }
    }
    Ok(())
}

fn lower_expression(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    input: Value,
    runtime: Value,
    variables: &[Option<Value>],
    code: &[Op],
) -> Result<Value, String> {
    let mut stack = Vec::with_capacity(code.len().max(1));
    for operation in code.iter().copied() {
        match operation {
            Op::Push(reference) => {
                let value = match reference {
                    ValueRef::Input(id) => load_input(builder, input, id)?,
                    ValueRef::Parameter {
                        index,
                        min,
                        span,
                        step,
                    } => {
                        let normalized = builder.ins().load(
                            types::F32,
                            input_flags(),
                            input,
                            parameter_offset(index)?,
                        );
                        let args = [
                            normalized,
                            f32_constant(builder, min),
                            f32_constant(builder, span),
                            f32_constant(builder, step),
                        ];
                        call_value(builder, helpers, Helper::Parameter, &args)
                    }
                    ValueRef::Variable(slot) => variable(variables, slot)?,
                    ValueRef::State(index) => {
                        let index = builder.ins().iconst(types::I32, index as i64);
                        call_value(builder, helpers, Helper::StateRead, &[runtime, index])
                    }
                    ValueRef::Constant(value) => f32_constant(builder, value),
                };
                stack.push(value);
            }
            Op::Noise => stack.push(load_input(builder, input, InputId::Rand)?),
            Op::Neg => {
                let x = pop(&mut stack)?;
                stack.push(builder.ins().fneg(x));
            }
            Op::Sqrt => {
                let x = pop(&mut stack)?;
                stack.push(builder.ins().sqrt(x));
            }
            Op::Abs => {
                let x = pop(&mut stack)?;
                stack.push(builder.ins().fabs(x));
            }
            Op::Floor => {
                let x = pop(&mut stack)?;
                stack.push(builder.ins().floor(x));
            }
            Op::Ceil => {
                let x = pop(&mut stack)?;
                stack.push(builder.ins().ceil(x));
            }
            Op::Add | Op::Sub | Op::Mul | Op::Div => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                stack.push(match operation {
                    Op::Add => builder.ins().fadd(a, b),
                    Op::Sub => builder.ins().fsub(a, b),
                    Op::Mul => builder.ins().fmul(a, b),
                    Op::Div => builder.ins().fdiv(a, b),
                    _ => unreachable!(),
                });
            }
            Op::Less
            | Op::LessEqual
            | Op::Greater
            | Op::GreaterEqual
            | Op::Equal
            | Op::NotEqual => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                let cc = match operation {
                    Op::Less => FloatCC::LessThan,
                    Op::LessEqual => FloatCC::LessThanOrEqual,
                    Op::Greater => FloatCC::GreaterThan,
                    Op::GreaterEqual => FloatCC::GreaterThanOrEqual,
                    Op::Equal => FloatCC::Equal,
                    Op::NotEqual => FloatCC::NotEqual,
                    _ => unreachable!(),
                };
                let condition = builder.ins().fcmp(cc, a, b);
                let one = f32_constant(builder, 1.0);
                let zero = f32_constant(builder, 0.0);
                stack.push(builder.ins().select(condition, one, zero));
            }
            op => lower_helper_operation(builder, helpers, input, runtime, &mut stack, op)?,
        }
    }
    if stack.len() == 1 {
        Ok(stack[0])
    } else {
        Err(format!("invalid expression stack: {}", stack.len()))
    }
}

fn lower_helper_operation(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    input: Value,
    runtime: Value,
    stack: &mut Vec<Value>,
    op: Op,
) -> Result<(), String> {
    let unary = match op {
        Op::Sin => Some(Helper::Sin),
        Op::Cos => Some(Helper::Cos),
        Op::Tan => Some(Helper::Tan),
        Op::Exp => Some(Helper::Exp),
        Op::Tanh => Some(Helper::Tanh),
        Op::Sinh => Some(Helper::Sinh),
        Op::Cosh => Some(Helper::Cosh),
        Op::Cbrt => Some(Helper::Cbrt),
        Op::Ln => Some(Helper::Ln),
        Op::Log2 => Some(Helper::Log2),
        Op::Log10 => Some(Helper::Log10),
        Op::Round => Some(Helper::Round),
        Op::Fract => Some(Helper::Fract),
        Op::Sign => Some(Helper::Sign),
        Op::Asin => Some(Helper::Asin),
        Op::Acos => Some(Helper::Acos),
        Op::Atan => Some(Helper::Atan),
        Op::Mtof => Some(Helper::Mtof),
        Op::Ftom => Some(Helper::Ftom),
        Op::DbToA => Some(Helper::DbToA),
        Op::AToDb => Some(Helper::AToDb),
        Op::CentRatio => Some(Helper::CentRatio),
        Op::SemitoneRatio => Some(Helper::SemitoneRatio),
        _ => None,
    };
    if let Some(helper) = unary {
        let x = pop(stack)?;
        stack.push(call_value(builder, helpers, helper, &[x]));
        return Ok(());
    }
    match op {
        Op::Standard(operation) => {
            let arguments = pop_arguments(stack, operation.arity())?;
            let mut call_arguments = Vec::with_capacity(6);
            call_arguments.push(builder.ins().iconst(types::I32, operation as i64));
            call_arguments.extend(arguments);
            while call_arguments.len() < 6 {
                call_arguments.push(f32_constant(builder, 0.0));
            }
            stack.push(call_value(
                builder,
                helpers,
                Helper::Standard,
                &call_arguments,
            ));
        }
        Op::BiquadCoefficient {
            kind,
            coefficient,
            arity,
        } => {
            let arguments = pop_arguments(stack, arity as usize)?;
            let kind = builder.ins().iconst(types::I32, kind as i64);
            let coefficient = builder.ins().iconst(types::I32, coefficient as i64);
            let zero = f32_constant(builder, 0.0);
            let (frequency, q, gain, sample_rate) = if arity == 3 {
                (arguments[0], arguments[1], zero, arguments[2])
            } else {
                (arguments[0], arguments[1], arguments[2], arguments[3])
            };
            stack.push(call_value(
                builder,
                helpers,
                Helper::BiquadCoefficient,
                &[kind, coefficient, frequency, q, gain, sample_rate],
            ));
        }
        Op::Dsp { index, arity } => {
            let arguments = pop_arguments(stack, arity as usize)?;
            let mut call_arguments = Vec::with_capacity(7);
            call_arguments.push(runtime);
            call_arguments.push(builder.ins().iconst(types::I32, index as i64));
            call_arguments.extend(arguments);
            while call_arguments.len() < 7 {
                call_arguments.push(f32_constant(builder, 0.0));
            }
            stack.push(call_value(builder, helpers, Helper::Dsp, &call_arguments));
        }
        Op::RingPeek { index, linear } => {
            let delay = pop(stack)?;
            let index = builder.ins().iconst(types::I32, index as i64);
            let linear = builder.ins().iconst(types::I32, i64::from(linear));
            stack.push(call_value(
                builder,
                helpers,
                Helper::RingPeek,
                &[runtime, index, delay, linear],
            ));
        }
        Op::RingLen { index } | Op::RingDuration { index } => {
            let helper = if matches!(op, Op::RingLen { .. }) {
                Helper::RingLen
            } else {
                Helper::RingDuration
            };
            let index = builder.ins().iconst(types::I32, index as i64);
            stack.push(call_value(builder, helpers, helper, &[runtime, index]));
        }
        Op::Cc => {
            let index = pop(stack)?;
            stack.push(call_value(builder, helpers, Helper::Cc, &[input, index]));
        }
        Op::Mod | Op::Pow | Op::Min | Op::Max | Op::Atan2 | Op::Step | Op::Triangle => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            let helper = match op {
                Op::Mod => Helper::Mod,
                Op::Pow => Helper::Pow,
                Op::Min => Helper::Min,
                Op::Max => Helper::Max,
                Op::Atan2 => Helper::Atan2,
                Op::Step => Helper::Step,
                Op::Triangle => Helper::Triangle,
                _ => unreachable!(),
            };
            stack.push(call_value(builder, helpers, helper, &[a, b]));
        }
        Op::Clamp | Op::Mix | Op::SmoothStep | Op::Select => {
            let c = pop(stack)?;
            let b = pop(stack)?;
            let a = pop(stack)?;
            let helper = match op {
                Op::Clamp => Helper::Clamp,
                Op::Mix => Helper::Mix,
                Op::SmoothStep => Helper::SmoothStep,
                Op::Select => Helper::Select,
                _ => unreachable!(),
            };
            stack.push(call_value(builder, helpers, helper, &[a, b, c]));
        }
        Op::Saw | Op::Square => {
            let t = pop(stack)?;
            let frequency = pop(stack)?;
            let sr = load_input(builder, input, InputId::Sr)?;
            let helper = if matches!(op, Op::Saw) {
                Helper::Saw
            } else {
                Helper::Square
            };
            stack.push(call_value(builder, helpers, helper, &[frequency, t, sr]));
        }
        Op::Pulse => {
            let duty = pop(stack)?;
            let t = pop(stack)?;
            let frequency = pop(stack)?;
            let sr = load_input(builder, input, InputId::Sr)?;
            stack.push(call_value(
                builder,
                helpers,
                Helper::Pulse,
                &[frequency, t, duty, sr],
            ));
        }
        _ => return Err(format!("unhandled JIT operation: {op:?}")),
    }
    Ok(())
}

fn pop(stack: &mut Vec<Value>) -> Result<Value, String> {
    stack.pop().ok_or_else(|| "invalid expression stack".into())
}
fn pop_arguments(stack: &mut Vec<Value>, arity: usize) -> Result<Vec<Value>, String> {
    if stack.len() < arity {
        return Err("invalid expression stack".into());
    }
    Ok(stack.drain(stack.len() - arity..).collect())
}
fn variable(values: &[Option<Value>], slot: usize) -> Result<Value, String> {
    values
        .get(slot)
        .copied()
        .flatten()
        .ok_or_else(|| format!("unavailable variable slot {slot}"))
}
fn call_value(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    helper: Helper,
    args: &[Value],
) -> Value {
    let call = builder.ins().call(helpers[helper as usize], args);
    builder.inst_results(call)[0]
}
fn call_void(
    builder: &mut FunctionBuilder<'_>,
    helpers: &[FuncRef],
    helper: Helper,
    args: &[Value],
) {
    builder.ins().call(helpers[helper as usize], args);
}
fn f32_constant(builder: &mut FunctionBuilder<'_>, value: f32) -> Value {
    builder.ins().f32const(Ieee32::with_float(value))
}
fn input_flags() -> MemFlagsData {
    MemFlagsData::trusted().with_readonly()
}
fn offset_i32(offset: usize) -> Result<i32, String> {
    i32::try_from(offset).map_err(|_| "ABI offset overflow".into())
}
fn parameter_offset(index: usize) -> Result<i32, String> {
    offset_i32(mem::offset_of!(Inputs, params) + index * mem::size_of::<f32>())
}
fn load_input(
    builder: &mut FunctionBuilder<'_>,
    pointer: Value,
    id: InputId,
) -> Result<Value, String> {
    let offset = match id {
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
        InputId::Wave => mem::offset_of!(Inputs, wave),
        InputId::WaveL => mem::offset_of!(Inputs, wave_l),
        InputId::WaveR => mem::offset_of!(Inputs, wave_r),
    };
    Ok(builder
        .ins()
        .load(types::F32, input_flags(), pointer, offset_i32(offset)?))
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
    Mtof,
    Ftom,
    DbToA,
    AToDb,
    CentRatio,
    SemitoneRatio,
    Mod,
    Pow,
    Min,
    Max,
    Atan2,
    Step,
    Triangle,
    Clamp,
    Mix,
    SmoothStep,
    Select,
    Saw,
    Square,
    Pulse,
    Cc,
    Parameter,
    Finite,
    Pan,
    StateRead,
    StateWrite,
    CommitVoice,
    CommitGlobal,
    Standard,
    BiquadCoefficient,
    Dsp,
    RingPeek,
    RingLen,
    RingDuration,
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
        Self::Mtof,
        Self::Ftom,
        Self::DbToA,
        Self::AToDb,
        Self::CentRatio,
        Self::SemitoneRatio,
        Self::Mod,
        Self::Pow,
        Self::Min,
        Self::Max,
        Self::Atan2,
        Self::Step,
        Self::Triangle,
        Self::Clamp,
        Self::Mix,
        Self::SmoothStep,
        Self::Select,
        Self::Saw,
        Self::Square,
        Self::Pulse,
        Self::Cc,
        Self::Parameter,
        Self::Finite,
        Self::Pan,
        Self::StateRead,
        Self::StateWrite,
        Self::CommitVoice,
        Self::CommitGlobal,
        Self::Standard,
        Self::BiquadCoefficient,
        Self::Dsp,
        Self::RingPeek,
        Self::RingLen,
        Self::RingDuration,
    ];
    fn symbol(self) -> &'static str {
        match self {
            Self::Sin => "cs_sin",
            Self::Cos => "cs_cos",
            Self::Tan => "cs_tan",
            Self::Exp => "cs_exp",
            Self::Tanh => "cs_tanh",
            Self::Sinh => "cs_sinh",
            Self::Cosh => "cs_cosh",
            Self::Cbrt => "cs_cbrt",
            Self::Ln => "cs_ln",
            Self::Log2 => "cs_log2",
            Self::Log10 => "cs_log10",
            Self::Round => "cs_round",
            Self::Fract => "cs_fract",
            Self::Sign => "cs_sign",
            Self::Asin => "cs_asin",
            Self::Acos => "cs_acos",
            Self::Atan => "cs_atan",
            Self::Mtof => "cs_mtof",
            Self::Ftom => "cs_ftom",
            Self::DbToA => "cs_dbtoa",
            Self::AToDb => "cs_atodb",
            Self::CentRatio => "cs_cent_ratio",
            Self::SemitoneRatio => "cs_semitone_ratio",
            Self::Mod => "cs_mod",
            Self::Pow => "cs_pow",
            Self::Min => "cs_min",
            Self::Max => "cs_max",
            Self::Atan2 => "cs_atan2",
            Self::Step => "cs_step",
            Self::Triangle => "cs_triangle",
            Self::Clamp => "cs_clamp",
            Self::Mix => "cs_mix",
            Self::SmoothStep => "cs_smoothstep",
            Self::Select => "cs_select",
            Self::Saw => "cs_saw",
            Self::Square => "cs_square",
            Self::Pulse => "cs_pulse",
            Self::Cc => "cs_cc",
            Self::Parameter => "cs_parameter",
            Self::Finite => "cs_finite",
            Self::Pan => "cs_pan",
            Self::StateRead => "cs_state_read",
            Self::StateWrite => "cs_state_write",
            Self::CommitVoice => "cs_commit_voice",
            Self::CommitGlobal => "cs_commit_global",
            Self::Standard => "cs_standard",
            Self::BiquadCoefficient => "cs_biquad_coefficient",
            Self::Dsp => "cs_dsp",
            Self::RingPeek => "cs_ring_peek",
            Self::RingLen => "cs_ring_len",
            Self::RingDuration => "cs_ring_duration",
        }
    }
    fn address(self) -> *const u8 {
        match self {
            Self::Sin => h_sin as _,
            Self::Cos => h_cos as _,
            Self::Tan => h_tan as _,
            Self::Exp => h_exp as _,
            Self::Tanh => h_tanh as _,
            Self::Sinh => h_sinh as _,
            Self::Cosh => h_cosh as _,
            Self::Cbrt => h_cbrt as _,
            Self::Ln => h_ln as _,
            Self::Log2 => h_log2 as _,
            Self::Log10 => h_log10 as _,
            Self::Round => h_round as _,
            Self::Fract => h_fract as _,
            Self::Sign => h_sign as _,
            Self::Asin => h_asin as _,
            Self::Acos => h_acos as _,
            Self::Atan => h_atan as _,
            Self::Mtof => h_mtof as _,
            Self::Ftom => h_ftom as _,
            Self::DbToA => h_dbtoa as _,
            Self::AToDb => h_atodb as _,
            Self::CentRatio => h_cent_ratio as _,
            Self::SemitoneRatio => h_semitone_ratio as _,
            Self::Mod => h_mod as _,
            Self::Pow => h_pow as _,
            Self::Min => h_min as _,
            Self::Max => h_max as _,
            Self::Atan2 => h_atan2 as _,
            Self::Step => h_step as _,
            Self::Triangle => h_triangle as _,
            Self::Clamp => h_clamp as _,
            Self::Mix => h_mix as _,
            Self::SmoothStep => h_smoothstep as _,
            Self::Select => h_select as _,
            Self::Saw => h_saw as _,
            Self::Square => h_square as _,
            Self::Pulse => h_pulse as _,
            Self::Cc => h_cc as _,
            Self::Parameter => h_parameter as _,
            Self::Finite => h_finite as _,
            Self::Pan => h_pan as _,
            Self::StateRead => jit_state_read as _,
            Self::StateWrite => jit_state_write as _,
            Self::CommitVoice => jit_commit_voice as _,
            Self::CommitGlobal => jit_commit_global as _,
            Self::Standard => jit_standard as _,
            Self::BiquadCoefficient => jit_biquad_coefficient as _,
            Self::Dsp => jit_dsp_process as _,
            Self::RingPeek => jit_ring_peek as _,
            Self::RingLen => jit_ring_len as _,
            Self::RingDuration => jit_ring_duration as _,
        }
    }
    fn signature(self, module: &JITModule, pointer: Type) -> Signature {
        let mut signature = module.make_signature();
        match self {
            Self::Standard => {
                signature.params.push(AbiParam::new(types::I32));
                for _ in 0..5 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::BiquadCoefficient => {
                signature.params.push(AbiParam::new(types::I32));
                signature.params.push(AbiParam::new(types::I32));
                for _ in 0..4 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::Dsp => {
                signature.params.push(AbiParam::new(pointer));
                signature.params.push(AbiParam::new(types::I32));
                for _ in 0..5 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::RingPeek => {
                signature.params.push(AbiParam::new(pointer));
                signature.params.push(AbiParam::new(types::I32));
                signature.params.push(AbiParam::new(types::F32));
                signature.params.push(AbiParam::new(types::I32));
            }
            Self::RingLen | Self::RingDuration => {
                signature.params.push(AbiParam::new(pointer));
                signature.params.push(AbiParam::new(types::I32));
            }
            Self::StateRead => {
                signature.params.push(AbiParam::new(pointer));
                signature.params.push(AbiParam::new(types::I32));
            }
            Self::StateWrite => {
                signature.params.push(AbiParam::new(pointer));
                signature.params.push(AbiParam::new(types::I32));
                signature.params.push(AbiParam::new(types::F32));
                return signature;
            }
            Self::CommitVoice | Self::CommitGlobal => {
                signature.params.push(AbiParam::new(pointer));
                return signature;
            }
            Self::Cc => {
                signature.params.push(AbiParam::new(pointer));
                signature.params.push(AbiParam::new(types::F32));
            }
            Self::Parameter | Self::Pulse => {
                for _ in 0..4 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::Clamp
            | Self::Mix
            | Self::SmoothStep
            | Self::Select
            | Self::Saw
            | Self::Square => {
                for _ in 0..3 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            Self::Mod
            | Self::Pow
            | Self::Min
            | Self::Max
            | Self::Atan2
            | Self::Step
            | Self::Triangle => {
                for _ in 0..2 {
                    signature.params.push(AbiParam::new(types::F32));
                }
            }
            _ => signature.params.push(AbiParam::new(types::F32)),
        }
        signature.returns.push(AbiParam::new(types::F32));
        signature
    }
}

macro_rules! unary {
    ($name:ident,$method:ident) => {
        extern "C" fn $name(x: f32) -> f32 {
            x.$method()
        }
    };
}
unary!(h_sin, sin);
unary!(h_cos, cos);
unary!(h_tan, tan);
unary!(h_exp, exp);
unary!(h_tanh, tanh);
unary!(h_sinh, sinh);
unary!(h_cosh, cosh);
unary!(h_cbrt, cbrt);
unary!(h_ln, ln);
unary!(h_log2, log2);
unary!(h_log10, log10);
unary!(h_round, round);
unary!(h_fract, fract);
unary!(h_sign, signum);
unary!(h_asin, asin);
unary!(h_acos, acos);
unary!(h_atan, atan);
extern "C" fn h_mtof(n: f32) -> f32 {
    440.0 * 2.0f32.powf((n - 69.0) / 12.0)
}
extern "C" fn h_ftom(f: f32) -> f32 {
    69.0 + 12.0 * (f / 440.0).log2()
}
extern "C" fn h_dbtoa(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}
extern "C" fn h_atodb(a: f32) -> f32 {
    20.0 * a.abs().log10()
}
extern "C" fn h_cent_ratio(c: f32) -> f32 {
    2.0f32.powf(c / 1200.0)
}
extern "C" fn h_semitone_ratio(s: f32) -> f32 {
    2.0f32.powf(s / 12.0)
}
extern "C" fn h_mod(a: f32, b: f32) -> f32 {
    a % b
}
extern "C" fn h_pow(a: f32, b: f32) -> f32 {
    a.powf(b)
}
extern "C" fn h_min(a: f32, b: f32) -> f32 {
    a.min(b)
}
extern "C" fn h_max(a: f32, b: f32) -> f32 {
    a.max(b)
}
extern "C" fn h_atan2(a: f32, b: f32) -> f32 {
    a.atan2(b)
}
extern "C" fn h_step(edge: f32, x: f32) -> f32 {
    f32::from(x >= edge)
}
extern "C" fn h_triangle(f: f32, t: f32) -> f32 {
    2.0 * (2.0 * (f * t - (f * t + 0.5).floor())).abs() - 1.0
}
extern "C" fn h_clamp(x: f32, a: f32, b: f32) -> f32 {
    let lo = a.min(b);
    let hi = a.max(b);
    if lo.is_nan() || hi.is_nan() {
        f32::NAN
    } else {
        x.clamp(lo, hi)
    }
}
extern "C" fn h_mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
extern "C" fn h_smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
extern "C" fn h_select(c: f32, a: f32, b: f32) -> f32 {
    if c == 0.0 { b } else { a }
}
extern "C" fn h_saw(f: f32, t: f32, sr: f32) -> f32 {
    let phase = (f * t).rem_euclid(1.0);
    let naive = 2.0 * phase - 1.0;
    naive - poly_blep(phase, (f.abs() / sr.max(1.0)).clamp(0.0, 1.0))
}
extern "C" fn h_square(f: f32, t: f32, sr: f32) -> f32 {
    h_pulse(f, t, 0.5, sr)
}
extern "C" fn h_pulse(f: f32, t: f32, duty: f32, sr: f32) -> f32 {
    let phase = (f * t).rem_euclid(1.0);
    let duty = duty.clamp(0.001, 0.999);
    let dt = (f.abs() / sr.max(1.0)).clamp(0.0, 1.0);
    let mut value = if phase < duty { 1.0 } else { -1.0 };
    value += poly_blep(phase, dt);
    value -= poly_blep((phase - duty).rem_euclid(1.0), dt);
    value
}
fn poly_blep(phase: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if phase < dt {
        let x = phase / dt;
        x + x - x * x - 1.0
    } else if phase > 1.0 - dt {
        let x = (phase - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}
extern "C" fn h_parameter(n: f32, min: f32, span: f32, step: f32) -> f32 {
    let value = min + n.clamp(0.0, 1.0) * span;
    (min + ((value - min) / step).round() * step).clamp(min, min + span)
}
extern "C" fn h_finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}
extern "C" fn h_pan(x: f32) -> f32 {
    h_finite(x).clamp(-1.0, 1.0)
}
extern "C" fn h_cc(input: *const Inputs, index: f32) -> f32 {
    if input.is_null() || !index.is_finite() {
        return 0.0;
    }
    let index = index.round().clamp(0.0, 127.0) as usize;
    // SAFETY: native entry passes its valid Inputs pointer unchanged.
    unsafe { (*input).cc[index] }
}
