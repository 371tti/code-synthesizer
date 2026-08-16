//! Compiled program ownership, migration, and lock-free hot reload exchange.

use crossbeam_queue::ArrayQueue;
use std::{
    hint::spin_loop,
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};
use synth_dsl::{Program, ProgramInstance, StateMigrationHandle};
/// Craneliftでネイティブコード化されたプログラムです。
pub(crate) struct RuntimeProgram {
    pub(crate) instance: Box<ProgramInstance>,
}

impl RuntimeProgram {
    pub(crate) fn from_program(program: Program, sample_rate: f32) -> Self {
        let instance = program
            .instantiate(sample_rate, None)
            .expect("compiled program state must prepare");
        Self::from_instance(instance)
    }
    fn from_instance(instance: ProgramInstance) -> Self {
        Self {
            instance: Box::new(instance),
        }
    }
}

/// Bounded lock-free handoff for compiled programs.
pub struct ProgramExchange {
    pending: ArrayQueue<Box<RuntimeProgram>>,
    retired: ArrayQueue<Box<RuntimeProgram>>,
    migration: Mutex<Option<StateMigrationHandle>>,
    sample_rate: AtomicU32,
}

impl ProgramExchange {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(2);
        Self {
            pending: ArrayQueue::new(capacity),
            retired: ArrayQueue::new(capacity),
            migration: Mutex::new(None),
            sample_rate: AtomicU32::new(48_000.0f32.to_bits()),
        }
    }

    /// Publishes a program from a non-real-time thread. If the editor outpaces
    /// the audio callback, stale unpublished programs are discarded here.
    pub fn publish(&self, program: Program) {
        let sample_rate = f32::from_bits(self.sample_rate.load(Ordering::Acquire));
        let previous = self
            .migration
            .lock()
            .expect("migration state poisoned")
            .clone();
        let instance = program
            .instantiate(sample_rate, previous.as_ref())
            .expect("compiled program state must prepare");
        self.publish_instance(instance);
    }

    pub fn publish_instance(&self, instance: ProgramInstance) {
        let handle = instance.migration_handle();
        *self.migration.lock().expect("migration state poisoned") = Some(handle);
        let mut program = Box::new(RuntimeProgram::from_instance(instance));
        loop {
            match self.pending.push(program) {
                Ok(()) => return,
                Err(returned) => {
                    program = returned;
                    if self.pending.pop().is_none() {
                        spin_loop();
                    }
                }
            }
        }
    }

    pub(crate) fn seed(&self, instance: &ProgramInstance) {
        self.sample_rate
            .store(instance.sample_rate().to_bits(), Ordering::Release);
        *self.migration.lock().expect("migration state poisoned") =
            Some(instance.migration_handle());
    }

    /// Drains programs retired by the audio thread. Call this periodically on
    /// the editor/background thread so deallocation never occurs in audio.
    pub fn collect_retired(&self) -> usize {
        let mut count = 0;
        while self.retired.pop().is_some() {
            count += 1;
        }
        count
    }

    pub(crate) fn swap_at_block_boundary(
        &self,
        current: &mut Box<RuntimeProgram>,
        deferred_retire: &mut Option<Box<RuntimeProgram>>,
    ) -> bool {
        if let Some(retired) = deferred_retire.take()
            && let Err(retired) = self.retired.push(retired)
        {
            *deferred_retire = Some(retired);
            return false;
        }
        let Some(next) = self.pending.pop() else {
            return false;
        };
        *deferred_retire = Some(std::mem::replace(current, next));
        true
    }
}
