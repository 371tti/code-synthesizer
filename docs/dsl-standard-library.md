# DSL Standard Library

## Overview

標準ライブラリは「純粋primitive」「bundleを返す関数」「内部stateを持つDSP」の3種類です。純粋primitiveは通常のCranelift IRとして評価され、state付きDSPはcall siteごとに必要なmemoryをcompile後・Audio thread外で確保します。sample処理中のheap allocationやlockはありません。

state付きDSPを`fn note`のcall treeで使うとVoiceごとのstate、`fn filter`のcall treeで使うとPlugin全体のglobal stateになります。同じsource位置・種類・sample rateでhot reloadした場合はstateを引き継ぎます。

## Worker Evaluation Compatibility

runtimeはworker dispatchに適した`note` entryを解析します。`f32 global` はrelaxed Atomic storageで共有でき、同一sampleに複数Voiceが書き込んだ場合は実行順に依存するlast-writer-winsです。現在は速度を優先してsample単位workerを無効化しており、Compile statusには`Serial`と表示されます。

`note`からGlobal RingBufまたはGlobal stateful DSPへ触れる場合、それらはworkerごとのrelaxed shardとして実行されます。worker間で同じfeedback stateは共有されません。`note` domainだけは同一ノートの複数Voiceで共有されるため、使用したProgramは直列評価対象になります。`filter`内のGlobal RingBuf/DSPは従来どおり単一の直列・transactional stateです。

時間引数の単位は秒です。`10ms`、`250us`、`2s`のsuffixをそのまま使用できます。

## RingBuf

`RingBuf<f32, Size> domain name`で宣言したRingBufには次のmethodがあります。

| Signature | 内容 |
| --- | --- |
| `buffer.peek(delay)` | 指定秒数だけ過去の値を読みます。同一sample内でcursorは移動しません。 |
| `buffer.peek_linear(delay)` | fractional sample位置を線形補間して読みます。 |
| `buffer.len()` | 容量をsample数で返します。 |
| `buffer.duration()` | 容量を秒で返します。 |

`peek*`も通常のRingBuf read/writeと同じくtransactionalです。writeはsample末尾のcommitまで反映されず、同一sample内では安定した過去値を読みます。

```text
fn filter(in, p) -> out {
    RingBuf<f32, 500ms> global delay
    wet = delay.peek_linear(120ms)
    delay = in.wave_l + wet * 0.4
    out.wave_l = in.wave_l + wet * 0.25
    out.wave_r = in.wave_r + wet * 0.25
}
```

## Math and DSP Utility

| Signature | 内容 |
| --- | --- |
| `exp2(x)` | `2^x` |
| `wrap(x, min, max)` | 周期的な範囲wrap |
| `hypot(x, y)` | 安定した`sqrt(x*x+y*y)` |
| `sinc(x)` | 正規化sinc |
| `hash(x)` / `hash2(x, y)` | 決定論的な`-1..1`値 |
| `fold(x, min, max)` | 境界で反射するfold |
| `pan_l(pan)` / `pan_r(pan)` | Equal-Power Pan gain |
| `onepole_coeff(freq, sr)` | 1-pole係数 |

## Biquad Coefficients

係数関数は`b0`、`b1`、`b2`、`a1`、`a2`を持つbundleを返します。係数は`a0=1`へ正規化済みです。

```text
coeff = biquad.lowpass(1200, 0.707, in.sr)
```

- `biquad.lowpass(freq, q, sr)`
- `biquad.highpass(freq, q, sr)`
- `biquad.bandpass(freq, q, sr)`
- `biquad.notch(freq, q, sr)`
- `biquad.allpass(freq, q, sr)`
- `biquad.peak(freq, q, gain_db, sr)`
- `biquad.lowshelf(freq, q, gain_db, sr)`
- `biquad.highshelf(freq, q, gain_db, sr)`

## Window

- `window.hann(x)`
- `window.hamming(x)`
- `window.blackman(x)`

`x`は`0..1`へclampされます。

## Filter

- `filter.onepole.lp(x, freq, sr)`
- `filter.onepole.hp(x, freq, sr)`
- `filter.svf.lp(x, freq, q, sr)`
- `filter.svf.hp(x, freq, q, sr)`
- `filter.svf.bp(x, freq, q, sr)`
- `filter.svf.notch(x, freq, q, sr)`
- `filter.biquad.lp(x, freq, q, sr)`
- `filter.biquad.hp(x, freq, q, sr)`
- `filter.biquad.bp(x, freq, q, sr)`
- `filter.biquad.notch(x, freq, q, sr)`
- `filter.biquad.allpass(x, freq, q, sr)`
- `filter.biquad.peak(x, freq, q, gain_db, sr)`
- `filter.biquad.lowshelf(x, freq, gain_db, sr)`
- `filter.biquad.highshelf(x, freq, gain_db, sr)`
- `dc_block(x)`

## Delay and Feedback

- `delay.fixed(x, time)`
- `delay.variable(x, time)`
- `delay.feedback(x, time, feedback)`
- `delay.multitap(x, time1, ..., time8)`
- `comb.feedforward(x, time, gain)`
- `comb.feedback(x, time, feedback)`
- `allpass(x, time, feedback)`

`delay.multitap`は`tap1`から`tap8`まで、指定した個数のfieldを持つbundleを返します。

## Resonator and Physical Modeling

- `resonator(x, freq, decay)`
- `resonator.q(x, freq, q)`
- `modal(x, freq, decay, gain)`
- `string.karplus(x, freq, decay, damping)`
- `waveguide(x, delay, feedback, damping)`
- `exciter.impulse(t, decay)`
- `exciter.noise(t, decay)`

## Modulation Effects

- `chorus(x, rate, depth, delay)`
- `flanger(x, rate, depth, feedback)`
- `phaser(x, rate, depth, feedback)`
- `tremolo(x, rate, depth)`
- `vibrato(x, rate, depth)`

`rate`はHzです。Chorus、Flanger、Vibratoのdepthとdelayは秒です。

## Waveshaping and Distortion

- `drive(x, amount)`
- `saturate(x, amount)`
- `waveshaper(x, drive, mix)`
- `wavefold(x, amount)`
- `bitcrush(x, bits)`
- `downsample(x, factor)`

## Dynamics

- `compressor(x, threshold, ratio, attack, release)`
- `limiter(x, threshold, attack, release)`
- `gate(x, threshold, attack, release)`
- `envelope_follower(x, attack, release)`

thresholdは線形振幅、attack/releaseは秒です。

## Control and Smoothing

- `slew(x, rise, fall)`
- `smooth(x, time)`
- `sample_hold(x, rate)`
- `track_hold(x, gate)`

`sample_hold`のrateはHz、そのほかの時間引数は秒です。

## Stereo

`pan.equal_power`と`stereo.width`は`left`、`right`を持つbundleを返します。

- `pan.equal_power(x, pan)`
- `stereo.mid(l, r)`
- `stereo.side(l, r)`
- `stereo.width(l, r, width)`

## Reverb

- `reverb.early(x, size)`
- `reverb.schroeder(x, room, decay, damping)`
- `reverb.fdn(x, size, decay, damping)`

Reverbをpost-mix処理として使う場合は`fn filter`内で左右それぞれに異なるcall siteを用意すると、左右の内部stateも独立します。
