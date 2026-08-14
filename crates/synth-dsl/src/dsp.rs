//! Standard DSP primitives and preallocated stateful processors.

use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub(crate) enum StandardOp {
    Exp2,
    Wrap,
    Hypot,
    Sinc,
    Hash,
    Hash2,
    Fold,
    PanL,
    PanR,
    OnePoleCoeff,
    WindowHann,
    WindowHamming,
    WindowBlackman,
    Drive,
    Saturate,
    Waveshaper,
    Wavefold,
    Bitcrush,
    StereoMid,
    StereoSide,
    ExciterImpulse,
    ExciterNoise,
    PanSignalL,
    PanSignalR,
    StereoWidthL,
    StereoWidthR,
}

impl StandardOp {
    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Exp2
            | Self::Sinc
            | Self::Hash
            | Self::PanL
            | Self::PanR
            | Self::WindowHann
            | Self::WindowHamming
            | Self::WindowBlackman => 1,
            Self::Hypot
            | Self::Hash2
            | Self::OnePoleCoeff
            | Self::Drive
            | Self::Saturate
            | Self::Wavefold
            | Self::Bitcrush
            | Self::StereoMid
            | Self::StereoSide
            | Self::ExciterImpulse
            | Self::ExciterNoise
            | Self::PanSignalL
            | Self::PanSignalR => 2,
            Self::Wrap
            | Self::Fold
            | Self::Waveshaper
            | Self::StereoWidthL
            | Self::StereoWidthR => 3,
        }
    }

    fn from_u32(value: u32) -> Option<Self> {
        (value <= Self::StereoWidthR as u32).then(|| {
            // SAFETY: repr(u32) variants are contiguous and range checked above.
            unsafe { std::mem::transmute::<u32, Self>(value) }
        })
    }
}

pub(crate) fn standard_operation(name: &str, arity: usize) -> Option<StandardOp> {
    let operation = match name {
        "exp2" => StandardOp::Exp2,
        "wrap" => StandardOp::Wrap,
        "hypot" => StandardOp::Hypot,
        "sinc" => StandardOp::Sinc,
        "hash" => StandardOp::Hash,
        "hash2" => StandardOp::Hash2,
        "fold" => StandardOp::Fold,
        "pan_l" => StandardOp::PanL,
        "pan_r" => StandardOp::PanR,
        "onepole_coeff" => StandardOp::OnePoleCoeff,
        "window.hann" => StandardOp::WindowHann,
        "window.hamming" => StandardOp::WindowHamming,
        "window.blackman" => StandardOp::WindowBlackman,
        "drive" => StandardOp::Drive,
        "saturate" => StandardOp::Saturate,
        "waveshaper" => StandardOp::Waveshaper,
        "wavefold" => StandardOp::Wavefold,
        "bitcrush" => StandardOp::Bitcrush,
        "stereo.mid" => StandardOp::StereoMid,
        "stereo.side" => StandardOp::StereoSide,
        "exciter.impulse" => StandardOp::ExciterImpulse,
        "exciter.noise" => StandardOp::ExciterNoise,
        _ => return None,
    };
    (operation.arity() == arity).then_some(operation)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub(crate) enum BiquadKind {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Allpass,
    Peak,
    LowShelf,
    HighShelf,
}

impl BiquadKind {
    pub(crate) const fn coefficient_arity(self) -> usize {
        match self {
            Self::Peak | Self::LowShelf | Self::HighShelf => 4,
            _ => 3,
        }
    }
}

pub(crate) fn biquad_kind(name: &str, arity: usize) -> Option<BiquadKind> {
    let kind = match name {
        "biquad.lowpass" => BiquadKind::Lowpass,
        "biquad.highpass" => BiquadKind::Highpass,
        "biquad.bandpass" => BiquadKind::Bandpass,
        "biquad.notch" => BiquadKind::Notch,
        "biquad.allpass" => BiquadKind::Allpass,
        "biquad.peak" => BiquadKind::Peak,
        "biquad.lowshelf" => BiquadKind::LowShelf,
        "biquad.highshelf" => BiquadKind::HighShelf,
        _ => return None,
    };
    (kind.coefficient_arity() == arity).then_some(kind)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DspKind {
    OnePoleLp,
    OnePoleHp,
    SvfLp,
    SvfHp,
    SvfBp,
    SvfNotch,
    BiquadLp,
    BiquadHp,
    BiquadBp,
    BiquadNotch,
    BiquadAllpass,
    BiquadPeak,
    BiquadLowShelf,
    BiquadHighShelf,
    DcBlock,
    DelayFixed,
    DelayVariable,
    DelayFeedback,
    CombFeedForward,
    CombFeedback,
    Allpass,
    Resonator,
    ResonatorQ,
    Modal,
    KarplusStrong,
    Waveguide,
    Chorus,
    Flanger,
    Phaser,
    Tremolo,
    Vibrato,
    Downsample,
    Compressor,
    Limiter,
    Gate,
    EnvelopeFollower,
    Slew,
    Smooth,
    SampleHold,
    TrackHold,
    ReverbEarly,
    ReverbSchroeder,
    ReverbFdn,
}

impl DspKind {
    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::DcBlock => 1,
            Self::DelayFixed
            | Self::DelayVariable
            | Self::Downsample
            | Self::Smooth
            | Self::SampleHold
            | Self::TrackHold
            | Self::ReverbEarly => 2,
            Self::OnePoleLp
            | Self::OnePoleHp
            | Self::DelayFeedback
            | Self::CombFeedForward
            | Self::CombFeedback
            | Self::Allpass
            | Self::Resonator
            | Self::ResonatorQ
            | Self::Tremolo
            | Self::Vibrato
            | Self::EnvelopeFollower
            | Self::Slew => 3,
            Self::SvfLp
            | Self::SvfHp
            | Self::SvfBp
            | Self::SvfNotch
            | Self::BiquadLp
            | Self::BiquadHp
            | Self::BiquadBp
            | Self::BiquadNotch
            | Self::BiquadAllpass
            | Self::BiquadLowShelf
            | Self::BiquadHighShelf
            | Self::Modal
            | Self::KarplusStrong
            | Self::Waveguide
            | Self::Chorus
            | Self::Flanger
            | Self::Phaser
            | Self::Limiter
            | Self::Gate
            | Self::ReverbSchroeder
            | Self::ReverbFdn => 4,
            Self::BiquadPeak | Self::Compressor => 5,
        }
    }

    pub(crate) const fn ring_durations(self) -> &'static [f32] {
        match self {
            Self::DelayFixed
            | Self::DelayVariable
            | Self::DelayFeedback
            | Self::CombFeedForward
            | Self::CombFeedback
            | Self::Allpass
            | Self::Waveguide => &[2.0],
            Self::KarplusStrong | Self::Chorus | Self::Vibrato => &[0.12],
            Self::Flanger => &[0.05],
            Self::ReverbEarly => &[0.5],
            Self::ReverbSchroeder => &[0.12, 0.12, 0.12, 0.12, 0.03, 0.03],
            Self::ReverbFdn => &[0.25, 0.25, 0.25, 0.25],
            _ => &[],
        }
    }
}

pub(crate) fn dsp_kind(name: &str, arity: usize) -> Option<DspKind> {
    let kind = match name {
        "filter.onepole.lp" => DspKind::OnePoleLp,
        "filter.onepole.hp" => DspKind::OnePoleHp,
        "filter.svf.lp" => DspKind::SvfLp,
        "filter.svf.hp" => DspKind::SvfHp,
        "filter.svf.bp" => DspKind::SvfBp,
        "filter.svf.notch" => DspKind::SvfNotch,
        "filter.biquad.lp" => DspKind::BiquadLp,
        "filter.biquad.hp" => DspKind::BiquadHp,
        "filter.biquad.bp" => DspKind::BiquadBp,
        "filter.biquad.notch" => DspKind::BiquadNotch,
        "filter.biquad.allpass" => DspKind::BiquadAllpass,
        "filter.biquad.peak" => DspKind::BiquadPeak,
        "filter.biquad.lowshelf" => DspKind::BiquadLowShelf,
        "filter.biquad.highshelf" => DspKind::BiquadHighShelf,
        "dc_block" => DspKind::DcBlock,
        "delay.fixed" => DspKind::DelayFixed,
        "delay.variable" => DspKind::DelayVariable,
        "delay.feedback" => DspKind::DelayFeedback,
        "comb.feedforward" => DspKind::CombFeedForward,
        "comb.feedback" => DspKind::CombFeedback,
        "allpass" => DspKind::Allpass,
        "resonator" => DspKind::Resonator,
        "resonator.q" => DspKind::ResonatorQ,
        "modal" => DspKind::Modal,
        "string.karplus" => DspKind::KarplusStrong,
        "waveguide" => DspKind::Waveguide,
        "chorus" => DspKind::Chorus,
        "flanger" => DspKind::Flanger,
        "phaser" => DspKind::Phaser,
        "tremolo" => DspKind::Tremolo,
        "vibrato" => DspKind::Vibrato,
        "downsample" => DspKind::Downsample,
        "compressor" => DspKind::Compressor,
        "limiter" => DspKind::Limiter,
        "gate" => DspKind::Gate,
        "envelope_follower" => DspKind::EnvelopeFollower,
        "slew" => DspKind::Slew,
        "smooth" => DspKind::Smooth,
        "sample_hold" => DspKind::SampleHold,
        "track_hold" => DspKind::TrackHold,
        "reverb.early" => DspKind::ReverbEarly,
        "reverb.schroeder" => DspKind::ReverbSchroeder,
        "reverb.fdn" => DspKind::ReverbFdn,
        _ => return None,
    };
    (kind.arity() == arity).then_some(kind)
}

pub(crate) extern "C" fn jit_standard(
    operation: u32,
    a: f32,
    b: f32,
    c: f32,
    _d: f32,
    _e: f32,
) -> f32 {
    let Some(operation) = StandardOp::from_u32(operation) else {
        return 0.0;
    };
    match operation {
        StandardOp::Exp2 => a.exp2(),
        StandardOp::Wrap => wrap(a, b, c),
        StandardOp::Hypot => a.hypot(b),
        StandardOp::Sinc => {
            if a.abs() < 1.0e-6 {
                1.0
            } else {
                (PI * a).sin() / (PI * a)
            }
        }
        StandardOp::Hash => hash_u32(a.to_bits()),
        StandardOp::Hash2 => hash_u32(a.to_bits() ^ b.to_bits().rotate_left(16)),
        StandardOp::Fold => fold(a, b, c),
        StandardOp::PanL => ((1.0 - a.clamp(-1.0, 1.0)) * 0.5).sqrt(),
        StandardOp::PanR => ((1.0 + a.clamp(-1.0, 1.0)) * 0.5).sqrt(),
        StandardOp::OnePoleCoeff => onepole_coeff(a, b),
        StandardOp::WindowHann => 0.5 - 0.5 * (TAU * a.clamp(0.0, 1.0)).cos(),
        StandardOp::WindowHamming => 0.54 - 0.46 * (TAU * a.clamp(0.0, 1.0)).cos(),
        StandardOp::WindowBlackman => {
            let x = a.clamp(0.0, 1.0);
            0.42 - 0.5 * (TAU * x).cos() + 0.08 * (2.0 * TAU * x).cos()
        }
        StandardOp::Drive => normalized_tanh(a, b),
        StandardOp::Saturate => {
            let amount = b.max(0.0);
            a * (1.0 + amount) / (1.0 + amount * a.abs())
        }
        StandardOp::Waveshaper => a + (normalized_tanh(a, b) - a) * c.clamp(0.0, 1.0),
        StandardOp::Wavefold => fold(a * b.max(0.0), -1.0, 1.0),
        StandardOp::Bitcrush => {
            let levels = 2.0f32.powf(b.round().clamp(1.0, 24.0) - 1.0);
            (a.clamp(-1.0, 1.0) * levels).round() / levels
        }
        StandardOp::StereoMid => (a + b) * 0.5,
        StandardOp::StereoSide => (a - b) * 0.5,
        StandardOp::ExciterImpulse => (-a.max(0.0) / b.max(1.0e-6)).exp(),
        StandardOp::ExciterNoise => {
            let noise = hash_u32((a.max(0.0) * 48_000.0).floor().to_bits());
            noise * (-a.max(0.0) / b.max(1.0e-6)).exp()
        }
        StandardOp::PanSignalL => a * ((1.0 - b.clamp(-1.0, 1.0)) * 0.5).sqrt(),
        StandardOp::PanSignalR => a * ((1.0 + b.clamp(-1.0, 1.0)) * 0.5).sqrt(),
        StandardOp::StereoWidthL => {
            let mid = (a + b) * 0.5;
            let side = (a - b) * 0.5 * c.max(0.0);
            mid + side
        }
        StandardOp::StereoWidthR => {
            let mid = (a + b) * 0.5;
            let side = (a - b) * 0.5 * c.max(0.0);
            mid - side
        }
    }
}

pub(crate) extern "C" fn jit_biquad_coefficient(
    kind: u32,
    coefficient: u32,
    freq: f32,
    q: f32,
    gain_db: f32,
    sample_rate: f32,
) -> f32 {
    let kind = match kind {
        0 => BiquadKind::Lowpass,
        1 => BiquadKind::Highpass,
        2 => BiquadKind::Bandpass,
        3 => BiquadKind::Notch,
        4 => BiquadKind::Allpass,
        5 => BiquadKind::Peak,
        6 => BiquadKind::LowShelf,
        7 => BiquadKind::HighShelf,
        _ => return 0.0,
    };
    biquad_coefficients(kind, freq, q, gain_db, sample_rate)
        .get(coefficient as usize)
        .copied()
        .unwrap_or(0.0)
}

fn normalized_tanh(x: f32, amount: f32) -> f32 {
    let amount = amount.max(0.000_1);
    (x * amount).tanh() / amount.tanh().max(0.000_1)
}

fn hash_u32(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn wrap(x: f32, min: f32, max: f32) -> f32 {
    let lo = min.min(max);
    let hi = min.max(max);
    let width = hi - lo;
    if width <= f32::EPSILON {
        lo
    } else {
        (x - lo).rem_euclid(width) + lo
    }
}

fn fold(x: f32, min: f32, max: f32) -> f32 {
    let lo = min.min(max);
    let hi = min.max(max);
    let width = hi - lo;
    if width <= f32::EPSILON {
        lo
    } else {
        let position = (x - lo).rem_euclid(width * 2.0);
        if position <= width {
            lo + position
        } else {
            hi - (position - width)
        }
    }
}

fn onepole_coeff(freq: f32, sample_rate: f32) -> f32 {
    let sample_rate = sample_rate.max(1.0);
    let freq = freq.clamp(0.0, sample_rate * 0.499);
    1.0 - (-TAU * freq / sample_rate).exp()
}

pub(crate) fn biquad_coefficients(
    kind: BiquadKind,
    freq: f32,
    q: f32,
    gain_db: f32,
    sample_rate: f32,
) -> [f32; 5] {
    let sample_rate = sample_rate.max(1_000.0);
    let freq = freq.clamp(1.0, sample_rate * 0.499);
    let q = q.clamp(0.025, 40.0);
    let omega = TAU * freq / sample_rate;
    let sin = omega.sin();
    let cos = omega.cos();
    let alpha = sin / (2.0 * q);
    let amplitude = 10.0f32.powf(gain_db.clamp(-60.0, 60.0) / 40.0);
    let (b0, b1, b2, a0, a1, a2) = match kind {
        BiquadKind::Lowpass => (
            (1.0 - cos) * 0.5,
            1.0 - cos,
            (1.0 - cos) * 0.5,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ),
        BiquadKind::Highpass => (
            (1.0 + cos) * 0.5,
            -(1.0 + cos),
            (1.0 + cos) * 0.5,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ),
        BiquadKind::Bandpass => (
            sin * 0.5,
            0.0,
            -sin * 0.5,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ),
        BiquadKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        BiquadKind::Allpass => (
            1.0 - alpha,
            -2.0 * cos,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ),
        BiquadKind::Peak => (
            1.0 + alpha * amplitude,
            -2.0 * cos,
            1.0 - alpha * amplitude,
            1.0 + alpha / amplitude,
            -2.0 * cos,
            1.0 - alpha / amplitude,
        ),
        BiquadKind::LowShelf | BiquadKind::HighShelf => {
            let slope = q.clamp(0.1, 2.0);
            let shelf_alpha = sin
                * 0.5
                * ((amplitude + 1.0 / amplitude) * (1.0 / slope - 1.0) + 2.0)
                    .max(0.0)
                    .sqrt();
            let beta = 2.0 * amplitude.sqrt() * shelf_alpha;
            if matches!(kind, BiquadKind::LowShelf) {
                (
                    amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos + beta),
                    2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
                    amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos - beta),
                    (amplitude + 1.0) + (amplitude - 1.0) * cos + beta,
                    -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
                    (amplitude + 1.0) + (amplitude - 1.0) * cos - beta,
                )
            } else {
                (
                    amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos + beta),
                    -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
                    amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos - beta),
                    (amplitude + 1.0) - (amplitude - 1.0) * cos + beta,
                    2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
                    (amplitude + 1.0) - (amplitude - 1.0) * cos - beta,
                )
            }
        }
    };
    let inverse = 1.0 / a0.max(1.0e-12);
    [
        b0 * inverse,
        b1 * inverse,
        b2 * inverse,
        a1 * inverse,
        a2 * inverse,
    ]
}

struct DspRing {
    values: Box<[f32]>,
    cursor: usize,
}

impl DspRing {
    fn new(seconds: f32, sample_rate: f32) -> Result<Self, String> {
        let capacity = (seconds * sample_rate).ceil().max(2.0) as usize + 2;
        let mut values = Vec::new();
        values
            .try_reserve_exact(capacity)
            .map_err(|_| format!("標準DSP用Delay memoryを確保できません（{capacity} samples）"))?;
        values.resize(capacity, 0.0);
        Ok(Self {
            values: values.into_boxed_slice(),
            cursor: 0,
        })
    }

    fn read(&self, seconds: f32, sample_rate: f32, linear: bool) -> f32 {
        let delay = (seconds.max(0.0) * sample_rate).clamp(1.0, (self.values.len() - 2) as f32);
        let whole = delay.floor() as usize;
        let fraction = delay - whole as f32;
        let index_a = (self.cursor + self.values.len() - whole) % self.values.len();
        if !linear {
            return self.values[index_a];
        }
        let index_b = (self.cursor + self.values.len() - whole - 1) % self.values.len();
        self.values[index_a] + (self.values[index_b] - self.values[index_a]) * fraction
    }

    fn push(&mut self, value: f32) {
        self.values[self.cursor] = value;
        self.cursor += 1;
        if self.cursor == self.values.len() {
            self.cursor = 0;
        }
    }

    fn reset(&mut self) {
        self.values.fill(0.0);
        self.cursor = 0;
    }
}

pub(crate) struct DspProcessor {
    kind: DspKind,
    sample_rate: f32,
    values: [f32; 32],
    rings: Vec<DspRing>,
}

impl DspProcessor {
    pub(crate) fn new(kind: DspKind, sample_rate: f32) -> Result<Self, String> {
        let rings = kind
            .ring_durations()
            .iter()
            .map(|seconds| DspRing::new(*seconds, sample_rate))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            kind,
            sample_rate,
            values: [0.0; 32],
            rings,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.values.fill(0.0);
        for ring in &mut self.rings {
            ring.reset();
        }
    }

    pub(crate) fn process(&mut self, arguments: [f32; 5]) -> f32 {
        let [a, b, c, d, e] = arguments;
        match self.kind {
            DspKind::OnePoleLp => self.onepole(a, b, false),
            DspKind::OnePoleHp => self.onepole(a, b, true),
            DspKind::SvfLp => self.svf(a, b, c, 0),
            DspKind::SvfHp => self.svf(a, b, c, 1),
            DspKind::SvfBp => self.svf(a, b, c, 2),
            DspKind::SvfNotch => self.svf(a, b, c, 3),
            DspKind::BiquadLp => self.biquad(a, BiquadKind::Lowpass, b, c, 0.0),
            DspKind::BiquadHp => self.biquad(a, BiquadKind::Highpass, b, c, 0.0),
            DspKind::BiquadBp => self.biquad(a, BiquadKind::Bandpass, b, c, 0.0),
            DspKind::BiquadNotch => self.biquad(a, BiquadKind::Notch, b, c, 0.0),
            DspKind::BiquadAllpass => self.biquad(a, BiquadKind::Allpass, b, c, 0.0),
            DspKind::BiquadPeak => self.biquad(a, BiquadKind::Peak, b, c, d),
            DspKind::BiquadLowShelf => self.biquad(a, BiquadKind::LowShelf, b, 0.707, c),
            DspKind::BiquadHighShelf => self.biquad(a, BiquadKind::HighShelf, b, 0.707, c),
            DspKind::DcBlock => {
                let output = a - self.values[0] + 0.995 * self.values[1];
                self.values[0] = a;
                self.values[1] = output;
                output
            }
            DspKind::DelayFixed => self.delay(a, b, 0.0, false, false),
            DspKind::DelayVariable => self.delay(a, b, 0.0, true, false),
            DspKind::DelayFeedback => self.delay(a, b, c, true, true),
            DspKind::CombFeedForward => {
                let delayed = self.rings[0].read(b, self.sample_rate, true);
                self.rings[0].push(a);
                a + delayed * c.clamp(-0.999, 0.999)
            }
            DspKind::CombFeedback => {
                let delayed = self.rings[0].read(b, self.sample_rate, true);
                let output = a + delayed * c.clamp(-0.999, 0.999);
                self.rings[0].push(output);
                output
            }
            DspKind::Allpass => {
                let gain = c.clamp(-0.999, 0.999);
                let delayed = self.rings[0].read(b, self.sample_rate, true);
                let output = delayed - gain * a;
                self.rings[0].push(a + gain * output);
                output
            }
            DspKind::Resonator => self.resonator(a, b, c, 1.0),
            DspKind::ResonatorQ => self.biquad(a, BiquadKind::Bandpass, b, c, 0.0),
            DspKind::Modal => self.resonator(a, b, c, d),
            DspKind::KarplusStrong => self.karplus(a, b, c, d),
            DspKind::Waveguide => self.waveguide(a, b, c, d),
            DspKind::Chorus => self.chorus(a, b, c, d),
            DspKind::Flanger => self.flanger(a, b, c, d),
            DspKind::Phaser => self.phaser(a, b, c, d),
            DspKind::Tremolo => self.tremolo(a, b, c),
            DspKind::Vibrato => self.vibrato(a, b, c),
            DspKind::Downsample => self.downsample(a, b),
            DspKind::Compressor => self.compressor(a, b, c, d, e, false),
            DspKind::Limiter => self.compressor(a, b, 100.0, c, d, false),
            DspKind::Gate => self.gate(a, b, c, d),
            DspKind::EnvelopeFollower => self.envelope(a, b, c),
            DspKind::Slew => self.slew(a, b, c),
            DspKind::Smooth => self.smooth(a, b),
            DspKind::SampleHold => self.sample_hold(a, b),
            DspKind::TrackHold => self.track_hold(a, b),
            DspKind::ReverbEarly => self.early(a, b),
            DspKind::ReverbSchroeder => self.schroeder(a, b, c, d),
            DspKind::ReverbFdn => self.fdn(a, b, c, d),
        }
    }

    fn onepole(&mut self, input: f32, frequency: f32, highpass: bool) -> f32 {
        let coefficient = onepole_coeff(frequency, self.sample_rate);
        self.values[0] += coefficient * (input - self.values[0]);
        if highpass {
            input - self.values[0]
        } else {
            self.values[0]
        }
    }

    fn svf(&mut self, input: f32, frequency: f32, q: f32, mode: usize) -> f32 {
        let frequency = frequency.clamp(1.0, self.sample_rate * 0.45);
        let g = (PI * frequency / self.sample_rate).tan().min(8.0);
        let k = 1.0 / q.clamp(0.1, 40.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let v3 = input - self.values[0];
        let v1 = a1 * self.values[1] + g * a1 * v3;
        let v2 = self.values[0] + g * v1;
        self.values[0] = 2.0 * v2 - self.values[0];
        self.values[1] = 2.0 * v1 - self.values[1];
        let high = input - k * v1 - v2;
        [v2, high, v1, high + v2][mode]
    }

    fn biquad(
        &mut self,
        input: f32,
        kind: BiquadKind,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> f32 {
        let [b0, b1, b2, a1, a2] =
            biquad_coefficients(kind, frequency, q, gain_db, self.sample_rate);
        let output = b0 * input + b1 * self.values[0] + b2 * self.values[1]
            - a1 * self.values[2]
            - a2 * self.values[3];
        self.values[1] = self.values[0];
        self.values[0] = input;
        self.values[3] = self.values[2];
        self.values[2] = output;
        output
    }

    fn delay(
        &mut self,
        input: f32,
        time: f32,
        feedback: f32,
        linear: bool,
        feed_back: bool,
    ) -> f32 {
        let delayed = self.rings[0].read(time, self.sample_rate, linear);
        self.rings[0].push(if feed_back {
            input + delayed * feedback.clamp(-0.999, 0.999)
        } else {
            input
        });
        delayed
    }

    fn resonator(&mut self, input: f32, frequency: f32, decay: f32, gain: f32) -> f32 {
        let frequency = frequency.clamp(1.0, self.sample_rate * 0.45);
        let radius = 0.001f32.powf(1.0 / (decay.max(0.001) * self.sample_rate));
        let coefficient = 2.0 * radius * (TAU * frequency / self.sample_rate).cos();
        let output = input * gain + coefficient * self.values[0] - radius * radius * self.values[1];
        self.values[1] = self.values[0];
        self.values[0] = output;
        output
    }

    fn karplus(&mut self, input: f32, frequency: f32, decay: f32, damping: f32) -> f32 {
        let frequency = frequency.clamp(10.0, self.sample_rate * 0.45);
        let delayed = self.rings[0].read(1.0 / frequency, self.sample_rate, true);
        let damping = damping.clamp(0.0, 1.0);
        let filtered = damping * self.values[0] + (1.0 - damping) * delayed;
        self.values[0] = filtered;
        let feedback = 0.001f32.powf(1.0 / (frequency * decay.max(0.01)));
        self.rings[0].push(input + filtered * feedback);
        delayed
    }

    fn waveguide(&mut self, input: f32, delay: f32, feedback: f32, damping: f32) -> f32 {
        let delayed = self.rings[0].read(delay, self.sample_rate, true);
        let filtered =
            self.values[0] + (1.0 - damping.clamp(0.0, 0.999)) * (delayed - self.values[0]);
        self.values[0] = filtered;
        self.rings[0].push(input + filtered * feedback.clamp(-0.999, 0.999));
        delayed
    }

    fn advance_phase(&mut self, index: usize, rate: f32) -> f32 {
        let phase = self.values[index];
        self.values[index] = (phase + rate.max(0.0) / self.sample_rate).rem_euclid(1.0);
        phase
    }

    fn chorus(&mut self, input: f32, rate: f32, depth: f32, delay: f32) -> f32 {
        let phase = self.advance_phase(0, rate);
        let time = delay.max(0.001) + depth.max(0.0) * (0.5 + 0.5 * (TAU * phase).sin());
        let delayed = self.rings[0].read(time, self.sample_rate, true);
        self.rings[0].push(input);
        0.5 * (input + delayed)
    }

    fn flanger(&mut self, input: f32, rate: f32, depth: f32, feedback: f32) -> f32 {
        let phase = self.advance_phase(0, rate);
        let time = 0.000_5 + depth.clamp(0.0, 0.02) * (0.5 + 0.5 * (TAU * phase).sin());
        let delayed = self.rings[0].read(time, self.sample_rate, true);
        self.rings[0].push(input + delayed * feedback.clamp(-0.95, 0.95));
        0.5 * (input + delayed)
    }

    fn phaser(&mut self, input: f32, rate: f32, depth: f32, feedback: f32) -> f32 {
        let phase = self.advance_phase(0, rate);
        let frequency =
            200.0 * 10.0f32.powf(1.5 * depth.clamp(0.0, 1.0) * (0.5 + 0.5 * (TAU * phase).sin()));
        let tangent = (PI * frequency.clamp(20.0, self.sample_rate * 0.4) / self.sample_rate).tan();
        let coefficient = (1.0 - tangent) / (1.0 + tangent);
        let mut value = input + self.values[9] * feedback.clamp(-0.95, 0.95);
        for stage in 0..4 {
            let previous_input = self.values[1 + stage * 2];
            let previous_output = self.values[2 + stage * 2];
            let output = -coefficient * value + previous_input + coefficient * previous_output;
            self.values[1 + stage * 2] = value;
            self.values[2 + stage * 2] = output;
            value = output;
        }
        self.values[9] = value;
        0.5 * (input + value)
    }

    fn tremolo(&mut self, input: f32, rate: f32, depth: f32) -> f32 {
        let phase = self.advance_phase(0, rate);
        let depth = depth.clamp(0.0, 1.0);
        input * (1.0 - depth * 0.5 + depth * 0.5 * (TAU * phase).sin())
    }

    fn vibrato(&mut self, input: f32, rate: f32, depth: f32) -> f32 {
        let phase = self.advance_phase(0, rate);
        let time = 0.025 + depth.clamp(0.0, 0.02) * (TAU * phase).sin();
        let delayed = self.rings[0].read(time, self.sample_rate, true);
        self.rings[0].push(input);
        delayed
    }

    fn downsample(&mut self, input: f32, factor: f32) -> f32 {
        let factor = factor.round().clamp(1.0, 4096.0);
        if self.values[1] <= 0.0 {
            self.values[0] = input;
            self.values[1] = factor;
        }
        self.values[1] -= 1.0;
        self.values[0]
    }

    fn envelope(&mut self, input: f32, attack: f32, release: f32) -> f32 {
        let target = input.abs();
        let time = if target > self.values[0] {
            attack
        } else {
            release
        };
        let coefficient = (-1.0 / (time.max(1.0e-5) * self.sample_rate)).exp();
        self.values[0] = target + coefficient * (self.values[0] - target);
        self.values[0]
    }

    fn compressor(
        &mut self,
        input: f32,
        threshold: f32,
        ratio: f32,
        attack: f32,
        release: f32,
        _hard: bool,
    ) -> f32 {
        let envelope = self.envelope(input, attack, release);
        let threshold = threshold.abs().max(1.0e-5);
        let ratio = ratio.max(1.0);
        let target_gain = if envelope > threshold {
            (threshold + (envelope - threshold) / ratio) / envelope
        } else {
            1.0
        };
        input * target_gain
    }

    fn gate(&mut self, input: f32, threshold: f32, attack: f32, release: f32) -> f32 {
        let target = f32::from(input.abs() >= threshold.abs());
        let time = if target > self.values[0] {
            attack
        } else {
            release
        };
        let coefficient = 1.0 - (-1.0 / (time.max(1.0e-5) * self.sample_rate)).exp();
        self.values[0] += coefficient * (target - self.values[0]);
        input * self.values[0]
    }

    fn slew(&mut self, input: f32, rise: f32, fall: f32) -> f32 {
        let maximum = if input > self.values[0] { rise } else { fall };
        let step = 1.0 / (maximum.max(1.0e-6) * self.sample_rate);
        self.values[0] += (input - self.values[0]).clamp(-step, step);
        self.values[0]
    }

    fn smooth(&mut self, input: f32, time: f32) -> f32 {
        let coefficient = 1.0 - (-1.0 / (time.max(1.0e-6) * self.sample_rate)).exp();
        self.values[0] += coefficient * (input - self.values[0]);
        self.values[0]
    }

    fn sample_hold(&mut self, input: f32, rate: f32) -> f32 {
        if self.values[1] <= 0.0 {
            self.values[0] = input;
            self.values[1] = self.sample_rate / rate.max(0.001);
        }
        self.values[1] -= 1.0;
        self.values[0]
    }

    fn track_hold(&mut self, input: f32, gate: f32) -> f32 {
        if gate > 0.0 {
            self.values[0] = input;
        }
        self.values[0]
    }

    fn early(&mut self, input: f32, size: f32) -> f32 {
        let size = size.clamp(0.1, 2.0);
        let output = 0.42 * self.rings[0].read(0.011 * size, self.sample_rate, true)
            + 0.31 * self.rings[0].read(0.019 * size, self.sample_rate, true)
            + 0.19 * self.rings[0].read(0.031 * size, self.sample_rate, true)
            + 0.12 * self.rings[0].read(0.047 * size, self.sample_rate, true);
        self.rings[0].push(input);
        output
    }

    fn schroeder(&mut self, input: f32, room: f32, decay: f32, damping: f32) -> f32 {
        let room = room.clamp(0.25, 2.0);
        let damping = damping.clamp(0.0, 0.99);
        let times = [0.0297, 0.0371, 0.0411, 0.0437];
        let mut sum = 0.0;
        for (index, time) in times.iter().enumerate() {
            let delayed = self.rings[index].read(time * room, self.sample_rate, true);
            self.values[index] += (1.0 - damping) * (delayed - self.values[index]);
            let feedback = 0.001f32.powf(*time * room / decay.max(0.05));
            self.rings[index].push(input + self.values[index] * feedback);
            sum += delayed * 0.25;
        }
        let mut output = sum;
        for (stage, time) in [0.005, 0.0017].iter().enumerate() {
            let ring = &mut self.rings[4 + stage];
            let delayed = ring.read(time * room, self.sample_rate, true);
            let next = delayed - 0.5 * output;
            ring.push(output + 0.5 * next);
            output = next;
        }
        output
    }

    fn fdn(&mut self, input: f32, size: f32, decay: f32, damping: f32) -> f32 {
        let size = size.clamp(0.25, 2.0);
        let times = [0.037, 0.051, 0.071, 0.089];
        let mut delayed = [0.0; 4];
        for index in 0..4 {
            delayed[index] = self.rings[index].read(times[index] * size, self.sample_rate, true);
            self.values[index] +=
                (1.0 - damping.clamp(0.0, 0.99)) * (delayed[index] - self.values[index]);
        }
        let matrix = [
            self.values[0] + self.values[1] + self.values[2] + self.values[3],
            self.values[0] - self.values[1] + self.values[2] - self.values[3],
            self.values[0] + self.values[1] - self.values[2] - self.values[3],
            self.values[0] - self.values[1] - self.values[2] + self.values[3],
        ];
        for index in 0..4 {
            let feedback = 0.001f32.powf(times[index] * size / decay.max(0.05));
            self.rings[index].push(input * 0.25 + matrix[index] * 0.5 * feedback);
        }
        delayed.iter().sum::<f32>() * 0.25
    }
}
