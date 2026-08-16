//! Prepared persistent storage for one audio program instance.
//!
//! Allocation and migration happen off the audio thread. JIT helpers only dereference
//! fixed pointers and mutate the slot owned by one voice worker or the main filter.

use crate::dsp::{DspKind, DspProcessor};
use std::{
    cell::UnsafeCell,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

pub const RUNTIME_STATE_SLOTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StorageDomain {
    Voice,
    Global,
}

impl StorageDomain {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "voice" => Self::Voice,
            "global" => Self::Global,
            _ => return None,
        })
    }

    fn cell_count(self) -> usize {
        match self {
            Self::Voice => RUNTIME_STATE_SLOTS,
            Self::Global => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RingCapacity {
    Samples(usize),
    Seconds(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StorageKind {
    Scalar { initial: f32 },
    Ring { capacity: RingCapacity },
    Dsp { kind: DspKind },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StorageSpec {
    pub key: String,
    pub source_name: String,
    pub domain: StorageDomain,
    pub kind: StorageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedKind {
    Scalar,
    Ring { capacity: usize },
    Dsp { kind: DspKind },
}

struct RingState {
    values: Box<[f32]>,
    cursor: usize,
    pending: f32,
    touched: bool,
    written: bool,
}

impl RingState {
    fn new(capacity: usize) -> Result<Self, String> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(capacity)
            .map_err(|_| format!("RingBuf用メモリを確保できません（{capacity} samples）"))?;
        values.resize(capacity, 0.0);
        Ok(Self {
            values: values.into_boxed_slice(),
            cursor: 0,
            pending: 0.0,
            touched: false,
            written: false,
        })
    }

    #[inline]
    fn read(&mut self) -> f32 {
        self.touched = true;
        self.values[self.cursor]
    }

    #[inline]
    fn peek(&mut self, delay_seconds: f32, sample_rate: f32, linear: bool) -> f32 {
        self.touched = true;
        let delay = (delay_seconds.max(0.0) * sample_rate).clamp(1.0, self.values.len() as f32);
        let whole = delay.floor() as usize;
        let fraction = delay - whole as f32;
        let index_a =
            (self.cursor + self.values.len() - whole % self.values.len()) % self.values.len();
        if !linear {
            return self.values[index_a];
        }
        let index_b = (index_a + self.values.len() - 1) % self.values.len();
        self.values[index_a] + (self.values[index_b] - self.values[index_a]) * fraction
    }

    #[inline]
    fn write(&mut self, value: f32) {
        self.touched = true;
        self.written = true;
        self.pending = value;
    }

    #[inline]
    fn commit(&mut self) {
        if !self.touched {
            return;
        }
        self.values[self.cursor] = if self.written { self.pending } else { 0.0 };
        self.cursor += 1;
        if self.cursor == self.values.len() {
            self.cursor = 0;
        }
        self.pending = 0.0;
        self.touched = false;
        self.written = false;
    }

    fn reset(&mut self) {
        self.values.fill(0.0);
        self.cursor = 0;
        self.pending = 0.0;
        self.touched = false;
        self.written = false;
    }
}

enum StateStorage {
    Scalar {
        initial: f32,
        cells: Box<[UnsafeCell<f32>]>,
    },
    GlobalScalar {
        initial: f32,
        value: AtomicU32,
    },
    Ring {
        cells: Box<[UnsafeCell<RingState>]>,
    },
    Dsp {
        cells: Box<[UnsafeCell<DspProcessor>]>,
    },
}

struct StateNode {
    domain: StorageDomain,
    resolved: ResolvedKind,
    sample_rate: f32,
    storage: StateStorage,
}

// SAFETY: Voice cellは固定affinityの担当workerだけ、Global cellはfilterを
// 実行するmain audio ownerだけがwriteします。同じcellへの同時accessはありません。
// StateMigrationHandleはArcを保持するだけで、cellへアクセスするAPIを公開しません。
unsafe impl Send for StateNode {}
unsafe impl Sync for StateNode {}

impl StateNode {
    fn new(spec: &StorageSpec, sample_rate: f32) -> Result<Self, String> {
        let count = spec.domain.cell_count();
        let (resolved, storage) = match spec.kind {
            StorageKind::Scalar { initial } if spec.domain == StorageDomain::Global => (
                ResolvedKind::Scalar,
                StateStorage::GlobalScalar {
                    initial,
                    value: AtomicU32::new(initial.to_bits()),
                },
            ),
            StorageKind::Scalar { initial } => {
                let cells = (0..count)
                    .map(|_| UnsafeCell::new(initial))
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                (
                    ResolvedKind::Scalar,
                    StateStorage::Scalar { initial, cells },
                )
            }
            StorageKind::Ring { capacity } => {
                let capacity = resolve_capacity(capacity, sample_rate)?;
                let mut cells = Vec::new();
                cells
                    .try_reserve_exact(count)
                    .map_err(|_| "RingBuf descriptorを確保できません".to_owned())?;
                for _ in 0..count {
                    cells.push(UnsafeCell::new(RingState::new(capacity)?));
                }
                (
                    ResolvedKind::Ring { capacity },
                    StateStorage::Ring {
                        cells: cells.into_boxed_slice(),
                    },
                )
            }
            StorageKind::Dsp { kind } => {
                let mut cells = Vec::new();
                cells
                    .try_reserve_exact(count)
                    .map_err(|_| "標準DSP descriptorを確保できません".to_owned())?;
                for _ in 0..count {
                    cells.push(UnsafeCell::new(DspProcessor::new(kind, sample_rate)?));
                }
                (
                    ResolvedKind::Dsp { kind },
                    StateStorage::Dsp {
                        cells: cells.into_boxed_slice(),
                    },
                )
            }
        };
        Ok(Self {
            domain: spec.domain,
            resolved,
            sample_rate,
            storage,
        })
    }

    #[inline]
    fn slot(&self, voice_slot: usize) -> usize {
        match self.domain {
            StorageDomain::Voice => voice_slot.min(RUNTIME_STATE_SLOTS - 1),
            StorageDomain::Global => 0,
        }
    }

    #[inline]
    unsafe fn read(&self, voice_slot: usize) -> f32 {
        let slot = self.slot(voice_slot);
        match &self.storage {
            StateStorage::Scalar { cells, .. } => {
                // SAFETY: the current execution owner exclusively owns this cell.
                unsafe { *cells[slot].get() }
            }
            StateStorage::GlobalScalar { value, .. } => {
                f32::from_bits(value.load(Ordering::Relaxed))
            }
            StateStorage::Ring { cells } => {
                // SAFETY: the current execution owner exclusively owns this cell.
                unsafe { (&mut *cells[slot].get()).read() }
            }
            StateStorage::Dsp { .. } => 0.0,
        }
    }

    #[inline]
    unsafe fn write(&self, voice_slot: usize, value: f32) {
        let slot = self.slot(voice_slot);
        match &self.storage {
            StateStorage::Scalar { cells, .. } => {
                // SAFETY: the current execution owner exclusively owns this cell.
                unsafe { *cells[slot].get() = value };
            }
            StateStorage::GlobalScalar { value: cell, .. } => {
                cell.store(value.to_bits(), Ordering::Relaxed);
            }
            StateStorage::Ring { cells } => {
                // SAFETY: the current execution owner exclusively owns this cell.
                unsafe { (&mut *cells[slot].get()).write(value) };
            }
            StateStorage::Dsp { .. } => {}
        }
    }

    unsafe fn commit(&self, slot: usize) {
        if let StateStorage::Ring { cells } = &self.storage {
            // SAFETY: the selected slot has one execution owner.
            unsafe { (&mut *cells[slot].get()).commit() };
        }
    }

    unsafe fn reset_slot(&self, slot: usize) {
        match &self.storage {
            StateStorage::Scalar { initial, cells } => {
                // SAFETY: reset runs only at a completed block/lifecycle boundary.
                unsafe { *cells[slot].get() = *initial };
            }
            StateStorage::GlobalScalar { initial, value } => {
                value.store(initial.to_bits(), Ordering::Relaxed);
            }
            StateStorage::Ring { cells } => {
                // SAFETY: reset runs only at a completed block/lifecycle boundary.
                unsafe { (&mut *cells[slot].get()).reset() };
            }
            StateStorage::Dsp { cells } => {
                // SAFETY: reset runs only at a completed block/lifecycle boundary.
                unsafe { (&mut *cells[slot].get()).reset() };
            }
        }
    }

    #[inline]
    unsafe fn ring_peek(&self, voice_slot: usize, delay_seconds: f32, linear: bool) -> f32 {
        let slot = self.slot(voice_slot);
        match &self.storage {
            StateStorage::Ring { cells } => {
                // SAFETY: the current execution owner exclusively owns this cell.
                unsafe { (&mut *cells[slot].get()).peek(delay_seconds, self.sample_rate, linear) }
            }
            _ => 0.0,
        }
    }

    fn ring_len(&self) -> f32 {
        match self.resolved {
            ResolvedKind::Ring { capacity } => capacity as f32,
            _ => 0.0,
        }
    }

    fn ring_duration(&self) -> f32 {
        self.ring_len() / self.sample_rate
    }

    #[inline]
    unsafe fn dsp_process(&self, voice_slot: usize, arguments: [f32; 5]) -> f32 {
        let slot = self.slot(voice_slot);
        match &self.storage {
            StateStorage::Dsp { cells } => {
                // SAFETY: the current execution owner exclusively owns this cell.
                unsafe { (&mut *cells[slot].get()).process(arguments) }
            }
            _ => 0.0,
        }
    }
}

fn resolve_capacity(capacity: RingCapacity, sample_rate: f32) -> Result<usize, String> {
    let samples = match capacity {
        RingCapacity::Samples(samples) => samples,
        RingCapacity::Seconds(seconds) => {
            let samples = f64::from(seconds) * f64::from(sample_rate);
            if !samples.is_finite() || samples > usize::MAX as f64 {
                return Err("RingBuf容量が実行環境のsize上限を超えています".to_owned());
            }
            samples.round().max(1.0) as usize
        }
    };
    if samples == 0 {
        Err("RingBuf容量は1以上である必要があります".to_owned())
    } else {
        Ok(samples)
    }
}

#[derive(Clone)]
struct MigrationRecord {
    key: String,
    domain: StorageDomain,
    resolved: ResolvedKind,
    node: Arc<StateNode>,
}

/// Hot reload時に完全一致するstorage backingだけを引き継ぐためのopaque handleです。
#[derive(Clone)]
pub struct StateMigrationHandle {
    sample_rate_bits: u32,
    records: Arc<[MigrationRecord]>,
}

impl std::fmt::Debug for StateMigrationHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StateMigrationHandle")
            .field("storage_count", &self.records.len())
            .finish()
    }
}

pub(crate) struct RuntimeState {
    sample_rate_bits: u32,
    specs: Arc<[StorageSpec]>,
    nodes: Vec<Arc<StateNode>>,
    pointers: Box<[*const StateNode]>,
    voice_rings: Box<[usize]>,
    global_rings: Box<[usize]>,
}

// SAFETY: raw pointers only target the Arc-owned nodes in the same RuntimeState.
// Moving the prepared instance to the audio thread does not invalidate them.
unsafe impl Send for RuntimeState {}

impl std::fmt::Debug for RuntimeState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeState")
            .field("storage_count", &self.nodes.len())
            .finish()
    }
}

impl RuntimeState {
    pub(crate) fn prepare(
        specs: Arc<[StorageSpec]>,
        sample_rate: f32,
        previous: Option<&StateMigrationHandle>,
    ) -> Result<Self, String> {
        Self::prepare_with(specs, sample_rate, previous, false)
    }

    pub(crate) fn prepare_worker(
        specs: Arc<[StorageSpec]>,
        sample_rate: f32,
        previous: Option<&StateMigrationHandle>,
    ) -> Result<Self, String> {
        // note() is restricted to voice state, so workers never access global
        // processors. Sharing their backing with the main filter runtime avoids
        // allocating unused per-worker global RingBuf/DSP shards.
        Self::prepare_with(specs, sample_rate, previous, false)
    }

    fn prepare_with(
        specs: Arc<[StorageSpec]>,
        sample_rate: f32,
        previous: Option<&StateMigrationHandle>,
        shard_global_processors: bool,
    ) -> Result<Self, String> {
        let sample_rate_bits = sample_rate.to_bits();
        let previous = previous.filter(|value| value.sample_rate_bits == sample_rate_bits);
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(specs.len())
            .map_err(|_| "storage descriptorを確保できません".to_owned())?;
        for spec in specs.iter() {
            let resolved = match spec.kind {
                StorageKind::Scalar { .. } => ResolvedKind::Scalar,
                StorageKind::Ring { capacity } => ResolvedKind::Ring {
                    capacity: resolve_capacity(capacity, sample_rate)?,
                },
                StorageKind::Dsp { kind } => ResolvedKind::Dsp { kind },
            };
            let reused = previous.and_then(|previous| {
                previous
                    .records
                    .iter()
                    .find(|record| {
                        record.key == spec.key
                            && record.domain == spec.domain
                            && record.resolved == resolved
                            && (!shard_global_processors
                                || record.domain != StorageDomain::Global
                                || matches!(record.resolved, ResolvedKind::Scalar))
                    })
                    .map(|record| record.node.clone())
            });
            nodes.push(match reused {
                Some(node) => node,
                None => Arc::new(StateNode::new(spec, sample_rate)?),
            });
        }
        let pointers = nodes
            .iter()
            .map(Arc::as_ptr)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let ring_indices = |domain| {
            specs
                .iter()
                .enumerate()
                .filter_map(|(index, spec)| {
                    (spec.domain == domain && matches!(spec.kind, StorageKind::Ring { .. }))
                        .then_some(index)
                })
                .collect::<Vec<_>>()
                .into_boxed_slice()
        };
        let voice_rings = ring_indices(StorageDomain::Voice);
        let global_rings = ring_indices(StorageDomain::Global);
        Ok(Self {
            sample_rate_bits,
            specs,
            nodes,
            pointers,
            voice_rings,
            global_rings,
        })
    }

    pub(crate) fn migration_handle(&self) -> StateMigrationHandle {
        let records = self
            .specs
            .iter()
            .zip(&self.nodes)
            .map(|(spec, node)| MigrationRecord {
                key: spec.key.clone(),
                domain: spec.domain,
                resolved: node.resolved,
                node: node.clone(),
            })
            .collect::<Vec<_>>();
        StateMigrationHandle {
            sample_rate_bits: self.sample_rate_bits,
            records: records.into(),
        }
    }

    #[inline]
    pub(crate) fn context(&mut self, voice_slot: usize) -> EvalContext {
        EvalContext {
            nodes: self.pointers.as_ptr(),
            node_count: self.pointers.len(),
            voice_slot,
        }
    }

    #[inline]
    pub(crate) fn commit_voice(&mut self, voice_slot: usize) {
        self.commit_indices(StorageDomain::Voice, voice_slot, &self.voice_rings);
    }

    #[inline]
    pub(crate) fn commit_global(&mut self) {
        self.commit_indices(StorageDomain::Global, 0, &self.global_rings);
    }

    fn commit_indices(&self, domain: StorageDomain, slot: usize, indices: &[usize]) {
        debug_assert!(slot < domain.cell_count());
        for &index in indices {
            // SAFETY: this domain slot has one execution owner for the block.
            unsafe { self.nodes[index].commit(slot) };
        }
    }

    pub(crate) fn reset_voice(&mut self, slot: usize) {
        self.reset_domain_slot(StorageDomain::Voice, slot);
    }

    pub(crate) fn reset_all(&mut self) {
        for node in &self.nodes {
            for slot in 0..node.domain.cell_count() {
                // SAFETY: reset is invoked without concurrent evaluation.
                unsafe { node.reset_slot(slot) };
            }
        }
    }

    fn reset_domain_slot(&self, domain: StorageDomain, slot: usize) {
        if slot >= domain.cell_count() {
            return;
        }
        for node in &self.nodes {
            if node.domain == domain {
                // SAFETY: lifecycle handling owns the selected slot after worker completion.
                unsafe { node.reset_slot(slot) };
            }
        }
    }
}

#[repr(C)]
pub(crate) struct EvalContext {
    nodes: *const *const StateNode,
    node_count: usize,
    voice_slot: usize,
}

#[inline]
pub(crate) extern "C" fn jit_state_read(context: *mut EvalContext, index: u32) -> f32 {
    if context.is_null() {
        return 0.0;
    }
    // SAFETY: JIT passes the EvalContext created by RuntimeState for this call.
    let context = unsafe { &mut *context };
    let index = index as usize;
    if index >= context.node_count {
        return 0.0;
    }
    // SAFETY: pointers refer to Arc-owned nodes for the entire native call.
    let node = unsafe { &**context.nodes.add(index) };
    // SAFETY: the current worker/filter owner exclusively owns this domain cell.
    unsafe { node.read(context.voice_slot) }
}

#[inline]
pub(crate) extern "C" fn jit_state_write(context: *mut EvalContext, index: u32, value: f32) {
    if context.is_null() {
        return;
    }
    // SAFETY: JIT passes the EvalContext created by RuntimeState for this call.
    let context = unsafe { &mut *context };
    let index = index as usize;
    if index >= context.node_count {
        return;
    }
    // SAFETY: pointers refer to Arc-owned nodes for the entire native call.
    let node = unsafe { &**context.nodes.add(index) };
    // SAFETY: the current worker/filter owner exclusively owns this domain cell.
    unsafe { node.write(context.voice_slot, value) };
}

#[inline]
pub(crate) extern "C" fn jit_commit_voice(context: *mut EvalContext) {
    if context.is_null() {
        return;
    }
    let context = unsafe { &*context };
    for index in 0..context.node_count {
        let node = unsafe { &**context.nodes.add(index) };
        if node.domain == StorageDomain::Voice {
            unsafe { node.commit(context.voice_slot) };
        }
    }
}

#[inline]
pub(crate) extern "C" fn jit_commit_global(context: *mut EvalContext) {
    if context.is_null() {
        return;
    }
    let context = unsafe { &*context };
    for index in 0..context.node_count {
        let node = unsafe { &**context.nodes.add(index) };
        if node.domain == StorageDomain::Global {
            unsafe { node.commit(0) };
        }
    }
}

#[inline]
pub(crate) extern "C" fn jit_ring_peek(
    context: *mut EvalContext,
    index: u32,
    delay_seconds: f32,
    linear: u32,
) -> f32 {
    if context.is_null() {
        return 0.0;
    }
    // SAFETY: JIT passes the EvalContext created by RuntimeState for this call.
    let context = unsafe { &mut *context };
    let index = index as usize;
    if index >= context.node_count {
        return 0.0;
    }
    // SAFETY: pointers refer to Arc-owned nodes for the entire native call.
    let node = unsafe { &**context.nodes.add(index) };
    // SAFETY: the current worker/filter owner exclusively owns this domain cell.
    unsafe { node.ring_peek(context.voice_slot, delay_seconds, linear != 0) }
}

#[inline]
pub(crate) extern "C" fn jit_ring_len(context: *mut EvalContext, index: u32) -> f32 {
    if context.is_null() {
        return 0.0;
    }
    // SAFETY: JIT passes the EvalContext created by RuntimeState for this call.
    let context = unsafe { &mut *context };
    let index = index as usize;
    if index >= context.node_count {
        return 0.0;
    }
    // SAFETY: pointers refer to Arc-owned nodes for the entire native call.
    unsafe { (&**context.nodes.add(index)).ring_len() }
}

#[inline]
pub(crate) extern "C" fn jit_ring_duration(context: *mut EvalContext, index: u32) -> f32 {
    if context.is_null() {
        return 0.0;
    }
    // SAFETY: JIT passes the EvalContext created by RuntimeState for this call.
    let context = unsafe { &mut *context };
    let index = index as usize;
    if index >= context.node_count {
        return 0.0;
    }
    // SAFETY: pointers refer to Arc-owned nodes for the entire native call.
    unsafe { (&**context.nodes.add(index)).ring_duration() }
}

#[inline]
pub(crate) extern "C" fn jit_dsp_process(
    context: *mut EvalContext,
    index: u32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
) -> f32 {
    if context.is_null() {
        return 0.0;
    }
    // SAFETY: JIT passes the EvalContext created by RuntimeState for this call.
    let context = unsafe { &mut *context };
    let index = index as usize;
    if index >= context.node_count {
        return 0.0;
    }
    // SAFETY: pointers refer to Arc-owned nodes for the entire native call.
    let node = unsafe { &**context.nodes.add(index) };
    // SAFETY: the current worker/filter owner exclusively owns this domain cell.
    unsafe { node.dsp_process(context.voice_slot, [a, b, c, d, e]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring_spec(domain: StorageDomain) -> Arc<[StorageSpec]> {
        vec![StorageSpec {
            key: "test::delay".into(),
            source_name: "delay".into(),
            domain,
            kind: StorageKind::Ring {
                capacity: RingCapacity::Samples(2),
            },
        }]
        .into()
    }

    #[test]
    fn ring_reads_old_front_and_commits_last_write() {
        let mut state =
            RuntimeState::prepare(ring_spec(StorageDomain::Global), 48_000.0, None).unwrap();
        let mut context = state.context(0);
        assert_eq!(jit_state_read(&mut context, 0), 0.0);
        jit_state_write(&mut context, 0, 1.0);
        jit_state_write(&mut context, 0, 2.0);
        assert_eq!(jit_state_read(&mut context, 0), 0.0);
        state.commit_global();
        let mut context = state.context(0);
        assert_eq!(jit_state_read(&mut context, 0), 0.0);
        state.commit_global();
        let mut context = state.context(0);
        assert_eq!(jit_state_read(&mut context, 0), 2.0);
    }

    #[test]
    fn exact_hot_reload_reuses_storage() {
        let specs = ring_spec(StorageDomain::Global);
        let mut old = RuntimeState::prepare(specs.clone(), 48_000.0, None).unwrap();
        let mut context = old.context(0);
        jit_state_write(&mut context, 0, 7.0);
        old.commit_global();
        let mut context = old.context(0);
        assert_eq!(jit_state_read(&mut context, 0), 0.0);
        old.commit_global();
        let handle = old.migration_handle();
        let mut next = RuntimeState::prepare(specs, 48_000.0, Some(&handle)).unwrap();
        let mut context = next.context(0);
        assert_eq!(jit_state_read(&mut context, 0), 7.0);
    }
}
