//! Allocation-free voice-affine block worker scheduler.

use crate::{MAX_VOICES, VOICE_WORKER_ENABLED, next_rng, program::RuntimeProgram};
use crossbeam_queue::ArrayQueue;
use std::{
    hint::spin_loop,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle, Thread},
};
use synth_dsl::{Inputs, NoteOutputMode, Outputs, ProgramInstance};

pub(crate) const MAX_BLOCK_FRAMES: usize = 256;
pub(crate) const MAX_WORKERS: usize = 4;

#[derive(Clone, Copy, Default)]
pub(crate) struct VoiceBlockSpec {
    pub(crate) input: Inputs,
    pub(crate) voice_slot: usize,
    pub(crate) released: bool,
    pub(crate) rng: u32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct VoiceBlockResult {
    pub(crate) rendered_frames: usize,
    pub(crate) t: f32,
    pub(crate) l: f32,
    pub(crate) rng: u32,
    pub(crate) became_inactive: bool,
}

struct WorkerPacket {
    frame_count: usize,
    ppq_step: f32,
    output_mode: NoteOutputMode,
    voice_count: usize,
    voices: [VoiceBlockSpec; MAX_VOICES],
    results: [VoiceBlockResult; MAX_VOICES],
    inputs: Vec<Inputs>,
    outputs: Vec<Outputs>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
}

impl WorkerPacket {
    fn new() -> Self {
        Self {
            frame_count: 0,
            ppq_step: 0.0,
            output_mode: NoteOutputMode::Mono,
            voice_count: 0,
            voices: [VoiceBlockSpec::default(); MAX_VOICES],
            results: [VoiceBlockResult::default(); MAX_VOICES],
            inputs: vec![Inputs::default(); MAX_BLOCK_FRAMES],
            outputs: vec![Outputs::default(); MAX_BLOCK_FRAMES],
            mix_l: vec![0.0; MAX_BLOCK_FRAMES],
            mix_r: vec![0.0; MAX_BLOCK_FRAMES],
        }
    }
}

enum WorkerCommand {
    Configure(Box<ProgramInstance>),
    ResetVoice(usize),
    Shutdown,
}

struct Endpoint {
    // Only a Box handle crosses each queue. The packet and every backing buffer
    // are allocated once at pool construction and cycle job -> completion -> free.
    jobs: Arc<ArrayQueue<Box<WorkerPacket>>>,
    free: Arc<ArrayQueue<Box<WorkerPacket>>>,
    commands: Arc<ArrayQueue<WorkerCommand>>,
    thread: Thread,
    handle: Option<JoinHandle<()>>,
}

pub(crate) struct VoiceWorkerPool {
    completed: Arc<ArrayQueue<(usize, Box<WorkerPacket>)>>,
    configured: Arc<ArrayQueue<(usize, Box<ProgramInstance>)>>,
    ready: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    endpoints: Vec<Endpoint>,
}

impl VoiceWorkerPool {
    pub(crate) fn new(program: &mut RuntimeProgram, enabled: bool) -> Option<Self> {
        if !VOICE_WORKER_ENABLED || !enabled {
            return None;
        }
        let count = thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .saturating_sub(1)
            .clamp(1, MAX_WORKERS);
        let completed: Arc<ArrayQueue<(usize, Box<WorkerPacket>)>> =
            Arc::new(ArrayQueue::new(MAX_WORKERS * 2));
        let configured: Arc<ArrayQueue<(usize, Box<ProgramInstance>)>> =
            Arc::new(ArrayQueue::new(MAX_WORKERS));
        let ready = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut endpoints = Vec::with_capacity(count);
        for worker_index in 0..count {
            let jobs: Arc<ArrayQueue<Box<WorkerPacket>>> = Arc::new(ArrayQueue::new(1));
            let free: Arc<ArrayQueue<Box<WorkerPacket>>> = Arc::new(ArrayQueue::new(1));
            free.push(Box::new(WorkerPacket::new())).ok().unwrap();
            let commands = Arc::new(ArrayQueue::new(MAX_VOICES * 2));
            let (
                worker_jobs,
                worker_commands,
                worker_completed,
                worker_configured,
                worker_ready,
                worker_shutdown,
            ) = (
                jobs.clone(),
                commands.clone(),
                completed.clone(),
                configured.clone(),
                ready.clone(),
                shutdown.clone(),
            );
            let initial_runtime = program.take_worker_instance(worker_index);
            let handle = thread::Builder::new()
                .name(format!("code-synth-voice-{worker_index}"))
                .spawn(move || {
                    let mut runtime = initial_runtime;
                    worker_ready.fetch_add(1, Ordering::Release);
                    loop {
                        while let Some(command) = worker_commands.pop() {
                            match command {
                                WorkerCommand::Configure(next) => {
                                    let mut previous =
                                        (worker_index, std::mem::replace(&mut runtime, next));
                                    loop {
                                        match worker_configured.push(previous) {
                                            Ok(()) => break,
                                            Err(returned) => {
                                                previous = returned;
                                                spin_loop();
                                            }
                                        }
                                    }
                                    worker_ready.fetch_add(1, Ordering::Release);
                                }
                                WorkerCommand::ResetVoice(slot) => runtime.reset_voice(slot),
                                WorkerCommand::Shutdown => return,
                            }
                        }
                        if worker_shutdown.load(Ordering::Acquire) {
                            return;
                        }
                        if let Some(mut packet) = worker_jobs.pop() {
                            process_packet(&mut runtime, &mut packet);
                            let mut value = (worker_index, packet);
                            loop {
                                match worker_completed.push(value) {
                                    Ok(()) => break,
                                    Err(returned) => {
                                        value = returned;
                                        spin_loop();
                                    }
                                }
                            }
                        } else {
                            thread::park();
                        }
                    }
                })
                .expect("voice worker thread must start");
            let thread = handle.thread().clone();
            endpoints.push(Endpoint {
                jobs,
                free,
                commands,
                thread,
                handle: Some(handle),
            });
        }
        let pool = Self {
            completed,
            configured,
            ready,
            shutdown,
            endpoints,
        };
        // Construction is a setup-time operation. Returning only after every
        // persistent worker owns a fully prepared runtime prevents the first
        // audio block from taking a different execution path nondeterministically.
        while !pool.is_ready() {
            spin_loop();
        }
        Some(pool)
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) >= self.endpoints.len() as u64
    }
    #[cfg(test)]
    pub(crate) fn worker_count(&self) -> usize {
        self.endpoints.len()
    }
    pub(crate) fn reset_voice(&self, slot: usize) {
        self.send_command(
            worker_for_voice(slot, self.endpoints.len()),
            WorkerCommand::ResetVoice(slot),
        );
    }
    pub(crate) fn configure(&mut self, program: &mut RuntimeProgram, retired: &mut RuntimeProgram) {
        self.ready.store(0, Ordering::Release);
        for index in 0..self.endpoints.len() {
            self.send_command(
                index,
                WorkerCommand::Configure(program.take_worker_instance(index)),
            );
        }
        let mut returned: [Option<Box<ProgramInstance>>; MAX_WORKERS] =
            std::array::from_fn(|_| None);
        for _ in 0..self.endpoints.len() {
            let (index, instance) = loop {
                if let Some(value) = self.configured.pop() {
                    break value;
                }
                spin_loop();
            };
            returned[index] = Some(instance);
        }
        for (index, instance) in returned.into_iter().enumerate() {
            if let Some(instance) = instance {
                retired.restore_worker_instance(index, instance);
            }
        }
    }
    fn send_command(&self, index: usize, mut command: WorkerCommand) {
        let endpoint = &self.endpoints[index];
        loop {
            match endpoint.commands.push(command) {
                Ok(()) => {
                    endpoint.thread.unpark();
                    return;
                }
                Err(returned) => {
                    command = returned;
                    spin_loop();
                }
            }
        }
    }

    pub(crate) fn evaluate_block(
        &self,
        specs: &[VoiceBlockSpec],
        ppq_step: f32,
        mode: NoteOutputMode,
        left: &mut [f32],
        right: &mut [f32],
        results: &mut [VoiceBlockResult; MAX_VOICES],
    ) {
        assert_eq!(left.len(), right.len());
        let frame_count = left.len();
        debug_assert!(frame_count <= MAX_BLOCK_FRAMES);
        for worker_index in 0..self.endpoints.len() {
            let endpoint = &self.endpoints[worker_index];
            let mut packet = loop {
                if let Some(packet) = endpoint.free.pop() {
                    break packet;
                }
                spin_loop();
            };
            packet.frame_count = frame_count;
            packet.ppq_step = ppq_step;
            packet.output_mode = mode;
            packet.voice_count = 0;
            for spec in specs.iter().copied().filter(|spec| {
                worker_for_voice(spec.voice_slot, self.endpoints.len()) == worker_index
            }) {
                packet.voices[packet.voice_count] = spec;
                packet.voice_count += 1;
            }
            endpoint.jobs.push(packet).ok().unwrap();
            endpoint.thread.unpark();
        }
        left.fill(0.0);
        right.fill(0.0);
        let mut packets: [Option<Box<WorkerPacket>>; MAX_WORKERS] = std::array::from_fn(|_| None);
        for _ in 0..self.endpoints.len() {
            let (worker_index, packet) = loop {
                if let Some(value) = self.completed.pop() {
                    break value;
                }
                spin_loop();
            };
            debug_assert!(packets[worker_index].is_none());
            packets[worker_index] = Some(packet);
        }
        // Completion order is intentionally not observable. A fixed reduction
        // order also makes floating-point output deterministic between runs.
        for (worker_index, packet) in packets[..self.endpoints.len()].iter_mut().enumerate() {
            let packet = packet.take().expect("every worker completes once");
            for frame in 0..frame_count {
                left[frame] += packet.mix_l[frame];
                right[frame] += packet.mix_r[frame];
            }
            for spec in &packet.voices[..packet.voice_count] {
                results[spec.voice_slot] = packet.results[spec.voice_slot];
            }
            self.endpoints[worker_index].free.push(packet).ok().unwrap();
        }
    }
}

fn worker_for_voice(voice_slot: usize, worker_count: usize) -> usize {
    debug_assert!(worker_count > 0);
    voice_slot % worker_count
}

fn process_packet(runtime: &mut ProgramInstance, packet: &mut WorkerPacket) {
    let frames = packet.frame_count;
    packet.mix_l[..frames].fill(0.0);
    packet.mix_r[..frames].fill(0.0);
    for spec in packet.voices[..packet.voice_count].iter().copied() {
        let mut rng = spec.rng;
        let mut t = spec.input.t;
        let mut l = spec.input.l;
        let mut ppq = spec.input.ppq;
        let dt = 1.0 / runtime.sample_rate();
        for frame in 0..frames {
            let mut input = spec.input;
            input.t = t;
            input.l = l;
            input.ppq = ppq;
            rng = next_rng(rng);
            input.rand = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
            packet.inputs[frame] = input;
            t += dt;
            if spec.released {
                l += dt;
            }
            ppq += packet.ppq_step;
        }
        runtime.evaluate_note_block(
            &packet.inputs[..frames],
            &mut packet.outputs[..frames],
            spec.voice_slot,
        );
        let mut rendered = frames;
        let mut became_inactive = false;
        let mut result_t = spec.input.t;
        let mut result_l = spec.input.l;
        let mut result_rng = spec.rng;
        for frame in 0..frames {
            let output = packet.outputs[frame];
            if frame >= rendered {
                break;
            }
            match packet.output_mode {
                NoteOutputMode::Mono => {
                    let pan = output.pan.clamp(-1.0, 1.0);
                    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                    packet.mix_l[frame] += output.wave * angle.cos();
                    packet.mix_r[frame] += output.wave * angle.sin();
                }
                NoteOutputMode::Stereo => {
                    packet.mix_l[frame] += output.wave_l;
                    packet.mix_r[frame] += output.wave_r;
                }
            }
            result_t += dt;
            if spec.released {
                result_l += dt;
            }
            result_rng = next_rng(result_rng);
            if spec.released && result_l >= output.l_limit.max(0.0) {
                rendered = frame + 1;
                became_inactive = true;
            }
        }
        packet.results[spec.voice_slot] = VoiceBlockResult {
            rendered_frames: rendered,
            t: result_t,
            l: result_l,
            rng: result_rng,
            became_inactive,
        };
    }
}

impl Drop for VoiceWorkerPool {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        for endpoint in &self.endpoints {
            let _ = endpoint.commands.push(WorkerCommand::Shutdown);
            endpoint.thread.unpark();
        }
        for endpoint in &mut self.endpoints {
            if let Some(handle) = endpoint.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::worker_for_voice;

    #[test]
    fn affinity_stripes_voice_slots_across_workers() {
        let actual = (0..12)
            .map(|slot| worker_for_voice(slot, 4))
            .collect::<Vec<_>>();
        assert_eq!(actual, [0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3]);
    }
}
