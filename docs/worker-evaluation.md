# Worker Evaluation Design

## Semantics

The compiler marks a `note` entry as worker-compatible when it only accesses Voice-domain state and relaxed Global scalar state. Global scalar reads and writes use `AtomicU32` with relaxed ordering; concurrent writes are intentionally last-writer-wins.

Global RingBuf and stateful DSP accessed from `note` are recreated as worker-local relaxed shards. Their feedback is therefore independent per worker rather than one globally ordered feedback path. `note` domain state remains serial because the same MIDI note may own multiple Voice slots.

## Runtime status

1 sampleごとにjob/resultを同期する初期dispatcherは、queueとthread wakeの固定費がJIT評価を上回ったため無効化しています。現在は直列JIT評価です。UIのCompile statusも実際の状態を`Serial`と表示します。

次のworker実装はsample単位ではなく、MIDI event間のblockをまとめて評価する構成にします。audio threadでのallocationとmutexを避け、Global post-mix `filter`はmix後に直列評価します。
