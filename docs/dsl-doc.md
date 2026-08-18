# Code Synthesizer DSL Language Reference

Code Synthesizer DSL は、リアルタイム音声処理を記述するための数式ベース DSP 言語です。

プログラムはトップレベル宣言と関数から構成され、Cranelift JIT により native code へコンパイルされます。

---

## 1. Program Structure

プログラムは次の要素から構成されます。

```text
program :=
    top_level*
```

トップレベルには以下を記述できます。

```text
top_level :=
      layout_declaration
    | parameter_declaration
    | function_declaration
```

例:

```text
note.out.layout = mono

p.gain = param(0.8, 0, 1, 0.01)

fn note(in, p) -> out {
    out.wave =
        sin(TAU * in.freq * in.t)
        * in.s
        * p.gain

    out.l_limit = 1s
}
```

プログラムには次の entrypoint の少なくとも一方が必要です。

```text
fn note(in, p) -> out
fn effect(in, p) -> out
```

制約:

```text
count(fn note)   <= 1
count(fn effect) <= 1

count(fn note) + count(fn effect) >= 1
```

したがって以下の3構成が有効です。

```text
note
```

```text
effect
```

```text
note + effect
```

---

# 2. Lexical Structure

## 2.1 Identifier

identifier は ASCII 英字または `_` で開始し、以降に ASCII 英数字または `_` を使用できます。

```text
foo
_gain
osc1
cutoff_hz
```

概念的には:

```text
identifier :=
    [A-Za-z_][A-Za-z0-9_]*
```

---

## 2.2 Qualified Name

`.` で複数の identifier を連結できます。

```text
in.freq
out.wave
p.gain

filter.svf.lp
foo.bar.value
```

```text
qualified_name :=
    identifier ("." identifier)*
```

---

## 2.3 Comments

行コメントは2形式あります。

```text
# comment
```

```text
// comment
```

コメントは改行まで続きます。

---

# 3. Numeric Literals

基本の数値型は `f32` です。

```text
0
1
3.14
.5
1e-3
2.5e4
```

単項 `+` / `-` も使用できます。

```text
-1
+0.5
```

---

## 3.1 Time Suffix

時間 suffix は秒へ変換されます。

| suffix |     倍率 |
| ------ | -----: |
| `s`    |    `1` |
| `ms`   | `1e-3` |
| `us`   | `1e-6` |

```text
2s
500ms
250us
```

したがって、

```text
500ms
```

は数値として `0.5` 秒です。

---

## 3.2 SI Suffix

| suffix |     倍率 |
| ------ | -----: |
| `k`    |  `1e3` |
| `m`    | `1e-3` |
| `u`    | `1e-6` |
| `g`    |  `1e9` |

```text
20k
5m
100u
1g
```

例:

```text
p.cutoff = param(2k, 20, 20k, 1)
```

---

# 4. Layout Declarations

Audio channel layout はトップレベルで宣言します。

## 4.1 Note Output

```text
note.out.layout = mono
```

または:

```text
note.out.layout = stereo
```

`fn note` を定義する場合に必要です。

---

## 4.2 Effect Input

```text
effect.in.layout = mono
```

または:

```text
effect.in.layout = stereo
```

---

## 4.3 Effect Output

```text
effect.out.layout = mono
```

または:

```text
effect.out.layout = stereo
```

`fn effect` を定義する場合は input / output の両方を指定します。

---

# 5. Parameters

ユーザー parameter はトップレベルで宣言します。

```text
p.name = param(default, min, max, step)
```

MIDI CC と関連付ける場合:

```text
p.name = param(default, min, max, step, cc)
```

例:

```text
p.gain = param(0.8, 0, 1, 0.01)

p.cutoff =
    param(2k, 20, 20k, 1, 74)
```

引数:

| Position | 意味               |
| -------: | ---------------- |
|        1 | default          |
|        2 | minimum          |
|        3 | maximum          |
|        4 | step             |
|        5 | optional MIDI CC |

最初の4引数は必須です。

parameter は最大32個です。

宣言順が VST3 parameter / automation slot の順序になります。

`p.*` 宣言はトップレベルであれば関数宣言の前後どちらにも配置できます。

```text
p.a = param(1, 0, 2, 0.01)

fn helper(in) -> out {
    out.value = in.x
}

p.b = param(1, 0, 2, 0.01)
```

関数内では `param()` を宣言できません。

---

# 6. Functions

構文:

```text
fn name(parameters) -> out {
    statements
}
```

例:

```text
fn oscillator(in) -> out {
    out.wave =
        sin(TAU * in.freq * in.t)
}
```

関数名には qualified name を使用できます。

```text
fn osc.basic(in) -> out {
    ...
}
```

forward reference は可能です。

```text
fn note(in, p) -> out {
    x = oscillator(in)
    out.wave = x.wave
    out.l_limit = 1s
}

fn oscillator(in) -> out {
    out.wave =
        sin(TAU * in.freq * in.t)
}
```

再帰は使用できません。

---

# 7. Entrypoints

特殊な関数名として `note` と `effect` があります。

---

## 7.1 `note`

```text
fn note(in, p) -> out {
    ...
}
```

各 Voice に対して評価されます。

複数 Voice は互いに独立して処理されます。

```text
Voice 0 ─┐
Voice 1 ─┼─ note ─→ mix
Voice 2 ─┤
...     ─┘
```

正確には各 Voice がそれぞれ `note` の独立した evaluation/state を持ち、runtime はそれらを worker 上で並列評価できます。

`note` call tree では `voice` storage のみ利用できます。

---

## 7.2 `effect`

```text
fn effect(in, p) -> out {
    ...
}
```

Voice mix 後の signal を処理します。

```text
notes → mix ─┐
             ├→ effect → output
audio input ─┘
```

`effect` call tree では `global` storage のみ利用できます。

---

# 8. Statements

function body は statement の列です。

```text
statement :=
      assignment
    | scalar_storage_declaration
    | ringbuf_storage_declaration
```

statement は上から順番に評価されます。

---

# 9. Assignment

基本構文:

```text
target = expression
```

例:

```text
x = 1
wave = sin(phase)
out.wave = wave
```

---

## 9.1 Reassignment

local は再代入できます。

```text
x = 1
x = x + 1
```

---

## 9.2 Chain Assignment

複数 target に同じ値を代入できます。

```text
out.wave_l = out.wave_r = wave
```

概念的構文:

```text
assignment :=
    qualified_name
    ("=" qualified_name)*
    "=" expression
```

---

# 10. Local Names

通常の代入で作られる名前は local です。

```text
phase = TAU * in.freq * in.t
wave = sin(phase)
```

local は現在の function evaluation 内だけ存在します。

最初の代入より前では参照できません。

```text
# invalid

y = x
x = 1
```

---

# 11. Persistent Scalar

sample 間で値を保持する scalar state を宣言できます。

構文:

```text
f32 domain name = initial_value
```

---

## 11.1 Voice Storage

```text
f32 voice phase = 0
```

`voice` state は Voice ごとに独立しています。

```text
fn note(in, p) -> out {
    f32 voice phase = 0

    phase =
        fract(phase + in.freq / in.sr)

    out.wave =
        sin(TAU * phase)

    out.l_limit = 1s
}
```

使用可能範囲:

```text
note call tree
```

のみです。

---

## 11.2 Global Storage

```text
f32 global previous = 0
```

Plugin instance 全体で1つの state を持ちます。

使用可能範囲:

```text
effect call tree
```

のみです。

---

## 11.3 Domain Rules

```text
note   → voice
effect → global
```

以下は無効です。

```text
fn note(in, p) -> out {
    f32 global x = 0
}
```

```text
fn effect(in, p) -> out {
    f32 voice x = 0
}
```

persistent scalar には初期値が必要です。

---

# 12. RingBuf

構文:

```text
RingBuf<f32, Size> domain name
```

例:

```text
RingBuf<f32, 500ms> voice delay
```

```text
RingBuf<f32, 2s> global reverb
```

domain rule は persistent scalar と同じです。

---

## 12.1 Write

RingBuf 名への代入で現在 sample の値を書き込みます。

```text
delay = input
```

---

## 12.2 Read

```text
buffer.peek(delay)
```

指定時間だけ過去の値を読みます。

```text
wet =
    delay.peek(120ms)
```

---

## 12.3 Linear Read

```text
buffer.peek_linear(delay)
```

fractional sample position を線形補間して読みます。

```text
wet =
    delay.peek_linear(123.4ms)
```

---

## 12.4 Metadata

```text
buffer.len()
```

容量を sample 数で返します。

```text
buffer.duration()
```

容量を秒で返します。

---

## 12.5 Transaction Semantics

RingBuf の read/write は sample 単位で transactional です。

```text
a = delay.peek(100ms)

delay = x

b = delay.peek(100ms)
```

同一 sample 内では `a` と `b` は同じ既存 state を参照します。

write は sample 終了時に commit されます。

同一 sample 内で複数回 write された場合、最後の write が採用されます。

---

# 13. Expressions

expression には以下を使用できます。

```text
expression :=
      numeric_literal
    | qualified_name
    | function_call
    | unary_expression
    | binary_expression
    | "(" expression ")"
```

---

# 14. Operators

## Arithmetic

```text
+  -  *  /  %  ^
```

---

## Comparison

```text
<  <=  >  >=  ==  !=
```

比較結果は boolean 型ではなく `f32` です。

```text
false = 0
true  = 1
```

したがって次のように signal として利用できます。

```text
gate =
    in.t < 100ms
```

---

## Precedence

高い順:

```text
unary + -
^

* / %

+ -

< <= > >=

== !=
```

`^` は右結合です。

```text
2 ^ 3 ^ 2
```

は:

```text
2 ^ (3 ^ 2)
```

として解釈されます。

---

# 15. Constants

組み込み定数:

```text
TAU
PI
E
PHI
```

例:

```text
wave =
    sin(TAU * in.freq * in.t)
```

---

# 16. Note Input Bundle

`fn note` の `in` には以下の field があります。

| Field              | 内容                   |
| ------------------ | -------------------- |
| `in.t`             | Note On からの経過時間 [s]  |
| `in.l`             | Note Off からの経過時間 [s] |
| `in.s`             | Velocity             |
| `in.freq`          | 周波数 [Hz]             |
| `in.note`          | MIDI note number     |
| `in.ch`            | MIDI channel         |
| `in.voice`         | Voice index          |
| `in.rand`          | Voice 固有乱数           |
| `in.bend`          | Pitch Bend `-1..1`   |
| `in.bend_st`       | Pitch Bend の半音換算     |
| `in.mw`            | CC1                  |
| `in.vol`           | CC7                  |
| `in.midi_pan`      | CC10                 |
| `in.mexpr`         | CC11                 |
| `in.sustain`       | Sustain              |
| `in.pressure`      | Channel Pressure     |
| `in.poly_pressure` | Poly Pressure        |
| `in.program`       | Program Change       |
| `in.sr`            | Sample rate          |
| `in.tempo`         | DAW tempo            |
| `in.beat`          | Beat position        |
| `in.bar`           | Bar position         |
| `in.ppq`           | PPQ position         |
| `in.playing`       | Transport state      |

任意の MIDI CC:

```text
in.cc(n)
```

---

# 17. Note Output Bundle

## Mono

```text
note.out.layout = mono
```

利用可能 output:

```text
out.wave
out.pan
out.l_limit
```

`out.pan` は optional です。

---

## Stereo

```text
note.out.layout = stereo
```

利用可能 output:

```text
out.wave_l
out.wave_r
out.l_limit
```

Stereo note では `out.pan` は使用できません。

---

## Voice Lifetime

```text
out.l_limit
```

は Note Off 後の Voice lifetime 上限を秒で指定します。

```text
out.l_limit = 3s
```

---

# 18. Effect Input Bundle

## Mono

```text
effect.in.layout = mono
```

入力:

```text
in.wave
```

---

## Stereo

```text
effect.in.layout = stereo
```

入力:

```text
in.wave_l
in.wave_r
```

Voice mix と Audio Input が effect input に渡されます。

---

# 19. Effect Output Bundle

## Mono

```text
effect.out.layout = mono
```

```text
out.wave
```

## Stereo

```text
effect.out.layout = stereo
```

```text
out.wave_l
out.wave_r
```

---

# 20. Bundles

一部の関数は scalar ではなく複数 field を持つ bundle を返します。

例:

```text
coeff =
    biquad.lowpass(
        1200,
        0.707,
        in.sr
    )

x = coeff.b0
```

bundle field は qualified name として参照します。

```text
bundle.field
```

---

# 21. Stateful Function Semantics

一部の DSP function は内部 state を持ちます。

例:

```text
filter.svf.lp(...)
delay.fixed(...)
resonator(...)
reverb.fdn(...)
```

state は call site ごとに生成されます。

```text
a = filter.svf.lp(x, 1k, 0.7, in.sr)
b = filter.svf.lp(x, 2k, 0.7, in.sr)
```

`a` と `b` は別々の filter state を持ちます。

state domain は呼び出し元 entrypoint により決まります。

```text
note call tree
    → state × Voice

effect call tree
    → state × Plugin instance
```

---

# 22. Standard Library

## Math

```text
sin(x)
cos(x)
tan(x)

asin(x)
acos(x)
atan(x)
atan2(y, x)

sinh(x)
cosh(x)

exp(x)
exp2(x)

sqrt(x)
cbrt(x)

abs(x)
tanh(x)

ln(x)
log(x)
log2(x)
log10(x)

floor(x)
ceil(x)
round(x)
fract(x)
sign(x)

min(a, b)
max(a, b)
pow(a, b)
mod(a, b)

clamp(x, min, max)
mix(a, b, t)

step(edge, x)
smoothstep(edge0, edge1, x)
select(condition, a, b)
```

---

## Conversion

```text
mtof(note)
ftom(freq)

dbtoa(db)
atodb(amplitude)

cent_ratio(cents)
semitone_ratio(semitones)
```

---

## Utility

```text
wrap(x, min, max)

hypot(x, y)

sinc(x)

hash(x)
hash2(x, y)

fold(x, min, max)

pan_l(pan)
pan_r(pan)

onepole_coeff(freq, sr)
```

---

# 23. Oscillators

```text
saw(freq, t)

square(freq, t)

pulse(freq, t, duty)

triangle(freq, t)

noise()
```

---

# 24. Biquad Coefficients

以下は coefficient bundle を返します。

```text
biquad.lowpass(freq, q, sr)
biquad.highpass(freq, q, sr)
biquad.bandpass(freq, q, sr)
biquad.notch(freq, q, sr)
biquad.allpass(freq, q, sr)

biquad.peak(freq, q, gain_db, sr)

biquad.lowshelf(freq, q, gain_db, sr)
biquad.highshelf(freq, q, gain_db, sr)
```

返り値:

```text
b0
b1
b2
a1
a2
```

係数は `a0 = 1` に正規化されています。

---

# 25. Window

```text
window.hann(x)
window.hamming(x)
window.blackman(x)
```

`x` は `0..1` に clamp されます。

---

# 26. Filters

### One-pole

```text
filter.onepole.lp(x, freq, sr)
filter.onepole.hp(x, freq, sr)
```

### State Variable Filter

```text
filter.svf.lp(x, freq, q, sr)
filter.svf.hp(x, freq, q, sr)
filter.svf.bp(x, freq, q, sr)
filter.svf.notch(x, freq, q, sr)
```

### Biquad

```text
filter.biquad.lp(x, freq, q, sr)
filter.biquad.hp(x, freq, q, sr)
filter.biquad.bp(x, freq, q, sr)
filter.biquad.notch(x, freq, q, sr)
filter.biquad.allpass(x, freq, q, sr)

filter.biquad.peak(
    x,
    freq,
    q,
    gain_db,
    sr
)

filter.biquad.lowshelf(
    x,
    freq,
    gain_db,
    sr
)

filter.biquad.highshelf(
    x,
    freq,
    gain_db,
    sr
)
```

### DC blocker

```text
dc_block(x)
```

---

# 27. Delay / Feedback

```text
delay.fixed(x, time)

delay.variable(x, time)

delay.feedback(
    x,
    time,
    feedback
)

delay.multitap(
    x,
    time1,
    ...,
    time8
)

comb.feedforward(
    x,
    time,
    gain
)

comb.feedback(
    x,
    time,
    feedback
)

allpass(
    x,
    time,
    feedback
)
```

`delay.multitap` は指定された tap 数に応じて、

```text
tap1
tap2
...
tap8
```

を持つ bundle を返します。

---

# 28. Resonator / Physical Modeling

```text
resonator(
    x,
    freq,
    decay
)

resonator.q(
    x,
    freq,
    q
)

modal(
    x,
    freq,
    decay,
    gain
)

string.karplus(
    x,
    freq,
    decay,
    damping
)

waveguide(
    x,
    delay,
    feedback,
    damping
)

exciter.impulse(
    t,
    decay
)

exciter.noise(
    t,
    decay
)
```

---

# 29. Modulation

```text
chorus(
    x,
    rate,
    depth,
    delay
)

flanger(
    x,
    rate,
    depth,
    feedback
)

phaser(
    x,
    rate,
    depth,
    feedback
)

tremolo(
    x,
    rate,
    depth
)

vibrato(
    x,
    rate,
    depth
)
```

`rate` は Hz です。

時間を表す引数は秒です。

---

# 30. Distortion

```text
drive(x, amount)

saturate(x, amount)

waveshaper(
    x,
    drive,
    mix
)

wavefold(
    x,
    amount
)

bitcrush(
    x,
    bits
)

downsample(
    x,
    factor
)
```

---

# 31. Dynamics

```text
compressor(
    x,
    threshold,
    ratio,
    attack,
    release
)

limiter(
    x,
    threshold,
    attack,
    release
)

gate(
    x,
    threshold,
    attack,
    release
)

envelope_follower(
    x,
    attack,
    release
)
```

`threshold` は線形振幅です。

`attack` / `release` は秒です。

---

# 32. Control / Smoothing

```text
slew(
    x,
    rise,
    fall
)

smooth(
    x,
    time
)

sample_hold(
    x,
    rate
)

track_hold(
    x,
    gate
)
```

`sample_hold.rate` は Hz、それ以外の時間引数は秒です。

---

# 33. Stereo

```text
pan.equal_power(
    x,
    pan
)
```

返り値:

```text
left
right
```

```text
stereo.mid(l, r)

stereo.side(l, r)
```

```text
stereo.width(
    l,
    r,
    width
)
```

`stereo.width` は、

```text
left
right
```

を持つ bundle を返します。

---

# 34. Reverb

```text
reverb.early(
    x,
    size
)

reverb.schroeder(
    x,
    room,
    decay,
    damping
)

reverb.fdn(
    x,
    size,
    decay,
    damping
)
```

各 call site は独立した内部 state を持ちます。

Stereo reverb の左右を独立させる場合:

```text
left =
    reverb.fdn(
        in.wave_l,
        0.7,
        3s,
        0.5
    )

right =
    reverb.fdn(
        in.wave_r,
        0.7,
        3s,
        0.5
    )
```

---

# 35. Evaluation Model

DSL の意味論は sample 順です。

通常の local は各 evaluation で再計算されます。

```text
x = ...
```

persistent storage と stateful DSP は次の sample へ state を保持します。

```text
f32 voice x = 0
RingBuf<f32, 1s> voice buf
```

runtime は実際には複数 sample を block kernel として処理できます。

Voice は互いに独立しているため並列評価できます。

```text
Voice 0 ─┐
Voice 1 ─┼→ MIX → effect
Voice 2 ─┤
...     ─┘
```

これは runtime optimization であり、DSL 上の sample 順の意味論には影響しません。

---

# 36. Hot Reload

DSL source は Audio thread 外で、

```text
Lexer
  ↓
Parser
  ↓
Validator
  ↓
Cranelift JIT
  ↓
Runtime preparation
```

まで処理されます。

正常に compile された program のみ Audio block 境界で現在の program と交換されます。

compatible な persistent state / stateful DSP state は hot reload 後も維持されます。

sample rate が変更された場合は state を reset します。

---

# 37. Resource Model

以下には DSL 言語仕様上の固定個数上限を設けません。

```text
local
user function
operation
persistent scalar
RingBuf capacity
stateful DSP call site
```

過度に大きな program には Warning を表示できます。

parameter のみ最大32個です。

---

# 38. Grammar Summary

```text
program :=
    top_level*

top_level :=
      layout_declaration
    | parameter_declaration
    | function_declaration


layout_declaration :=
      "note.out.layout"
      "=" layout

    | "effect.in.layout"
      "=" layout

    | "effect.out.layout"
      "=" layout


layout :=
      "mono"
    | "stereo"


parameter_declaration :=
    "p." identifier
    "="
    "param"
    "(" arguments ")"


function_declaration :=
    "fn"
    qualified_name
    "(" parameter_names? ")"
    "->"
    "out"
    "{"
        statement*
    "}"


statement :=
      assignment
    | scalar_storage_declaration
    | ringbuf_storage_declaration


scalar_storage_declaration :=
    "f32"
    storage_domain
    qualified_name
    "="
    expression


ringbuf_storage_declaration :=
    "RingBuf"
    "<"
        "f32"
        ","
        numeric_literal
    ">"
    storage_domain
    qualified_name


storage_domain :=
      "voice"
    | "global"


assignment :=
    qualified_name
    ("=" qualified_name)*
    "="
    expression


expression :=
      numeric_literal
    | qualified_name
    | call
    | unary_expression
    | binary_expression
    | "(" expression ")"


call :=
    qualified_name
    "(" arguments? ")"


arguments :=
    expression
    ("," expression)*


qualified_name :=
    identifier
    ("." identifier)*
```

Semantic constraints:

```text
fn note:
    0..1

fn effect:
    0..1

fn note + fn effect:
    at least one required

note call tree:
    voice storage only

effect call tree:
    global storage only

persistent scalar:
    initializer required

parameter declaration:
    top-level only
    maximum 32

recursive function call:
    forbidden

forward function reference:
    allowed
```

---

# 39. Minimal Examples

## Instrument

```text
note.out.layout = mono

p.gain =
    param(0.8, 0, 1, 0.01)

fn note(in, p) -> out {
    out.wave =
        sin(
            TAU
            * in.freq
            * in.t
        )
        * in.s
        * p.gain

    out.l_limit = 1s
}
```

## Effect

```text
effect.in.layout = stereo
effect.out.layout = stereo

p.drive =
    param(1, 1, 8, 0.01)

fn effect(in, p) -> out {
    out.wave_l =
        tanh(
            in.wave_l
            * p.drive
        )

    out.wave_r =
        tanh(
            in.wave_r
            * p.drive
        )
}
```

## Instrument + Effect

```text
note.out.layout = mono

effect.in.layout = stereo
effect.out.layout = stereo

p.gain =
    param(0.8, 0, 1, 0.01)

p.drive =
    param(1, 1, 8, 0.01)

fn note(in, p) -> out {
    wave =
        sin(
            TAU
            * in.freq
            * in.t
        )

    out.wave =
        wave
        * in.s
        * p.gain

    out.l_limit = 1s
}

fn effect(in, p) -> out {
    out.wave_l =
        tanh(
            in.wave_l
            * p.drive
        )

    out.wave_r =
        tanh(
            in.wave_r
            * p.drive
        )
}
```
