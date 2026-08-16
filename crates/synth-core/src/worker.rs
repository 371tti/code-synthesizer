//! Disabled sample-granularity Voice worker implementation.

use crate::{MAX_VOICES, VOICE_WORKER_ENABLED};
use crossbeam_queue::ArrayQueue;
use std::{
    hint::spin_loop,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle, Thread},
};
use synth_dsl::{Inputs, Program, ProgramInstance, StateMigrationHandle};
pub(crate) const WORKER_THRESHOLD: usize = 8;
const MAX_WORKERS: usize = 4;

#[derive(Clone, Copy)]
pub(crate) struct VoiceJob {
    pub(crate) input: Inputs,
    pub(crate) voice_slot: usize,
    pub(crate) note_slot: usize,
}

#[derive(Clone, Copy)]
struct VoiceResult {
    pub(crate) voice_slot: usize,
    output: synth_dsl::Outputs,
}

enum WorkerCommand {
    Configure {
        generation: u64,
        program: Program,
        sample_rate: f32,
        migration: StateMigrationHandle,
    },
    Shutdown,
}

struct WorkerEndpoint {
    commands: Arc<ArrayQueue<WorkerCommand>>,
    thread: Thread,
    handle: Option<JoinHandle<()>>,
}

pub(crate) struct VoiceWorkerPool {
    jobs: Arc<ArrayQueue<VoiceJob>>,
    results: Arc<ArrayQueue<VoiceResult>>,
    ready_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    endpoints: Vec<WorkerEndpoint>,
    generation: u64,
}

impl VoiceWorkerPool {
    pub(crate) fn new(instance: &ProgramInstance) -> Option<Self> {
        if !VOICE_WORKER_ENABLED {
            return None;
        }
        let worker_count = thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .saturating_sub(1)
            .clamp(1, MAX_WORKERS);
        if worker_count == 0 {
            return None;
        }
        let jobs = Arc::new(ArrayQueue::<VoiceJob>::new(MAX_VOICES * 2));
        let results = Arc::new(ArrayQueue::<VoiceResult>::new(MAX_VOICES * 2));
        let ready_generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut endpoints = Vec::with_capacity(worker_count);
        let program = instance.program().clone();
        let sample_rate = instance.sample_rate();
        let migration = instance.migration_handle();
        for _ in 0..worker_count {
            let commands = Arc::new(ArrayQueue::new(4));
            let jobs = jobs.clone();
            let results = results.clone();
            let ready = ready_generation.clone();
            let shutdown = shutdown.clone();
            let initial_program = program.clone();
            let initial_migration = migration.clone();
            let command_receiver = commands.clone();
            let handle = thread::Builder::new()
                .name("code-synth-voice".into())
                .spawn(move || {
                    let mut instance = initial_program
                        .instantiate_worker(sample_rate, Some(&initial_migration))
                        .expect("prepared worker instance must initialize");
                    ready.fetch_add(1, Ordering::Release);
                    loop {
                        while let Some(command) = command_receiver.pop() {
                            match command {
                                WorkerCommand::Configure {
                                    generation,
                                    program,
                                    sample_rate,
                                    migration,
                                } => {
                                    instance = program
                                        .instantiate_worker(sample_rate, Some(&migration))
                                        .expect("prepared worker instance must initialize");
                                    ready.fetch_add(1, Ordering::Release);
                                    let _ = generation;
                                }
                                WorkerCommand::Shutdown => return,
                            }
                        }
                        if shutdown.load(Ordering::Acquire) {
                            return;
                        }
                        if let Some(job) = jobs.pop() {
                            let output =
                                instance.evaluate_note(&job.input, job.voice_slot, job.note_slot);
                            instance.commit_voice(job.voice_slot);
                            let result = VoiceResult {
                                voice_slot: job.voice_slot,
                                output,
                            };
                            let mut result = result;
                            loop {
                                match results.push(result) {
                                    Ok(()) => break,
                                    Err(returned) => {
                                        result = returned;
                                        thread::yield_now();
                                    }
                                }
                            }
                            continue;
                        }
                        thread::park_timeout(std::time::Duration::from_millis(1));
                    }
                })
                .expect("voice worker thread must start");
            let thread = handle.thread().clone();
            endpoints.push(WorkerEndpoint {
                commands,
                thread,
                handle: Some(handle),
            });
        }
        Some(Self {
            jobs,
            results,
            ready_generation,
            shutdown,
            endpoints,
            generation: 0,
        })
    }

    pub(crate) fn configure(&mut self, instance: &ProgramInstance) {
        self.generation = self.generation.wrapping_add(1);
        self.ready_generation.store(0, Ordering::Release);
        for endpoint in &self.endpoints {
            let mut command = WorkerCommand::Configure {
                generation: self.generation,
                program: instance.program().clone(),
                sample_rate: instance.sample_rate(),
                migration: instance.migration_handle(),
            };
            loop {
                match endpoint.commands.push(command) {
                    Ok(()) => break,
                    Err(returned) => {
                        command = returned;
                        if endpoint.commands.pop().is_none() {
                            spin_loop();
                        }
                    }
                }
            }
            endpoint.thread.unpark();
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready_generation.load(Ordering::Acquire) >= self.endpoints.len() as u64
    }

    pub(crate) fn evaluate(
        &self,
        jobs: &[VoiceJob],
        outputs: &mut [synth_dsl::Outputs; MAX_VOICES],
    ) {
        for job in jobs {
            let mut job = *job;
            loop {
                match self.jobs.push(job) {
                    Ok(()) => break,
                    Err(returned) => {
                        job = returned;
                        spin_loop();
                    }
                }
            }
        }
        for endpoint in &self.endpoints {
            endpoint.thread.unpark();
        }
        let mut received = 0;
        while received < jobs.len() {
            if let Some(result) = self.results.pop() {
                outputs[result.voice_slot] = result.output;
                received += 1;
            } else {
                spin_loop();
            }
        }
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
