# Worker Evaluation Design

## Semantics

`note`のpersistent scalar、RingBuf、stateful DSPはすべてVoice slot domainです。`note` call treeから`global`または廃止済みの`note` storageへはアクセスできません。`filter` call treeは逆に`global` storageだけを扱います。この制約により、異なるVoice slotの評価間に共有mutable stateはありません。

各Voiceは`voice_slot % worker_count`で常に同じworkerへ割り当てます。同じ `(channel, note)` を重ねて発音してもpersistent stateは共有せず、それぞれのVoice slotが独立したstateを持ちます。Voice内部のsample順、RingBuf transaction、release/steal/reset順は維持しますが、Voice間の評価順はobservableではありません。

## Runtime status

VST processはMIDI sample offsetでevent-free segmentへ分割し、長いsegmentだけを内部最大block長でchunk化します。各chunkにつき各workerへjobを1回送り、workerは担当active voiceをvoice-major順に`evaluate_note_block()`で処理してworker-local stereo mixへ加算します。completionもworkerごとに1回だけです。

main threadはcompletionを任意順で回収した後、worker番号順にnote mixをreduceし、hostのstereo audio inputを加算します。この `mix + input` をeffect layoutへ変換してからだけ`evaluate_filter_block()`を実行し、global stateをsample順にcommitして出力します。worker、queue、packet、Inputs/Outputs scratch、mix bufferは起動時またはpublish時に固定容量で準備され、audio callback中のheap allocation、thread spawn、mutex、blocking waitはありません。`render_sample()`もblock size 1の互換wrapperです。
