# Code Synthesizer DSL v2 — Implementation Specification and Plan

Status: **Implemented / 検証済み**  
Source: `Code Synthesizer DSL — Core Language Specification`（2026-08-15 添付）

この文書は、添付仕様を現在のCode Synthesizerへ実装するための仕様案と作業計画である。末尾の **Decisions** にある `[D-xx]` は、元仕様だけでは実装を一意に決められない項目である。各項目から1案をチェックした後、その選択を確定仕様として実装へ進む。

## Goals

- `fn note(in, p) -> out` と省略可能な `fn filter(in, p) -> out` を正式なEntry Pointにする。
- qualified identifier、ユーザー定義関数、比較演算、時間/SI suffix、連鎖代入を追加する。
- mono / true stereo Voiceと、Voice mix後のstereo filterを同じDSLで記述できるようにする。
- `voice` / `note` / `global` の永続scalar storageを追加する。
- sample単位でtransactionalに動作する `RingBuf<f32, Size>` を追加する。
- 新しい構文もCraneliftでJITし、Audio threadでallocation、lock、parse、compile、deallocationを行わない。
- Monaco、diagnostics、preset、preview、VST3 stateを新仕様へ追従させる。

## Non-goals for the first implementation

- `f32` 以外の数値型、vector型、文字列型。
- 可変長array、動的memory allocation、ユーザー定義struct。
- `if` / loop / recursion。条件選択は比較演算と `select` でbranchlessに構成する。
- RingBuf以外の組み込みFilter、Delay、Reverbなど。これらは言語primitiveからユーザーが構成する。
- JIT済みmachine codeのplugin stateへの保存。

## Current Baseline and Gap

| Area | Current implementation | DSL v2 target |
| --- | --- | --- |
| Program structure | file全体が上から並ぶ代入列 | 複数の `fn`、`note` / `filter` Entry Point |
| Names | ASCII identifierと`p_*` | 任意段数のqualified identifier |
| Functions | 固定built-inのみ | user function、bundle形式の`in.*` / `out.*` |
| Outputs | mono `wave` + `pan` + `l_limit` | monoまたはtrue stereo、mix後filter |
| State | sample内temporaryのみ | `voice` / `note` / `global` persistent state |
| Delay memory | なし | transactional fixed-size RingBuf |
| Literals | plain `f32` | `s/ms/us`、`k/m/u/g` suffix |
| Operators | arithmetic | arithmetic + comparison + chain assignment |
| Runtime | Voiceごとに1個のJIT関数を直接実行 | `note` JIT + optional `filter` JIT + state transaction |
| UI tooling | regex中心のsingle-scope補完 | function scope、qualified name、storage、entry別補完 |

現在のstack IRはstatelessな単一Entry Pointを前提としている。v2ではfunction boundary、bundle、storage side effect、2つのEntry Pointを表現する必要があるため、既存IRへ継ぎ足すのではなく、frontendをAST → semantic HIR → Cranelift loweringの3段に分ける。

## Proposed Core Syntax

### Grammar outline

```text
program          := parameter_declaration* function_definition*

parameter_declaration
                 := qualified_name "=" "param" "("
                    const_expression "," const_expression ","
                    const_expression "," const_expression
                    ("," midi_cc)? ")"

function_definition
                 := "fn" qualified_name "(" function_parameters ")" "->" "out" block

function_parameters
                 := "in" | "in" "," "p"

block            := "{" statement* "}"

statement        := scalar_storage_declaration
                  | ring_storage_declaration
                  | assignment

scalar_storage_declaration
                 := "f32" storage_domain qualified_name "=" const_expression

ring_storage_declaration
                 := "RingBuf" "<" "f32" "," ring_size ">"
                    storage_domain qualified_name

storage_domain   := "voice" | "note" | "global"
ring_size        := positive_integer | time_literal

assignment       := qualified_name ("=" qualified_name)* "=" expression

qualified_name   := identifier ("." identifier)*
```

- `{}` がfunction scopeを決め、statement separatorはparenthesis外の改行とする。同じ行へ複数statementは書かない。
- `=`、binary operator、comma、開き括弧の直後、または次行がbinary operatorで始まる場合は改行後もexpressionを継続する。
- `=` の後の改行は許可する。連鎖代入は右結合で、右辺を1回だけ評価する。
- `p.* = param(...)` はファイル先頭へ連続して置き、最初の `fn` より後には宣言できない。
- Entry Pointの仮引数は `note(in, p)` / `filter(in, p)`、通常関数は `name(in)`、戻り値名は常に `out` に固定する。
- function定義順には依存せずforward referenceを許可する。
- call graphにcycleがあればrecursion errorにする。
- `#` commentを正式仕様とし、`//` の扱いは[D-15]で決める。

### Name model

- `a.b.c` は1個のsymbol名であり、object/member accessではない。
- local symbol tableはfunctionごとに独立する。
- `in.*` はread-only、`out.*` はwrite-onlyの予約prefixとする。
- local、storage、parameter、call result prefixの同名衝突はcompile errorにする。
- local変数の再代入可否は[D-04]で決める。
- 同じ `out.*` fieldへの複数statementからのwriteはerrorにする。連鎖代入内の各field writeは1回として扱う。

### User function I/O bundle

```text
osc.in.freq = in.freq
osc.in.t = in.t
osc = osc.detuned(osc.in)
left = osc.left
```

内部にはobjectを作らず、call時点で `osc.in.*` に属するfield mapをcalleeの `in.*` へ対応付ける。calleeの `out.*` はcall result prefix `osc.*` としてbindする。

初期実装ではuser function callを次のstatement形式に限定する。

```text
result_prefix = function.name(input_prefix)
```

user functionはscalar expressionの途中へ直接nestできない。built-in functionは従来どおりexpression内で使用できる。calleeはcompile時にEntry Pointへinline展開し、runtime call dispatchは作らない。これによりcall-site単位のstate identityとCranelift最適化を両立する。

## Type and Expression Semantics

### Initial type set

- runtime scalarは `f32` のみ。
- function input/outputは `f32` fieldのcompile-time bundle。
- `RingBuf<f32, Size>` はscalarではなくstorage primitive。
- comparison resultの型は[D-13]で決める。
- storage declarationで `f32` 以外を指定した場合は、将来予約済みとして明示的なunsupported type errorを返す。

### Precedence

高い順に次を採用する。

1. `()`、built-in call、qualified name
2. unary `+ -`
3. `^`（右結合）
4. `* / %`
5. `+ -`
6. `< <= > >=`
7. `== !=`
8. statement-level `=`（右結合）

比較演算の連鎖（`a < b < c`）は初期実装ではerrorにし、`select`等で明示させる。

### Numeric and time literals

- `s = 1`、`ms = 1e-3`、`us = 1e-6` 秒。
- `k = 1e3`、`m = 1e-3`、`u = 1e-6`、`g = 1e9`。
- suffixはcase-sensitiveなlowercase、最長一致でtokenizeする。したがって `500ms` は `500m * s` ではなく0.5秒。
- 通常expression内のtime literalは秒を表す `f32` にlowerする。
- RingBufのtime sizeはinstance準備時のsample rateで `round(seconds * sr)` に変換し、最小容量を1 sampleにする。
- suffixなしのRingBuf sizeは正の整数element数のみ許可する。
- non-finite literal、0以下のRingBuf size、容量計算overflowはcompile/prepare errorにする。

### Built-ins

既存math/oscillatorに次を追加する。

```text
step(edge, x)
smoothstep(edge0, edge1, x)
select(condition, when_true, when_false)

mtof(note)               = 440 * 2^((note - 69) / 12)
ftom(freq)               = 69 + 12 * log2(freq / 440)
dbtoa(db)                = 10^(db / 20)
atodb(amp)               = 20 * log10(abs(amp))
cent_ratio(cents)        = 2^(cents / 1200)
semitone_ratio(st)       = 2^(st / 12)
```

`step` は `x < edge` のとき0、それ以外1。`smoothstep` はclampしたHermite補間。`select` のcondition解釈は[D-13]に従う。NaN/Infinityは演算途中ではIEEE-754に従い、Entry Point出力時に現在と同様non-finiteを0へsanitizeする。

Oscillatorの `square` / `pulse` arityは[D-14]で確定する。`noise()` は`note` call treeではVoice固有PRNG、`filter` call treeではPlugin global PRNGを使用する。

## Entry Point Semantics

### `note`

ちょうど1個必要。出力modeはcompile時に確定する。

Mono mode:

```text
out.wave       # required
out.pan        # optional, default 0, clamp -1..1
out.l_limit    # required, seconds
```

True stereo mode:

```text
out.wave_l     # required
out.wave_r     # required
out.l_limit    # required, seconds
```

- `out.wave` と `out.wave_l/out.wave_r` の混在はerror。
- true stereo modeの `out.pan` はerror。左右信号をそのままmixする。
- mono modeのみengineがequal-power panを適用する。
- v2でMIDI volume/expression/panをengineが暗黙適用するかは[D-02]で決める。
- `l_limit` はnon-finiteなら0、負値はVoice retirement判定時に0として扱う。

### `filter`

0個または1個。存在する場合は次を必須にする。

```text
in.wave_l
in.wave_r
out.wave_l
out.wave_r
```

Voice固有入力を参照するとsemantic errorにする。filter未定義ならmix結果をそのまま出力する。filter出力もnon-finiteを0へsanitizeする。

`in.mw`、`in.cc(n)`等のMIDI channel選択は元仕様では一意でないため[D-12]で決める。

## Parameters

- `p.name = param(default, min, max, step, cc_link?)` をトップレベル先頭に宣言する。専用の `parameter` / `def` ブロックは設けない。
- `p` はEntry Pointへ第2引数として渡し、`p.name` で値を読む。通常関数へparameter bundleは渡さず、必要な値は `in.*` bundleへ明示的に詰める。
- `default`、`min`、`max`、`step` は必須、`cc_link` だけ省略可能。VST3 slot上限32は維持する。
- `cc_link` は0..127の整数MIDI CC番号。CC eventによる更新だけを受け入れ、同一process blockではsoftware/host parameter変更を優先する。その後のblockで新しいCC eventが来れば再び更新できる。
- parameterはruntime localではなくprogram-globalなhost bindingとして扱う。
- declaration順をVST parameter slot順とする。
- default/min/max/stepはsuffixを含むcompile-time constant expressionのみ許可する。
- parameter名、range、stepがhot reload前後で一致する場合は現在値とPerformance layoutを維持する。

## Persistent Storage

### Scalar

```text
f32 voice phase = 0
f32 note energy = 0
f32 global master.level = 1
```

- initializerはinput、parameter、storageを参照しないcompile-time constant expressionに限定する。
- scalar initializerは必須。RingBufだけは容量全体を0で初期化する。
- readは現在値、assignmentは即時write。後続statementは更新後の値を読む。
- `voice` はVoice slotごと、`note` はnote-domain slotごと、`global` はPlugin instanceごとに保持する。
- filter call treeから `voice` / `note` storageへ到達した場合はerror。
- note call treeから3 domainすべてを利用できる。
- user function内storageのinstance identityは[D-05]で決める。

### Note domain

keyは仕様どおり `(MIDI channel, MIDI note)`。domainの生成、retrigger、破棄時期は[D-06]で確定する。Audio threadでallocationしないため、最大active note-domain slot数は `MAX_VOICES` と同数を事前準備する。

### State layout

semantic phaseで各storageへ安定した `StorageKey` を付け、次を含む `StateSchema` を生成する。

```text
StorageKey
domain
kind (Scalar / RingBuf)
initializer
call-site path
source qualified name
ring capacity specification
```

scalar block、Voice slot、Note slot、Global block、RingBuf backing memoryはProgramのaudio instanceを作る非RT threadで確保する。JITは固定offsetまたはruntime descriptor経由で直接load/storeする。

DSL複雑度、storage数、RingBuf容量には言語上の固定上限を設けず、推定memoryとCPU負荷はWarningにする。ただし実memoryのsize overflowやOS allocation failureは、安全に実行できないためprepare errorとして報告する。

## RingBuf Semantics

RingBufは固定容量array、cursor、sample内pending writeを持つ。生成時は全elementを0にする。

- 同じplugin sample中のreadはすべてcommit前の同一front値を返す。
- writeはpending値を置き換え、最後のwriteだけ残す。
- write後のreadもcommit前のfront値を返す。
- transaction境界はfunction callではなくplugin sample全体。
- Voice-domain RingBufは各Voiceの`note`終了後、Note-domainは全Voice評価後、Global-domainはoptional filter終了後にcommitする。
- 複数Voiceが共有domainへwriteする場合の順序とlast-writer規則は[D-07]で確定する。
- readだけ、writeだけの場合のcursor挙動は[D-08]で確定する。

JITはread時にfrontをloadし、write時にpending slotとwrite flagを更新するだけにする。commitはdomainごとのtouched RingBufだけを固定loopで処理し、allocation/lockを行わない。

## Runtime and JIT Architecture

### Compiler pipeline

```text
Source
  -> Lexer (token + exact span)
  -> Parser (AST)
  -> Declaration collection
  -> Name / scope / call-graph resolution
  -> Type and entry validation
  -> StateSchema + typed HIR
  -> Entry specialization / user-function inline expansion
  -> Cranelift IR
  -> note function + optional filter function
```

test-only reference evaluatorをtyped HIRに対して残し、全演算・state transactionをJITとdifferential testする。

### Native ABI proposal

概念上のABIは次とする。実際には `#[repr(C)]` struct pointerを使用する。

```text
note(
  NoteInputs*,
  NoteOutputs*,
  RuntimeState*,
  voice_slot,
  note_slot
)

filter(
  FilterInputs*,
  FilterOutputs*,
  RuntimeState*
)
```

user functionはEntryへinlineされるため公開native ABIを持たない。JIT executable memoryは現在の `Arc<JitProgram>` 所有と `JITModule::free_memory()` を維持し、最後の参照が非RT threadで破棄される。

### One-sample schedule

```text
begin Global RingBuf transaction
begin active Note-domain transactions

for active Voice in deterministic order:
    begin Voice transaction
    note(...)
    mono pan conversion or true-stereo accumulation
    commit Voice RingBuf

mix complete
optional filter(...)

commit active Note-domain RingBuf
commit Global RingBuf
sanitize output
update waveform monitor
```

shared scalar storageは即時writeなので、後続Voice/filterは更新値を読む。決定的なVoice順は[D-07]で確定する。

### Sample rate

time-sized RingBufはsample rateに依存する。sample rate変更時のscalar/RingBuf継承は[D-10]で決める。いずれの場合もmemory再構築は非RT threadで行い、Audio threadはblock境界でprepared instanceを交換する。

### Hot reload

JIT codeとstate schemaを分離する。hot reload時のstate継承方針は[D-09]で決める。継承する場合は、domain、kind、`StorageKey`、RingBuf capacityが完全一致するstorage backingだけを再利用し、型や容量が変わったstorageはinitializerから再作成する。

UI waveform previewはaudio instanceのstateを共有せず、専用のPreview instanceを持つ。source、sample rate、preview noteの変更時にPreview stateを再作成し、preview sample列の中ではstateを連続させる。

## Compatibility and Migration

既存preset、保存済みproject、README例はtop-level assignment構文である。互換方針は[D-01]で確定する。

推奨するlegacy AST adapterでは、旧built-in名を `in.*` へ変換し、現在のengine暗黙処理も式へ明示的に埋め込む。

```text
legacy wave       -> out.wave = legacy.wave * in.vol * in.mexpr
legacy pan        -> out.pan = clamp(legacy.pan + in.midi_pan, -1, 1)
missing legacy pan-> out.pan = in.midi_pan
legacy l_limit    -> out.l_limit
```

これは概念表記であり、実際のadapterは旧runtimeと同じnon-finite sanitizeとpan clamp順序もHIRで再現する。これにより旧sourceの音量/panをsample単位で変えずにv2 runtimeで実行できる。factory presetはすべてnative v2へ書き換え、parameter guideにfunction、storage、RingBuf、mono/stereo/filter例を追加する。

runtime DSP stateをVST3 project stateへ含めるかは[D-11]で決める。source、parameter、layout、modeの既存state formatはversionを上げ、旧versionを引き続きloadできるようにする。

## Editor and UI Work

- Monarch tokenizerへ `fn`、domain/type keyword、`{}`、comparison、qualified name、suffixを追加。
- completionをfunction scope対応にし、Entryごとに利用可能な `in.*` を切り替える。
- user function、input/output field、local、storage、parameterをsemantic symbolとして補完する。
- signature helpへ新built-inとuser functionを追加。
- hoverへstorage domain、RingBuf容量、parameter range、function call先を表示。
- diagnosticsへduplicate function、recursive call、missing field、invalid domain、output mode conflict、RingBuf memory estimateを追加。
- quick fixへmissing `out.l_limit`、mono/stereo output skeleton、legacy-to-v2 conversionを追加。
- static waveform previewを新note/filter runtimeで生成し、stateful presetも正しく表示する。
- GuideとREADMEをv2へ更新する。

正確なscope補完のため、長期的にはJavaScript regexだけでsymbolを推測せず、Rust側の `analyze(source)` が返すfunction/symbol/span情報をUI APIから利用する。

## Implementation Phases

### Phase 0 — Decision freeze and fixtures

- Decisionsを確定する。
- 添付の完全例、mono、true stereo、function call、各storage domain、RingBuf edge caseをgolden sourceにする。
- 現行presetの音をlegacy regression fixtureとして固定する。

Exit criteria: 本文とtestsに未定義semanticが残っていない。

### Phase 1 — Lexer, AST, parser

- `lexer.rs`、`ast.rs`、`parser.rs`へsingle-file parserを分割する。
- function/block、qualified name、storage declaration、suffix、comparison、chain assignmentをparseする。
- 全node/tokenにbyte spanとline/columnを保持する。
- error recoveryを入れ、1回のanalysisで複数diagnosticを返せる形にする。

Exit criteria: 添付例がAST化され、malformed sourceのsnapshot testが通る。

### Phase 2 — Semantic HIR and pure functions

- function table、lexical scope、bundle field map、call graphを実装する。
- forward call、inline specialization、recursion rejectionを実装する。
- Entry input/output validation、parameter hoist、comparison、新built-inをtyped HIRへlowerする。
- storageを使わないv2 programのreference evaluatorを作る。

Exit criteria: stateless mono/stereo/function DSLをreference evaluatorで実行できる。

### Phase 3 — Cranelift v2 entries

- typed HIRからnote/filterの2 native functionを生成する。
- scalar arithmetic、comparison、selectをCranelift命令へlowerする。
- math/oscillator utility helperを追加する。
- JIT/reference differential testとrelease benchmarkを追加する。

Exit criteria: stateless v2がJITで旧backend以上のthroughputを保つ。

### Phase 4 — SynthEngine integration

- NoteInputs/Outputs、FilterInputs/Outputsを追加する。
- mono equal-power pan、true stereo mix、post-mix filter scheduleを実装する。
- v2 MIDI gain/pan semanticsを反映する。
- UI previewを新runtimeへ切り替える。

Exit criteria: mono、true stereo、filterがVST render pathとpreviewの両方で一致する。

### Phase 5 — Persistent scalar storage

- StateSchema、Global/Voice/Note state slotを実装する。
- state memoryを非RT threadでprepareし、ProgramExchangeでblock境界交換する。
- initializer、read/write、domain restriction、Note-domain lifecycleを実装する。
- 64 Voice、多重発音、Voice steal、sustain、all-sound-off testを追加する。

Exit criteria: Audio callback allocation/lockなしでdomain isolation testが通る。

### Phase 6 — RingBuf and time-sized memory

- fixed-capacity backing、pending write、domain commitを実装する。
- time literalからsample-rate依存容量を準備する。
- same-sample coalescing、last write、cross-Voice transactionを実装する。
- memory estimate Warningとallocation failure diagnosticを追加する。

Exit criteria: 1/2/N sample delay、feedback delay、stereo cross-delayがsample-exactに通る。

### Phase 7 — Hot reload, state, compatibility

- 選択されたstate migrationとsample-rate policyを実装する。
- legacy adapterまたは選択された互換方式を実装する。
- VST3 state formatをversionedにし、旧project load testを追加する。
- old JIT/state memoryが必ずretired queue経由で破棄されることをstress testする。

Exit criteria: 演奏中編集、preset load、project restore、sample-rate切替がRT invariantを破らない。

### Phase 8 — Monaco, presets, documentation

- tokenizer/completion/hover/signature/quick fixをv2対応する。
- factory presetとParameter Guideを移行する。
- scalar state、phase accumulator、delay、true stereo、filterを示すpresetを追加する。
- READMEのDSL referenceとarchitectureを更新する。

Exit criteria: 全factory presetがcompile/renderされ、Editorでscope-aware補完が出る。

### Phase 9 — Final QA

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- UI build/smoke test
- release VST3 bundle
- 64 Voice stress、長時間hot reload、RingBuf memory、DAW automation/state restore test
- JIT/reference differential fuzz test

Exit criteria: regressionなし、RT callbackにallocation/lock/deallocationなし、既知semantic gapなし。

## Proposed File Layout

```text
crates/synth-dsl/src/
  lib.rs
  lexer.rs
  ast.rs
  parser.rs
  diagnostic.rs
  semantic.rs
  hir.rs
  state_schema.rs
  legacy.rs             # [D-01]で採用時
  reference.rs          # test/reference evaluator
  jit/
    mod.rs
    lower.rs
    helpers.rs

crates/synth-core/src/
  lib.rs
  runtime_program.rs
  state_bank.rs
  ringbuf.rs
```

既存public APIは移行期間中adapterを置き、UI/VST3をphase単位で壊さずに切り替える。

## Acceptance Test Matrix

- Lexer: `500ms` longest match、`1m`、`1k`、scientific notation、invalid suffix。
- Parser: nested qualified name、multiline chain assignment、function/block recovery。
- Scope: 同名localのfunction分離、input read-only、output read-before-write rejection。
- Calls: forward call、missing input field、extra field warning、recursive cycle rejection。
- Entry: mono/stereo conflict、missing l_limit、filter voice-input rejection。
- Math: 全built-inのJIT/reference比較、NaN/Infinity sanitize。
- State: Voice isolation、Note sharing、Global sharing、initializer一回だけ。
- RingBuf: repeated read、multiple write、read-after-write、read-only/write-only、capacity 1、time size。
- Ordering: multi-Voice shared write、filterが同sampleのglobal scalar更新を読むこと。
- Lifecycle: retrigger、release、sustain、Voice steal、all sound off。
- Reload: unchanged/changed schema、pending program、retired executable/state memory。
- UI: scope completion、qualified completion、marker span、preview state continuity。
- Compatibility: 全旧preset、旧VST3 state、legacy outputのsample比較。
- Performance: 現在のstateless JIT benchmarkを下回らないこと、RingBuf使用時の64 Voice測定。

## Decisions

各IDで1案を選択する。`推奨`は現在のarchitectureと添付仕様から見た実装案で、まだ確定ではない。

### [D-01] Legacy source compatibility

- [ ] **A（推奨）**: functionがないsourceを自動検出し、legacy AST adapterで音を保ったままv2 runtimeへlowerする。WarningとConvert actionを出す。
- [ ] B: one-click converterだけ提供し、変換前のsourceはcompile errorにする。
- [x] C: 旧構文をサポートせず、preset/projectを一括で破壊的移行する。

### [D-02] MIDI gain and pan in native v2

- [x] **A（推奨）**: `in.vol`、`in.mexpr`、`in.midi_pan`はDSLが明示使用し、engineは暗黙に加算/乗算しない。
- [ ] B: 現在と同様、engineがvolume/expression/panを常に暗黙適用する。

### [D-03] Parameter declaration syntax and scope

- [x] D（確定）: `p_*` と専用宣言ブロックを廃止する。ファイル先頭へ `p.vol = param(default, min, max, step, cc_link?)` の形で宣言し、Entry Pointへ `p` bundleを第2引数として渡す。

### [D-04] Local reassignment

- [x] **A（推奨）**: statement順のmutable localを許可する。最初のwrite前のreadだけerror。
- [ ] B: localはsingle assignmentとし、同名への2回目の代入をerrorにする。

かつstorage domain の宣言は初期値必須とする bufferは0で初期化とする
### [D-05] Stateful user-function instances

- [ ] **A（推奨）**: storageはcall-siteごとに独立させる。同じstateful functionを2箇所から呼べば2 instanceになる。
- [x] B: function declarationごとに1 instanceとし、すべてのcall siteで共有する。

### [D-06] Note-domain lifecycle

- [x] **A（推奨）**: `(ch,note)`で共有し、最後のVoiceがretireした時点で破棄。release中の同音retriggerは旧Voiceと共有する。
- [ ] B: Note On generationごとに別domainを作り、同じ `(ch,note)` の重複発音でも共有しない。

### [D-07] Shared state ordering

- [x] **A（推奨）**: Voice slot昇順で決定的に実行。scalarは即時、RingBufはplugin sample全体でtransactional。共有writeは最後のVoice/filterが勝つ。
- [ ] B: note call treeからGlobalへのwriteを禁止する。Note-domain writeはVoice slot昇順・last writer wins、Global writeはfilterだけに制限する。

### [D-08] RingBuf read-only / write-only commit

- [x] **A（推奨）**: readまたはwriteがあればcursorを1進める。read-onlyは現在slotを0で補充、write-onlyは最古slotをpending値で置換する。
- [ ] B: 同一sampleでreadまたはwriteの片方しか行わないRingBuf利用をcompile errorにする。
- [ ] C: readしたsampleだけcursorを進め、write-onlyはcursorを進めず現在slotを書き換える。

### [D-09] Hot reload state migration

- [x] **A（推奨）**: 完全一致する `StorageKey/domain/kind/capacity` のstateだけ維持し、変更分だけresetする。
- [ ] B: scalarだけ維持し、RingBufは毎compile resetする。
- [ ] C: compile成功programへ切り替えるたびに全persistent stateをresetする。

### [D-10] Sample-rate change

- [ ] **A（推奨）**: scalarは維持し、time-sized RingBufだけ新容量でzero-resetする。
- [x] B: scalarとRingBufを含む全stateをresetする。
- [ ] C: RingBuf内容を時間軸でresampleしてtailを維持する。

### [D-11] Persistent DSP state in VST3 project state

- [x] **A（推奨）**: runtime scalar/RingBufは保存せず、source/parameter/layoutのみ保存する。load時はinitializer/zeroから開始する。
- [ ] B: Global scalarだけ保存し、Voice/Note/RingBufは保存しない。
- [ ] C: Global RingBufを含めて保存し、delay/reverb tailも復元する。

### [D-12] Channel-dependent MIDI input in `filter`

- [x] **A（推奨）**: `in.mw`、`in.cc(n)`等はMIDI Channel 1（内部index 0）をmaster channelとして読む。
- [ ] B: 最後に該当controllerを更新したchannelの値を読む。
- [ ] C: `in.midi(channel).cc(n)`等の明示channel syntaxを今回から追加し、短縮形はChannel 1とする。

### [D-13] Comparison and `select` type

- [x] **A（推奨）**: comparisonは `f32` の0/1を返す。`select` はconditionが0ならfalse側、非0ならtrue側。
- [ ] B: transient `bool` 型を追加し、`select` conditionにはboolだけを許可する。

### [D-14] `square` / `pulse` signatures

- [x] **A（推奨）**: native v2は `square(freq,t)`を50% duty、`pulse(freq,t,duty)`を可変dutyとする。legacyの3引数squareはadapterのみ維持。
- [ ] B: 現在と同様 `square(freq,t,duty)` と `pulse(freq,t,duty)` をaliasにする。
- [ ] C: `square` は2引数と3引数のoverloadを許可し、`pulse`も3引数で残す。

### [D-15] `//` comments

- [x] **A（推奨）**: 正式な `#` に加えて、既存互換として `//` も維持する。
- [ ] B: v2では `#` だけ許可する。

## Definition of Done

実装結果（2026-08-15）:

- lexer/parser、semantic HIR、Cranelift note/filter entry、全built-inを実装した。
- mutable local、qualified bundle call、forward reference、program全体のrecursion検出を実装した。
- Voice/Note/Global scalar、transactional RingBuf、sample-rate prepare、完全一致hot reload migrationを実装した。
- true stereo mix、post-mix filter、Note-domain lifecycle、deterministic Voice順をSynthEngineへ統合した。
- `p.name = param(default, min, max, step, cc_link?)`、software優先のCC link、VST/UI表示を統合した。
- 全factory preset、Parameter Guide、README、Editor tokenizer/completion/snippetをnative v2へ移行した。
- workspace test、Clippy `-D warnings`、UI production buildで検証した。

- Decisionsで選択されたsemanticがcompiler、runtime、tests、READMEで一致している。
- 添付の完全例が変更なし、または選択事項に基づく最小変更だけでcompile/renderできる。
- note mono、note true stereo、post-mix filter、3 storage domain、RingBufがJITで動作する。
- 全factory preset、旧project state、Editor toolingが選択された互換方針どおり動く。
- Audio callbackでallocation、lock、parse、JIT compile、program/state deallocationが発生しない。
- JIT/reference differential tests、workspace tests、clippy、UI build、release VST3 bundleが成功する。
