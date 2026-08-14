<div align="center">
<h1 style="font-size: 50px">Code Synthesizer</h1>
<img src="images/image.png" width="80%" />
</div>

数式ベースの独自 DSL で音を定義し、編集内容を演奏中に反映できる Windows x86_64 向け Rust 製 VST3 シンセサイザーです。

README で定義していた最初の MVP は実装済みです。

- VST3 Instrument / Stereo Output
- 64 voice のポリフォニック SynthEngine
- MIDI Note On/Off、CC、Pitch Bend、Pressure、Sustain、Program Change
- MathSynth から引き継いだ Monaco / WebView2 UI と Editor / Play モード
- Lexer / Parser / Validator / Bytecode Compiler を備えた DSL
- 非 RT スレッドでのコンパイルと Audio block 境界での hot reload
- phase lock 付き live 波形、プリセット、プレビュー鍵盤
- `p_*` ユーザーパラメータ、配置可能な knob / slider / toggle、VST3 automation
- ソース、パラメータ値、Play レイアウト、画面モードを含む plugin state 保存・復元

## ギャラリー
<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px; align-items: center;">
<img src="images/image.png" />
<img src="images/image2.png" />
<img src="images/image3.png" />
<img src="images/image4.png" />
</div>

## 必要環境

- Windows x86_64
- 64-bit VST3 対応 DAW
- Rust stable（Rust 2024 edition 対応版）
- Node.js `^20.19.0` または `>=22.12.0`
- npm
- Microsoft Edge WebView2 Runtime (システムのWebViewでOk つまりLinux非対応)

このリポジトリでは Rust 1.92.0、Node.js 24.6.0、npm 11.5.1 で検証しています。

## ビルド

通常はリポジトリ直下で次だけを実行します。初回は UI 依存関係も自動で導入され、release VST3 バンドルまで生成されます。

```powershell
.\build.cmd
```

PowerShell から直接実行する場合:

```powershell
.\build.ps1                 # release bundle
.\build.ps1 -Dev            # debug bundle
.\build.ps1 -Test           # fmt + test + clippy + release bundle
.\build.ps1 -Install        # ユーザーVST3フォルダーへもコピー
.\build.ps1 -Smoke          # 5sのUIテスト
```

生成先:

```text
target/bundled/Code Synthesizer.vst3/
  Contents/
    x86_64-win/Code Synthesizer.vst3
    Resources/moduleinfo.json
```

`-Install` を使わない場合は、生成されたバンドルディレクトリを次へコピーします。

```powershell
$vst3Dir = "$env:LOCALAPPDATA\Programs\Common\VST3"
New-Item -ItemType Directory -Force $vst3Dir | Out-Null
Copy-Item -Recurse -Force "target\bundled\Code Synthesizer.vst3" $vst3Dir
```

こんなふうに。

その後、DAW で VST3 を再スキャンし、Instrument トラックへ `Code Synthesizer` を追加してください。

## Version と Release

version はルート `Cargo.toml` の `[workspace.package]` にある `version` だけを更新します。この値が全 crate、VST3 metadata、UI のタイトルと見出しに自動反映されます。

`main` または `master` へ push した際、GitHub Actions が直前の `Cargo.toml` と version を比較します。version が変わった場合だけ全テストと release buildを実行し、`v{version}` tag、GitHub Release、Windows x64 VST3 zipを生成します。通常のコード変更や、version以外の `Cargo.toml` 変更ではReleaseは作成されません。

## 使い方

プラグイン画面の Editor モードで DSL を編集すると、260 ms の debounce 後に自動コンパイルされます。成功したプログラムだけが Audio block 境界で DSP へ渡されるため、編集中に構文エラーがあっても直前の正常な音は継続します。Monaco には DSL 補完、snippet、hover、引数ヒント、コンパイラ marker、候補名の quick fix が入っています。

```text
p_attack_ms = param(8.33, 1, 200, 0.1)
p_release_s = param(1.5, 0.1, 6, 0.01)
p_gain = param(1, 0, 1.5, 0.01)

attack = min(t * 1000 / p_attack_ms, 1)
# 大きい Release S は常に長いリリースになる
release = exp(-9 * l / p_release_s)
env = attack * release

wave = p_gain * env * s * sin(TAU * freq * t)
pan = midi_pan
l_limit = p_release_s
```

`p_* = param(default, min, max, step)` を宣言すると、Play モードの鍵盤上にコントロールが自動作成され、DAW には automation 可能な VST3 parameter として公開されます。`p_` で始まる名前が必須で、`step` は省略可能です。式へ渡る値はこの範囲・stepで量子化された実数です。

Play の `Guide` では記法、意味の作り方、配置、automationを確認できます。`Arrange` 中はドラッグで移動、右下ハンドルでリサイズします。位置・大きさ・表示形式はプロジェクトに保存されます。右クリックでは reset、値の copy/paste、knob / slider / toggle の切替を行えます。

`Parameter Guide` プリセットは同じ内容をコメントだけで解説する、編集して使えるサンプルです。さらに Velocity Piano、MPE Lead、Expressive Strings、CC74 Pluck、MIDI Bass、Motion Pad を追加しています。いずれも velocity、CC1 (MW)、CC10 pan、CC74、channel/poly pressure、expressionのうち音色に適した入力を使い、engineが適用するMIDI gainを二重に掛けません。

下部の鍵盤とPCキーはプレビュー用で、通常の DAW MIDI 入力も同じ SynthEngine へ送られます。Wave は発音中に audio callback の実出力を表示し、無音時はコンパイル済み式の静的プレビューへ切り替わります。モニターは余剰サンプルから連続フレームを切り出すため、右端で波形を循環させません。(循環させてひどい目にあった)

## DSL リファレンス

代入は上から順に評価されます。`wave` は必須です。`=` のない次行は直前の式の続きとして扱われ、`#` または `//` 以降はコメントです。

ユーザーパラメータは最大32個です。範囲と既定値は実数、`step` は省略可能です。宣言順がVST parameter slotになります。演奏者が直感的に扱えるよう、時間を示す値は `p_release_s` / `p_attack_ms` のように単位を名前へ含め、値を増やしたときの音の変化を式側でも一致させることを推奨します。

ローカル変数の数や式の長さには DSL 側の固定上限はありません。多くの変数、または 1 サンプルあたり 512 個を超える演算を含むプログラムもコンパイルされますが、発音数に応じて CPU 負荷が増える可能性があるため `Warning` を表示します。`p_*` はローカル変数ではなく DAW の VST automation slot なので、ホストと確実に同期するため最大32個です?

```text
p_cutoff = param(1200, 20, 20000, 1)
p_mix = param(0.5, 0, 1, 0.01)
```

主要入力:

| 名前                                         | 内容                                  |
| -------------------------------------------- | ------------------------------------- |
| `t`                                          | Note On からの秒数                    |
| `l`                                          | Note Off からの秒数。押下中は `0`     |
| `s`                                          | Velocity `0..1`                       |
| `freq` / `note` / `ch`                       | 周波数、MIDI note、MIDI channel       |
| `bend` / `bend_st`                           | Pitch Bend `-1..1`、半音換算値        |
| `mw` / `vol` / `midi_pan` / `mexpr`          | CC 1 / 7 / 10 / 11                    |
| `sustain`                                    | CC 64 の状態                          |
| `pressure` / `poly_pressure`                 | Channel / Poly Pressure               |
| `program` / `cc(n)`                          | Program Change、任意 CC `0..127`      |
| `sr`                                         | Sample rate                           |
| `tempo` / `beat` / `bar` / `ppq` / `playing` | DAW transport                         |
| `voice` / `rand`                             | Voice index、voice ごとの乱数 `-1..1` |

出力:

| 名前      | 内容                                     |
| --------- | ---------------------------------------- |
| `wave`    | Voice のモノラル波形。必須               |
| `pan`     | Voice pan `-1..1`。省略時 `0`            |
| `l_limit` | Note Off 後に voice を終了する秒数。必須 |

定数は `TAU`、`PI`、`E`、`PHI`、演算子は `+ - * / % ^` を使用できます。

関数:

```text
sin cos tan asin acos atan atan2 sinh cosh
exp sqrt cbrt abs tanh ln log log2 log10
floor ceil round fract sign
min max pow mod clamp mix cc
saw square pulse triangle noise
```

## 構成

```text
DAW
 └─ VST3 Adapter
     ├─ Processor: Audio / Event / SynthEngine
     ├─ Controller: Parameters / Automation / State
     └─ View: WebView2 / Monaco / IPC
                    │
                    └─ DSL Compiler
                         │ lock-free queue
                         ▼
SynthEngine ─ Voice Engine ─ Compiled DSL Runtime
```

```text
crates/
  synth-core/   固定 voice、MIDI state、stereo DSP、program/parameter/waveform共有
  synth-dsl/    lexer、parser、validation、bytecode compiler/runtime
  synth-vst3/   VST3 component/controller/view/factory
  synth-ui/     WebView2、IPC、preset、waveform preview
ui/             Monaco ベース UI
presets/        DSL プリセット
xtask/          VST3 バンドル生成
```

VST3 ホストには Single Component として公開し、同じインスタンス内で Processor と Controller の状態を共有します。内部の責務と Audio/UI thread の境界は分離しています。

## RT-safe 方針

Audio callback 内では次を行いません。

- heap allocation / deallocation
- Mutex lock
- filesystem、WebView、JSON、DSL parsing
- program の破棄や重い初期化

Voice と評価スタックは固定容量です。UI thread がコンパイル済み `Program` を bounded lock-free queue へ publish し、Audio thread は block 先頭で pointer を交換します。旧 program は別 queue へ返し、UI thread 側で破棄します。`p_*` の値と oscilloscope ring buffer も atomic な固定容量ストレージで共有します。

## えとせとら

```powershell
npm run build --prefix ui
npm audit --prefix ui --audit-level=high
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask bundle --release
```

WebView2 だけを DAW 外で確認する場合:

```powershell
cargo run -p synth-ui --example ui-smoke
```

VST3 の UI lifecycle、WebView2 生成、asset 配信、Monaco 起動結果は次へ記録されます。

```text
%LOCALAPPDATA%\Code Synthesizer\ui.log
```

`CODE_SYNTH_UI_DEVTOOLS=1` を設定してから DAW を起動すると、エディター表示時に WebView2 DevTools も開きます。

## wiwiwi

式ベースの poly synth、MathSynth UI、Editor / Play、VST automation、ライブ波形、MIDI/transport、プリセット、状態保存まで実装しています。`modal`、`resonator`、`delay`、filter、history など状態付き DSP primitive と standalone Web 版は今後の拡張候補です。DSL と DSP コアは UI/VST3 から独立しているため、Web 版でも同じ compiler/runtime を再利用するつもりです。ういー
