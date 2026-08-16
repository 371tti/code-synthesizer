import './style.css';
import * as monaco from 'monaco-editor/editor/editor.api';
import 'monaco-editor/editor/contrib/codeAction/browser/codeActionContributions.js';
import 'monaco-editor/editor/contrib/hover/browser/hoverContribution.js';
import 'monaco-editor/editor/contrib/parameterHints/browser/parameterHints.js';
import 'monaco-editor/editor/contrib/snippet/browser/snippetController2.js';
import 'monaco-editor/editor/contrib/suggest/browser/suggestController.js';
import EditorWorker from 'monaco-editor/editor/editor.worker?worker';

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };

const INPUT_DOCS = {
  t: 'ボイス開始からの秒数。', l: 'ノートを離してからの秒数。', s: 'ノートのベロシティ、0–1。',
  freq: 'ベンド適用後の周波数Hz。', note: 'MIDIノート番号。', ch: 'MIDIチャンネル、0–15。',
  bend: 'ピッチベンド、-1–1。', bend_st: '半音単位のピッチベンド。', mw: 'モジュレーションホイール、0–1。',
  vol: 'MIDIチャンネル音量。', midi_pan: 'MIDIパン、-1–1。', mexpr: 'MIDIエクスプレッション、0–1。',
  sustain: 'サステインペダルの状態。', pressure: 'チャンネルプレッシャー。', poly_pressure: 'ノートごとのプレッシャー。',
  program: 'MIDIプログラム番号。', sr: 'ホストのサンプルレートHz。', tempo: 'ホストテンポBPM。',
  beat: '小節内の拍位置。', bar: '現在の小節のPPQ位置。', ppq: 'ホストのPPQ位置。',
  playing: 'ホストの再生中は1。', voice: 'ボイススロット番号。', rand: 'サンプルごとのノイズ、-1–1。',
  wave: 'effect.in.layout = mono の入力。note mixとaudio inputをdownmixした値。',
  wave_l: 'effect.in.layout = stereo の左入力。', wave_r: 'effect.in.layout = stereo の右入力。',
};

const OUTPUT_DOCS = {
  wave: 'mono layout の必須出力。',
  pan: 'note.out.layout = mono でのみ使用できる任意pan、-1–1。',
  l_limit: '必須。ノートを離してからボイスを終了するまでの秒数。',
  wave_l: 'stereo layout の必須左出力。', wave_r: 'stereo layout の必須右出力。',
};

const TOP_LEVEL_DOCS = {
  'note.out.layout': 'note() が生成するvoice出力のlayout。monoまたはstereo。',
  'effect.in.layout': 'effect() が受け取るaudio入力のlayout。monoまたはstereo。',
  'effect.out.layout': 'effect() が返すaudio出力のlayout。monoまたはstereo。',
};

const FUNCTION_DOCS = {
  sin: ['sin(x)', 'Sine.'], cos: ['cos(x)', 'Cosine.'], tan: ['tan(x)', 'Tangent.'],
  sinh: ['sinh(x)', 'Hyperbolic sine.'], cosh: ['cosh(x)', 'Hyperbolic cosine.'],
  exp: ['exp(x)', 'Natural exponential.'], sqrt: ['sqrt(x)', 'Square root.'], cbrt: ['cbrt(x)', 'Cube root.'],
  abs: ['abs(x)', 'Absolute value.'], tanh: ['tanh(x)', 'Soft clipping.'], ln: ['ln(x)', 'Natural logarithm.'],
  log: ['log(x)', 'Natural logarithm.'], log2: ['log2(x)', 'Base-2 logarithm.'], log10: ['log10(x)', 'Base-10 logarithm.'],
  floor: ['floor(x)', 'Round down.'], ceil: ['ceil(x)', 'Round up.'], round: ['round(x)', 'Round nearest.'],
  fract: ['fract(x)', 'Fractional part.'], sign: ['sign(x)', 'Sign.'], asin: ['asin(x)', 'Arc sine.'],
  acos: ['acos(x)', 'Arc cosine.'], atan: ['atan(x)', 'Arc tangent.'], atan2: ['atan2(y, x)', 'Two-argument arc tangent.'],
  min: ['min(a, b)', 'Smaller value.'], max: ['max(a, b)', 'Larger value.'], pow: ['pow(x, y)', 'Power.'],
  mod: ['mod(x, y)', 'Remainder.'], clamp: ['clamp(x, min, max)', 'Clamp into range.'],
  mix: ['mix(a, b, amount)', 'Linear interpolation.'], step: ['step(edge, x)', 'Step.'],
  smoothstep: ['smoothstep(edge0, edge1, x)', 'Smooth Hermite step.'], select: ['select(condition, yes, no)', '0ならfalse側、それ以外はtrue側。'],
  mtof: ['mtof(note)', 'MIDI note to Hz.'], ftom: ['ftom(freq)', 'Hz to MIDI note.'],
  dbtoa: ['dbtoa(db)', 'dB to amplitude.'], atodb: ['atodb(amp)', 'Amplitude to dB.'],
  cent_ratio: ['cent_ratio(cents)', 'Cent ratio.'], semitone_ratio: ['semitone_ratio(st)', 'Semitone ratio.'],
  noise: ['noise()', 'White noise.'],
  saw: ['saw(freq, t)', 'Band-limited saw.'], triangle: ['triangle(freq, t)', 'Triangle oscillator.'],
  square: ['square(freq, t)', '50% duty band-limited square.'], pulse: ['pulse(freq, t, duty)', 'Variable duty pulse.'],
};

Object.assign(FUNCTION_DOCS, {
  'in.cc': ['in.cc(number)', '指定したMIDI CCの現在値、0..1。Entry Point内で使用します。'],
  exp2: ['exp2(x)', '2のx乗。Pitchや周波数比の計算向け。'],
  wrap: ['wrap(x, min, max)', '値を指定範囲へ周期的にwrapします。'],
  hypot: ['hypot(x, y)', 'sqrt(x*x + y*y)を安定して計算します。'],
  sinc: ['sinc(x)', '正規化sinc関数。'],
  hash: ['hash(x)', '入力から決定論的な-1..1の値を生成します。'],
  hash2: ['hash2(x, y)', '2入力から決定論的な-1..1の値を生成します。'],
  fold: ['fold(x, min, max)', '範囲外の値を境界で反射します。'],
  pan_l: ['pan_l(pan)', 'Equal-Power Panの左gain。'],
  pan_r: ['pan_r(pan)', 'Equal-Power Panの右gain。'],
  onepole_coeff: ['onepole_coeff(freq, sr)', '1-pole filterの係数。'],
  'window.hann': ['window.hann(x)', '0..1位置のHann window。'],
  'window.hamming': ['window.hamming(x)', '0..1位置のHamming window。'],
  'window.blackman': ['window.blackman(x)', '0..1位置のBlackman window。'],
  'biquad.lowpass': ['biquad.lowpass(freq, q, sr)', 'b0/b1/b2/a1/a2係数bundleを返します。'],
  'biquad.highpass': ['biquad.highpass(freq, q, sr)', 'High-Pass Biquad係数bundle。'],
  'biquad.bandpass': ['biquad.bandpass(freq, q, sr)', 'Band-Pass Biquad係数bundle。'],
  'biquad.notch': ['biquad.notch(freq, q, sr)', 'Notch Biquad係数bundle。'],
  'biquad.allpass': ['biquad.allpass(freq, q, sr)', 'All-Pass Biquad係数bundle。'],
  'biquad.peak': ['biquad.peak(freq, q, gain_db, sr)', 'Peaking EQ係数bundle。'],
  'biquad.lowshelf': ['biquad.lowshelf(freq, q, gain_db, sr)', 'Low-Shelf EQ係数bundle。'],
  'biquad.highshelf': ['biquad.highshelf(freq, q, gain_db, sr)', 'High-Shelf EQ係数bundle。'],
  'filter.onepole.lp': ['filter.onepole.lp(x, freq, sr)', '自動state付き1-pole Low-Pass。'],
  'filter.onepole.hp': ['filter.onepole.hp(x, freq, sr)', '自動state付き1-pole High-Pass。'],
  'filter.svf.lp': ['filter.svf.lp(x, freq, q, sr)', 'State Variable Low-Pass。'],
  'filter.svf.hp': ['filter.svf.hp(x, freq, q, sr)', 'State Variable High-Pass。'],
  'filter.svf.bp': ['filter.svf.bp(x, freq, q, sr)', 'State Variable Band-Pass。'],
  'filter.svf.notch': ['filter.svf.notch(x, freq, q, sr)', 'State Variable Notch。'],
  'filter.biquad.lp': ['filter.biquad.lp(x, freq, q, sr)', 'Biquad Low-Pass。'],
  'filter.biquad.hp': ['filter.biquad.hp(x, freq, q, sr)', 'Biquad High-Pass。'],
  'filter.biquad.bp': ['filter.biquad.bp(x, freq, q, sr)', 'Biquad Band-Pass。'],
  'filter.biquad.notch': ['filter.biquad.notch(x, freq, q, sr)', 'Biquad Notch。'],
  'filter.biquad.allpass': ['filter.biquad.allpass(x, freq, q, sr)', 'Biquad All-Pass。'],
  'filter.biquad.peak': ['filter.biquad.peak(x, freq, q, gain_db, sr)', 'Peaking EQ。'],
  'filter.biquad.lowshelf': ['filter.biquad.lowshelf(x, freq, gain_db, sr)', 'Low-Shelf EQ。'],
  'filter.biquad.highshelf': ['filter.biquad.highshelf(x, freq, gain_db, sr)', 'High-Shelf EQ。'],
  dc_block: ['dc_block(x)', 'DC offsetを除去します。'],
  'delay.fixed': ['delay.fixed(x, time)', '固定時間Delay。timeは秒です。'],
  'delay.variable': ['delay.variable(x, time)', '線形補間付き可変Delay。'],
  'delay.feedback': ['delay.feedback(x, time, feedback)', 'Feedback付きDelay。'],
  'delay.multitap': ['delay.multitap(x, time1, time2, ...)', 'tap1..tap8 bundleを返します。'],
  'comb.feedforward': ['comb.feedforward(x, time, gain)', 'Feed-Forward Comb Filter。'],
  'comb.feedback': ['comb.feedback(x, time, feedback)', 'Feedback Comb Filter。'],
  allpass: ['allpass(x, time, feedback)', 'Delay型All-Pass Filter。'],
  resonator: ['resonator(x, freq, decay)', '周波数と減衰秒を指定する共鳴器。'],
  'resonator.q': ['resonator.q(x, freq, q)', 'Q指定の共鳴器。'],
  modal: ['modal(x, freq, decay, gain)', '単一Modal Resonance。'],
  'string.karplus': ['string.karplus(x, freq, decay, damping)', 'Karplus-Strong plucked string。'],
  waveguide: ['waveguide(x, delay, feedback, damping)', 'Digital Waveguideの基本要素。'],
  'exciter.impulse': ['exciter.impulse(t, decay)', '短いImpulse励振。'],
  'exciter.noise': ['exciter.noise(t, decay)', '決定論的Noise Burst励振。'],
  chorus: ['chorus(x, rate, depth, delay)', '可変Delay Chorus。depth/delayは秒です。'],
  flanger: ['flanger(x, rate, depth, feedback)', '短い可変Delay Flanger。depthは秒です。'],
  phaser: ['phaser(x, rate, depth, feedback)', '4-stage All-Pass Phaser。'],
  tremolo: ['tremolo(x, rate, depth)', '振幅を周期変調します。'],
  vibrato: ['vibrato(x, rate, depth)', '補間DelayでPitchを周期変調します。depthは秒です。'],
  drive: ['drive(x, amount)', '正規化tanh Overdrive。'],
  saturate: ['saturate(x, amount)', '滑らかなSaturation。'],
  waveshaper: ['waveshaper(x, drive, mix)', 'Dry/Wet付きWaveshaping。'],
  wavefold: ['wavefold(x, amount)', 'Wave Folding。'],
  bitcrush: ['bitcrush(x, bits)', '振幅を指定bit数へ量子化します。'],
  downsample: ['downsample(x, factor)', 'Sample HoldによるSample Rate Reduction。'],
  compressor: ['compressor(x, threshold, ratio, attack, release)', '振幅thresholdのCompressor。時間は秒です。'],
  limiter: ['limiter(x, threshold, attack, release)', '振幅thresholdのLimiter。'],
  gate: ['gate(x, threshold, attack, release)', 'Noise Gate。'],
  envelope_follower: ['envelope_follower(x, attack, release)', '絶対振幅Envelopeを追跡します。'],
  slew: ['slew(x, rise, fall)', '上昇・下降時間で変化速度を制限します。'],
  smooth: ['smooth(x, time)', '値を時間方向に平滑化します。'],
  sample_hold: ['sample_hold(x, rate)', '指定Hzで入力をsampleして保持します。'],
  track_hold: ['track_hold(x, gate)', 'Gate中は追跡し、Gate Offで保持します。'],
  'pan.equal_power': ['pan.equal_power(x, pan)', 'left/right stereo bundleを返します。'],
  'stereo.mid': ['stereo.mid(l, r)', 'StereoのMid成分。'],
  'stereo.side': ['stereo.side(l, r)', 'StereoのSide成分。'],
  'stereo.width': ['stereo.width(l, r, width)', 'left/right幅調整bundleを返します。'],
  'reverb.early': ['reverb.early(x, size)', '複数tapのEarly Reflection。'],
  'reverb.schroeder': ['reverb.schroeder(x, room, decay, damping)', 'Comb + All-Pass Reverb。'],
  'reverb.fdn': ['reverb.fdn(x, size, decay, damping)', '4-line Feedback Delay Network Reverb。'],
});

const RING_METHOD_DOCS = {
  peek: ['peek(delay)', '指定秒数だけ過去を読み、同一sample内のcursorは移動しません。'],
  peek_linear: ['peek_linear(delay)', 'fractional sample位置を線形補間して読みます。'],
  len: ['len()', 'RingBuf容量をsample数で返します。'],
  duration: ['duration()', 'RingBuf容量を秒で返します。'],
};

const FUNCTION_PATTERN = Object.keys(FUNCTION_DOCS)
  .map(name => name.replaceAll('.', '\\.'))
  .join('|');

monaco.languages.register({ id: 'synth-dsl' });

monaco.languages.setMonarchTokensProvider('synth-dsl', {
  tokenizer: {
    root: [
      [/(#|\/\/).*$/, 'comment'],
      [/\bp(?:\.[A-Za-z_][\w]*)+\b/, 'parameter'],
      [/\b(?:TAU|PI|E|PHI)\b/, 'constant'],
      [/\b(?:mono|stereo|layout|effect|fn|f32|RingBuf|voice|note|global|in|out)\b/, 'keyword'],
      [new RegExp(`\\b(?:${FUNCTION_PATTERN}|param)\\b(?=\\s*\\()`), 'function'],
      [/\bin\.[A-Za-z_][\w]*/, 'variable.predefined'],
      [/\bout\.(?:wave|wave_l|wave_r|pan|l_limit)\b/, 'type.identifier'],
      [/[A-Za-z_][\w]*(?:\.[A-Za-z_][\w]*)*/, 'identifier'],
      [/(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?(?:ms|us|s|k|m|u|g)?/i, 'number'],
      [/(?:->|==|!=|<=|>=|[=+\-*\/%^<>])/, 'operator'],
      [/[(){},]/, 'delimiter'],
    ],
  },
});

monaco.languages.setLanguageConfiguration('synth-dsl', {
  wordPattern: /[A-Za-z_][A-Za-z0-9_]*/,
  brackets: [['(', ')'], ['{', '}']],
  autoClosingPairs: [
    { open: '(', close: ')' },
    { open: '{', close: '}' },
  ],
  surroundingPairs: [
    { open: '(', close: ')' },
    { open: '{', close: '}' },
  ],
});

monaco.editor.defineTheme('math-synth', {
  base: 'vs-dark',
  inherit: true,
  rules: [
    { token: 'comment', foreground: '666666', fontStyle: 'italic' },
    { token: 'constant', foreground: 'b8a985' },
    { token: 'function', foreground: 'a9b7e6' },
    { token: 'parameter', foreground: 'd4b6cf' },
    { token: 'variable.predefined', foreground: '91a6b8' },
    { token: 'type.identifier', foreground: 'cccccc', fontStyle: 'bold' },
    { token: 'number', foreground: 'b9aa9d' },
    { token: 'operator', foreground: '999999' },
  ],
  colors: {
    'editor.background': '#1a1a1a',
    'editor.foreground': '#999999',
    'editorLineNumber.foreground': '#444444',
    'editorLineNumber.activeForeground': '#a9b7e6',
    'editorCursor.foreground': '#a9b7e6',
    'editor.selectionBackground': '#6f7a9566',
    'editor.lineHighlightBackground': '#202020',
    'editorWidget.background': '#1a1a1a',
    'editorWidget.border': '#6f7a95',
    'editorSuggestWidget.background': '#1a1a1a',
    'editorSuggestWidget.border': '#6f7a95',
    'editorSuggestWidget.selectedBackground': '#282828',
    'editorHoverWidget.background': '#1a1a1a',
    'editorHoverWidget.border': '#6f7a95',
    'input.background': '#121212',
  },
});

const editor = monaco.editor.create(document.querySelector('#editor'), {
  value: '',
  language: 'synth-dsl',
  theme: 'math-synth',
  automaticLayout: true,
  minimap: { enabled: false },
  contextmenu: false,
  glyphMargin: true,
  folding: false,
  fontFamily: "'UDEV Gothic HSLG', 'Cascadia Code', Consolas, monospace",
  fontSize: 13,
  lineHeight: 21,
  padding: { top: 12, bottom: 12 },
  scrollBeyondLastLine: false,
  smoothScrolling: true,
  bracketPairColorization: { enabled: true },
  guides: { bracketPairs: true, indentation: false },
  overviewRulerBorder: false,
  renderLineHighlight: 'all',
  wordWrap: 'on',
  quickSuggestions: { other: true, comments: false, strings: false },
  quickSuggestionsDelay: 0,
  suggest: {
    showWords: true,
    showSnippets: true,
    preview: true,
    snippetsPreventQuickSuggestions: false,
  },
  snippetSuggestions: 'top',
  wordBasedSuggestions: 'off',
  suggestOnTriggerCharacters: true,
  acceptSuggestionOnEnter: 'on',
  suggestSelection: 'first',
  parameterHints: { enabled: true },
  tabSize: 2,
});

editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Space, () => {
  editor.trigger('synth-dsl', 'editor.action.triggerSuggest', {});
});

function sourceSymbols(model) {
  return [...new Set(
    [...model.getValue().matchAll(/^\s*([A-Za-z_][\w.]*)\s*=/gm)]
      .map(match => match[1]),
  )];
}

function currentFunction(model, position) {
  const source = model.getValueInRange(
    new monaco.Range(1, 1, position.lineNumber, position.column),
  );

  return [
    ...source.matchAll(
      /\bfn\s+([A-Za-z_][\w.]*)\s*\([^)]*\)\s*->\s*out\s*\{/g,
    ),
  ].at(-1)?.[1] || '';
}

function declaredLayout(model, target) {
  const match = model.getValue().match(
    new RegExp(
      `^\\s*${target.replaceAll('.', '\\\\.')}\\s*=\\s*(mono|stereo)\\b`,
      'm',
    ),
  );

  return match?.[1] || 'mono';
}

function hierarchicalSourceSymbols(model) {
  const source = model.getValue();
  const symbols = new Map(
    sourceSymbols(model).map(name => [name, {}]),
  );

  for (
    const match of source.matchAll(
      /\bRingBuf\s*<[^>]+>\s+(?:voice|note|global)\s+([A-Za-z_][\w.]*)/g,
    )
  ) {
    for (
      const [method, [signature, documentation]]
      of Object.entries(RING_METHOD_DOCS)
    ) {
      symbols.set(
        `${match[1]}.${method}`,
        { signature, documentation },
      );
    }
  }

  for (
    const match of source.matchAll(
      /^\s*([A-Za-z_][\w.]*)\s*=\s*biquad\./gm,
    )
  ) {
    for (const field of ['b0', 'b1', 'b2', 'a1', 'a2']) {
      symbols.set(`${match[1]}.${field}`, {});
    }
  }

  for (
    const match of source.matchAll(
      /^\s*([A-Za-z_][\w.]*)\s*=\s*(?:pan\.equal_power|stereo\.width)\s*\(/gm,
    )
  ) {
    symbols.set(`${match[1]}.left`, {});
    symbols.set(`${match[1]}.right`, {});
  }

  for (
    const match of source.matchAll(
      /^\s*([A-Za-z_][\w.]*)\s*=\s*delay\.multitap\s*\(/gm,
    )
  ) {
    for (let index = 1; index <= 8; index += 1) {
      symbols.set(`${match[1]}.tap${index}`, {});
    }
  }

  return symbols;
}

function hasEndpoint(source, name) {
  const escaped = name.replace(/[.*+?^${}()|[\\]\\]/g, '\\$&');
  return new RegExp(`^\\s*fn\\s+${escaped}\\s*\\(`, 'm').test(source);
}

function layoutValueCompletions(model, position) {
  const prefix = model
    .getLineContent(position.lineNumber)
    .slice(0, position.column - 1);

  const match = prefix.match(
    /^\s*(note\.out\.layout|effect\.(?:in|out)\.layout)\s*=\s*([A-Za-z_]*)$/,
  );

  if (!match) return null;

  const fragment = match[2];
  const range = new monaco.Range(
    position.lineNumber,
    position.column - fragment.length,
    position.lineNumber,
    position.column,
  );

  return {
    suggestions: ['mono', 'stereo'].map((layout, index) => ({
      label: layout,
      kind: monaco.languages.CompletionItemKind.EnumMember,
      insertText: layout,
      detail: `${match[1]} = ${layout}`,
      documentation: TOP_LEVEL_DOCS[match[1]],
      range,
      sortText: `0-${index}-${layout}`,
    })),
  };
}

function hierarchicalCompletions(model, position) {
  const layoutValues = layoutValueCompletions(model, position);
  if (layoutValues) return layoutValues;
  const line = model
    .getLineContent(position.lineNumber)
    .slice(0, position.column - 1);

  const typed = line.match(/[A-Za-z_][\w.]*$/)?.[0] || '';
  const dot = typed.lastIndexOf('.');
  const parent = dot >= 0 ? typed.slice(0, dot + 1) : '';
  const fragment = dot >= 0 ? typed.slice(dot + 1) : typed;

  const range = new monaco.Range(
    position.lineNumber,
    position.column - fragment.length,
    position.lineNumber,
    position.column,
  );

  const scope = currentFunction(model, position);
  const noteLayout = declaredLayout(model, 'note.out.layout');
  const effectInputLayout = declaredLayout(model, 'effect.in.layout');
  const effectOutputLayout = declaredLayout(model, 'effect.out.layout');

  const effectScope = scope === 'filter' || scope === 'effect';

  const inputNames = effectScope
    ? [
      ...(effectInputLayout === 'stereo'
        ? ['wave_l', 'wave_r']
        : ['wave']),
      'sr',
      'tempo',
      'beat',
      'bar',
      'ppq',
      'playing',
      'mw',
      'vol',
      'midi_pan',
      'mexpr',
      'sustain',
      'program',
    ]
    : Object.keys(INPUT_DOCS);

  const outputLayout = effectScope
    ? effectOutputLayout
    : noteLayout;

  const outputNames = effectScope
    ? (
      outputLayout === 'stereo'
        ? ['wave_l', 'wave_r']
        : ['wave']
    )
    : (
      outputLayout === 'stereo'
        ? ['wave_l', 'wave_r', 'l_limit']
        : ['wave', 'pan', 'l_limit']
    );

  const descriptors = [];

  for (const [name, documentation] of Object.entries(TOP_LEVEL_DOCS)) {
    descriptors.push({
      name,
      kind: monaco.languages.CompletionItemKind.Property,
      detail: 'Audio layout configuration',
      documentation,
      sort: '0',
    });
  }

  for (const label of inputNames) {
    descriptors.push({
      name: `in.${label}`,
      kind: monaco.languages.CompletionItemKind.Variable,
      detail: '実行時入力',
      documentation: INPUT_DOCS[label],
      sort: '2',
    });
  }

  for (const label of outputNames) {
    descriptors.push({
      name: `out.${label}`,
      kind: monaco.languages.CompletionItemKind.Field,
      detail: 'Entry output',
      documentation: OUTPUT_DOCS[label],
      sort: '1',
    });
  }

  for (
    const [name, [signature, documentation]]
    of Object.entries(FUNCTION_DOCS)
  ) {
    descriptors.push({
      name,
      kind: monaco.languages.CompletionItemKind.Function,
      detail: signature,
      documentation,
      signature,
      sort: '3',
    });
  }

  for (const [name, metadata] of hierarchicalSourceSymbols(model)) {
    descriptors.push({
      name,
      kind: name.startsWith('p.')
        ? monaco.languages.CompletionItemKind.Property
        : monaco.languages.CompletionItemKind.Variable,
      detail: metadata.signature || (
        name.startsWith('p.')
          ? 'ユーザーパラメーター'
          : 'local / qualified value'
      ),
      documentation: metadata.documentation,
      signature: metadata.signature,
      sort: '1',
    });
  }

  const suggestions = [];
  const seen = new Set();

  for (const descriptor of descriptors) {
    if (!descriptor.name.startsWith(parent)) continue;

    const remainder = descriptor.name.slice(parent.length);
    if (!remainder) continue;

    const separator = remainder.indexOf('.');
    const segment = separator >= 0
      ? remainder.slice(0, separator)
      : remainder;

    const hasChildren = separator >= 0;
    const key = `${parent}${segment}${hasChildren ? '.' : ''}`;

    if (seen.has(key)) continue;
    seen.add(key);

    if (hasChildren) {
      suggestions.push({
        label: `${segment}.`,
        kind: monaco.languages.CompletionItemKind.Module,
        insertText: `${segment}.`,
        detail: `${parent}${segment} namespace`,
        range,
        sortText: `0-${segment}`,
        command: {
          id: 'editor.action.triggerSuggest',
          title: '次の候補を表示',
        },
      });

      continue;
    }

    let insertText = segment;
    let insertTextRules;

    if (descriptor.signature) {
      const raw = descriptor.signature.slice(
        descriptor.signature.indexOf('(') + 1,
        -1,
      );

      const args = raw
        ? raw
          .split(', ')
          .map(
            (argument, index) =>
              '${' + (index + 1) + ':' + argument + '}',
          )
          .join(', ')
        : '';

      insertText = `${segment}(${args})`;
      insertTextRules =
        monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet;
    }

    suggestions.push({
      label: segment,
      kind: descriptor.kind,
      insertText,
      insertTextRules,
      detail: descriptor.detail,
      documentation: descriptor.documentation,
      range,
      sortText: `${descriptor.sort}-${segment}`,
    });
  }

  if (!parent) {
    if (!scope) {
      suggestions.unshift({
        label: 'p.',
        kind: monaco.languages.CompletionItemKind.Module,
        insertText: 'p.',
        detail: 'User parameter namespace',
        documentation: 'p.name = param(...) でVST automation対応parameterを宣言します。',
        range,
        sortText: '0-p-namespace',
        command: {
          id: 'editor.action.triggerSuggest',
          title: 'parameter候補を表示',
        },
      });
    }

    if (parent === 'p.' && !scope) {
      suggestions.unshift({
        label: 'new parameter',
        kind: monaco.languages.CompletionItemKind.Snippet,
        insertText: '${1:name} = param(${2:0.5}, ${3:0}, ${4:1}, ${5:0.01}${6:, 74})',
        insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        detail: '新しい p.* VST parameter を宣言',
        documentation: 'トップレベルで p.name = param(default, min, max, step, cc_link?) を宣言します。',
        range,
        sortText: '0-new-parameter',
      });
    }

    suggestions.push({
      label: 'param',
      kind: monaco.languages.CompletionItemKind.Snippet,
      insertText:
        'p.${1:name} = param(${2:0.5}, ${3:0}, ${4:1}, ${5:0.01}${6:, 74})',
      insertTextRules:
        monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
      detail: 'Play mode用VSTパラメーターを宣言',
      documentation:
        'ホストからオートメーションでき、Play modeで自由に配置できるコントロールを作成します。',
      range,
      sortText: '0-param',
    });

    suggestions.push({
      label: 'mono program',
      kind: monaco.languages.CompletionItemKind.Snippet,
      insertText:
        'note.out.layout = mono\n\nfn note(in, p) -> out {\n\tattack = clamp(in.t / ${1:0.008}, 0, 1)\n\trelease = exp(-${2:6} * in.l)\n\tout.wave = in.s * in.vol * in.mexpr * attack * release * ${3|sin(TAU * in.freq * in.t),saw(in.freq, in.t),triangle(in.freq, in.t)|}\n\tout.pan = in.midi_pan\n\tout.l_limit = ${4:1s}\n}',
      insertTextRules:
        monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
      detail: 'mono voice のひな形',
      range,
      sortText: '0-template-mono',
    });

    suggestions.push({
      label: 'stereo program',
      kind: monaco.languages.CompletionItemKind.Snippet,
      insertText:
        'note.out.layout = stereo\n\nfn note(in, p) -> out {\n\tamp = in.s * in.vol * in.mexpr * ${1|sin(TAU * in.freq * in.t),saw(in.freq, in.t),triangle(in.freq, in.t)|}\n\tout.wave_l = amp * pan_l(in.midi_pan)\n\tout.wave_r = amp * pan_r(in.midi_pan)\n\tout.l_limit = ${2:1s}\n}',
      insertTextRules:
        monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
      detail: 'stereo voice のひな形',
      range,
      sortText: '0-template-stereo',
    });
  }

  return { suggestions };
}

monaco.languages.registerCompletionItemProvider('synth-dsl', {
  triggerCharacters: ['.', '_', '('],

  provideCompletionItems(model, position) {
    return hierarchicalCompletions(model, position);
  },
});

monaco.languages.registerHoverProvider('synth-dsl', {
  provideHover(model, position) {
    const word = model.getWordAtPosition(position)?.word;
    if (!word) return null;

    const line = model.getLineContent(position.lineNumber);

    const left =
      line
        .slice(0, position.column - 1)
        .match(/[A-Za-z_][\w.]*$/)?.[0] || '';

    const right =
      line
        .slice(position.column - 1)
        .match(/^[A-Za-z0-9_.]*/)?.[0] || '';

    const qualified = (left + right) || word;

    if (FUNCTION_DOCS[qualified]) {
      return {
        contents: [
          { value: `\`${FUNCTION_DOCS[qualified][0]}\`` },
          { value: FUNCTION_DOCS[qualified][1] },
        ],
      };
    }

    if (INPUT_DOCS[word]) {
      return {
        contents: [
          { value: `**${word}** — 実行時入力` },
          { value: INPUT_DOCS[word] },
        ],
      };
    }

    if (OUTPUT_DOCS[word]) {
      return {
        contents: [
          { value: `**${word}** — output` },
          { value: OUTPUT_DOCS[word] },
        ],
      };
    }

    if (qualified.startsWith('p.')) {
      return {
        contents: [
          {
            value:
              `**${qualified}** — ホストオートメーション対応パラメーター`,
          },
          {
            value:
              'Play modeのコントロールを右クリックして表示形式と配置を変更できます。',
          },
        ],
      };
    }

    return null;
  },
});

monaco.languages.registerSignatureHelpProvider('synth-dsl', {
  signatureHelpTriggerCharacters: ['(', ','],
  signatureHelpRetriggerCharacters: [','],

  provideSignatureHelp(model, position) {
    const prefix = model.getValueInRange(
      new monaco.Range(
        position.lineNumber,
        1,
        position.lineNumber,
        position.column,
      ),
    );

    const name =
      prefix.match(/([A-Za-z_][\w.]*)\s*\([^()]*$/)?.[1];

    const entry = name === 'param'
      ? [
        'param(default, min, max, step, cc_link?)',
        'p.nameとして先頭に宣言するVST parameter。',
      ]
      : FUNCTION_DOCS[name];

    if (!entry) return null;

    const activeParameter = (
      prefix
        .slice(prefix.lastIndexOf('(') + 1)
        .match(/,/g) || []
    ).length;

    const labels = entry[0]
      .slice(entry[0].indexOf('(') + 1, -1)
      .split(', ')
      .filter(Boolean);

    return {
      value: {
        signatures: [{
          label: entry[0],
          documentation: entry[1],
          parameters: labels.map(label => ({ label })),
        }],
        activeSignature: 0,
        activeParameter,
      },
      dispose() { },
    };
  },
});

monaco.languages.registerCodeActionProvider('synth-dsl', {
  provideCodeActions(model, _range, context) {
    const actions = [];

    for (const marker of context.markers) {
      const suggestion =
        marker.message.match(/Did you mean `([^`]+)`/i)?.[1];

      const word = model.getWordAtPosition({
        lineNumber: marker.startLineNumber,
        column: marker.startColumn,
      });

      if (suggestion) {
        actions.push({
          title: `「${suggestion}」に置き換える`,
          kind: 'quickfix',
          isPreferred: true,
          edit: {
            edits: [{
              resource: model.uri,
              versionId: model.getVersionId(),
              textEdit: {
                range: word
                  ? new monaco.Range(
                    marker.startLineNumber,
                    word.startColumn,
                    marker.startLineNumber,
                    word.endColumn,
                  )
                  : new monaco.Range(
                    marker.startLineNumber,
                    marker.startColumn,
                    marker.endLineNumber,
                    marker.endColumn,
                  ),
                text: suggestion,
              },
            }],
          },
        });
      }
    }

    return {
      actions,
      dispose() { },
    };
  },
});

// -----------------------------------------------------------------------------
// UI runtime
// -----------------------------------------------------------------------------

const $ = selector => document.querySelector(selector);
const $$ = selector => [...document.querySelectorAll(selector)];

const clamp01 = value =>
  Math.max(0, Math.min(1, Number(value) || 0));

const cloneControls = controls =>
  (controls || []).map(control => ({ ...control }));

const app = $('#app');
const presetSelect = $('#preset');
const statusElement = $('#status');
const diagnostic = $('#diagnostic');
const diagnosticMessage = $('#diagnostic-message');
const stage = $('#control-stage');
const contextMenu = $('#context-menu');
const scope = $('#scope');

const CUSTOM_PRESET_STORAGE_KEY =
  'code-synthesizer.custom-presets.v1';

const POLL_INTERVAL_MS = 80;
const SCOPE_INTERVAL_MS = 40;
const COMPILE_DEBOUNCE_MS = 260;

const ui = {
  initialized: false,
  pollInFlight: false,
  latestState: null,
  mode: 'editor',

  endpoints: {
    hasNote: null,
  },

  editor: {
    applyingRemote: false,
    dirty: false,
    compileTimer: 0,
    suggestTimer: 0,
    submittedSource: null,
    lastBackendSource: null,
    deferredRemoteSource: null,
    pendingLoad: null,
  },

  status: {
    lastGeneration: -1,
  },

  layout: {
    arranging: false,
    interacting: false,
    fingerprint: '',
    pendingFingerprint: null,
    pendingControls: null,
    initialChecked: false,
    initialRepairJustApplied: false,
  },

  presets: {
    factory: [],
    custom: [],
    pendingConfirm: null,
    deleteArmed: '',
    deleteTimer: 0,
  },

  copiedParameter: null,
  toastTimer: 0,
};

function send(message) {
  window.ipc?.postMessage?.(JSON.stringify(message));
}

function showToast(message) {
  const toast = $('#toast');
  if (!toast) return;

  toast.textContent = message;
  toast.hidden = false;

  clearTimeout(ui.toastTimer);

  ui.toastTimer = setTimeout(() => {
    toast.hidden = true;
  }, 1800);
}

function openModal(id) {
  const modal = $(id);
  if (!modal) return;

  modal.hidden = false;
  modal.setAttribute('aria-hidden', 'false');
}

function closeModal(id) {
  const modal = $(id);
  if (!modal) return;

  modal.hidden = true;
  modal.setAttribute('aria-hidden', 'true');
}

function syncEndpointUi(source = editor.getValue()) {
  const hasNote = hasEndpoint(String(source ?? ''), 'note');
  const changed = ui.endpoints.hasNote !== hasNote;
  ui.endpoints.hasNote = hasNote;

  app.dataset.hasNote = hasNote ? 'true' : 'false';

  const keyboardSection = $('.keyboard-section');
  if (keyboardSection) keyboardSection.hidden = !hasNote;

  const previewButton = $('#preview-button');
  if (previewButton) previewButton.hidden = !hasNote;

  const scopeNote = $('#scope-note');
  if (scopeNote) scopeNote.hidden = !hasNote;

  if (changed && !hasNote) {
    releaseAllNotes();
  }

  if (changed && hasNote) {
    requestAnimationFrame(() => buildKeyboard());
  }
}

// -----------------------------------------------------------------------------
// Editor source ownership
// -----------------------------------------------------------------------------
// The editor owns source text while the user is editing. Polling never calls
// setValue() just because the backend is one poll behind. Backend -> editor
// replacement is permitted only for initial load, an explicit preset load, or
// a host-side state change while the editor is clean and unfocused.

function replaceEditorSource(
  source,
  {
    clean = true,
    resetView = false,
  } = {},
) {
  const next = String(source ?? '');
  const model = editor.getModel();

  if (!model) return;

  if (model.getValue() !== next) {
    const viewState = resetView
      ? null
      : editor.saveViewState();

    ui.editor.applyingRemote = true;

    try {
      editor.setValue(next);
    } finally {
      ui.editor.applyingRemote = false;
    }

    if (resetView) {
      editor.setPosition({
        lineNumber: 1,
        column: 1,
      });

      editor.setScrollTop(0);
      editor.setScrollLeft(0);
    } else if (viewState) {
      editor.restoreViewState(viewState);
    }
  }

  syncEndpointUi(next);

  if (clean) {
    ui.editor.dirty = false;
  }
}

function scheduleCompile() {
  clearTimeout(ui.editor.compileTimer);

  statusElement.className = 'compile-status pending';
  statusElement.textContent = 'Compiling…';

  diagnostic.className = 'editor-foot';
  diagnosticMessage.textContent = '式を確認中…';

  ui.editor.compileTimer = setTimeout(
    () => compileNow(false),
    COMPILE_DEBOUNCE_MS,
  );
}

function compileNow(preview = false) {
  clearTimeout(ui.editor.compileTimer);

  const source = editor.getValue();

  ui.editor.submittedSource = source;

  statusElement.className = 'compile-status pending';
  statusElement.textContent = 'Compiling…';

  send({
    cmd: 'setExpression',
    source,
  });

  if (preview) {
    previewNote();
  }
}

function remoteLoadIsReady(state) {
  const pending = ui.editor.pendingLoad;

  if (!pending) return false;
  if (pending.kind !== 'factory') return true;

  const selected =
    state.selectedPreset === pending.name;

  const generation =
    Number(state.status?.generation ?? -1);

  return selected && (
    generation > pending.generationAtRequest ||
    state.source !== pending.sourceAtRequest
  );
}

function reconcileEditorSource(state) {
  const backendSource = String(state.source ?? '');
  const currentSource = editor.getValue();

  if (!ui.initialized) {
    replaceEditorSource(
      backendSource,
      {
        clean: true,
        resetView: true,
      },
    );

    ui.editor.lastBackendSource = backendSource;
    ui.editor.deferredRemoteSource = null;

    return;
  }

  // Explicit factory preset load: this is the only normal polling path allowed
  // to replace a focused editor.
  if (
    ui.editor.pendingLoad &&
    remoteLoadIsReady(state)
  ) {
    replaceEditorSource(
      backendSource,
      {
        clean: true,
        resetView: true,
      },
    );

    ui.editor.pendingLoad = null;
    ui.editor.submittedSource = null;
    ui.editor.lastBackendSource = backendSource;
    ui.editor.deferredRemoteSource = null;

    return;
  }

  // Acknowledge our own submitted source without touching Monaco.
  if (backendSource === currentSource) {
    ui.editor.lastBackendSource = backendSource;
    ui.editor.deferredRemoteSource = null;
    ui.editor.submittedSource = null;
    ui.editor.dirty = false;

    return;
  }

  if (
    ui.editor.submittedSource &&
    backendSource === ui.editor.submittedSource
  ) {
    ui.editor.lastBackendSource = backendSource;
    ui.editor.submittedSource = null;

    // The user may already have typed more. Never roll back to the submitted
    // snapshot in that case.
    return;
  }

  if (
    backendSource === ui.editor.lastBackendSource
  ) {
    return;
  }

  // A genuinely external source change (e.g. project/state restore). If local
  // editing is active, remember it but never overwrite the user's buffer.
  if (
    ui.editor.dirty ||
    editor.hasTextFocus()
  ) {
    ui.editor.deferredRemoteSource = backendSource;
    return;
  }

  replaceEditorSource(
    backendSource,
    {
      clean: true,
      resetView: false,
    },
  );

  ui.editor.lastBackendSource = backendSource;
  ui.editor.deferredRemoteSource = null;
}

editor.onDidChangeModelContent(event => {
  if (
    !ui.initialized ||
    ui.editor.applyingRemote
  ) {
    return;
  }

  ui.editor.dirty = true;
  ui.editor.deferredRemoteSource = null;
  syncEndpointUi(editor.getValue());

  if (
    editor.hasTextFocus() &&
    event.changes.some(
      change =>
        change.text.length === 1 &&
        /[A-Za-z_.]/.test(change.text),
    )
  ) {
    clearTimeout(ui.editor.suggestTimer);

    ui.editor.suggestTimer = setTimeout(
      () =>
        editor.trigger(
          'keyboard',
          'editor.action.triggerSuggest',
          {},
        ),
      0,
    );
  }

  scheduleCompile();
});

editor.addCommand(
  monaco.KeyMod.CtrlCmd |
  monaco.KeyCode.Enter,
  () => compileNow(true),
);

editor.addCommand(
  monaco.KeyMod.CtrlCmd |
  monaco.KeyCode.KeyS,
  () => compileNow(false),
);

$('#compile-button').addEventListener(
  'click',
  () => compileNow(false),
);

$('#preview-button').addEventListener(
  'click',
  () => compileNow(true),
);

// -----------------------------------------------------------------------------
// Status / host state polling
// -----------------------------------------------------------------------------

function renderStatus(status) {
  if (!status) return;

  const generation =
    Number(status.generation ?? -1);

  if (
    generation <
    ui.status.lastGeneration
  ) {
    return;
  }

  ui.status.lastGeneration = generation;

  const warnings =
    Array.isArray(status.warnings)
      ? status.warnings
      : [];

  const level = status.ok
    ? (
      warnings.length
        ? 'warning'
        : 'ok'
    )
    : 'error';

  const parallel =
    Boolean(status.parallelVoiceSafe);

  statusElement.className =
    `compile-status ${level}`;

  statusElement.textContent = status.ok
    ? `● Compiled · ${parallel
      ? 'Parallel'
      : 'Serial'
    } · Generation ${generation}${warnings.length
      ? ` · ${warnings.length} warning${warnings.length === 1
        ? ''
        : 's'
      }`
      : ''
    }`
    : `● ${status.line}:${status.column} ${status.message}`;

  diagnostic.className =
    `editor-foot ${level}`;

  diagnosticMessage.textContent = status.ok
    ? (
      warnings[0] ||
      (
        parallel
          ? 'Ready · worker parallel evaluation enabled'
          : 'Ready · serial JIT evaluation'
      )
    )
    : `${status.line}:${status.column} ${status.message}${status.hint
      ? ` — ${status.hint}`
      : ''
    }`;

  diagnosticMessage.title = status.ok
    ? warnings.join('\n')
    : (status.hint || '');

  const markers = status.ok
    ? (
      warnings.length
        ? [{
          startLineNumber: 1,
          startColumn: 1,
          endLineNumber: 1,
          endColumn: 2,
          message: warnings.join('\n'),
          severity:
            monaco.MarkerSeverity.Warning,
          source: 'Code Synthesizer',
        }]
        : []
    )
    : [{
      startLineNumber:
        Math.max(
          1,
          Number(status.line) || 1,
        ),

      startColumn:
        Math.max(
          1,
          Number(status.column) || 1,
        ),

      endLineNumber:
        Math.max(
          1,
          Number(status.line) || 1,
        ),

      endColumn:
        Math.max(
          2,
          (Number(status.column) || 1) + 1,
        ),

      message: status.hint
        ? `${status.message}\nHint: ${status.hint}`
        : status.message,

      severity:
        monaco.MarkerSeverity.Error,

      source:
        'Code Synthesizer',
    }];

  monaco.editor.setModelMarkers(
    editor.getModel(),
    'synth-compiler',
    markers,
  );
}

diagnosticMessage.addEventListener(
  'click',
  () => {
    const status =
      ui.latestState?.status;

    if (status?.ok !== false) {
      return;
    }

    editor.setPosition({
      lineNumber:
        Math.max(
          1,
          Number(status.line) || 1,
        ),

      column:
        Math.max(
          1,
          Number(status.column) || 1,
        ),
    });

    editor.focus();
  },
);

function setMode(
  mode,
  persist = true,
) {
  ui.mode =
    mode === 'play'
      ? 'play'
      : 'editor';

  app.dataset.mode = ui.mode;

  $$('.mode-button').forEach(
    button => {
      button.classList.toggle(
        'active',
        button.dataset.mode === ui.mode,
      );
    },
  );

  if (persist) {
    send({
      cmd: 'setMode',
      mode: ui.mode,
    });
  }

  requestAnimationFrame(
    () => editor.layout(),
  );
}

$$('.mode-button').forEach(
  button => {
    button.addEventListener(
      'click',
      () =>
        setMode(
          button.dataset.mode,
        ),
    );
  },
);

async function pollState() {
  if (ui.pollInFlight) {
    return;
  }

  ui.pollInFlight = true;

  try {
    const response =
      await fetch(
        './api/state',
        {
          cache: 'no-store',
        },
      );

    if (!response.ok) {
      throw new Error(
        `state HTTP ${response.status}`,
      );
    }

    const state =
      await response.json();

    // Preserve local layout while a pointer interaction is in progress.
    if (
      ui.layout.interacting &&
      ui.latestState?.controls
    ) {
      state.controls =
        cloneControls(
          ui.latestState.controls,
        );
    }

    reconcileEditorSource(state);

    ui.latestState = state;

    if (!ui.initialized) {
      rebuildPresetOptions(
        state.presets,
      );

      setMode(
        state.mode || 'editor',
        false,
      );

      ui.initialized = true;
    }

    syncPresetSelection(state);
    renderStatus(state.status);

    $('#sample-rate-badge').textContent =
      `${Math.round(
        state.sampleRate || 48000,
      ).toLocaleString()
      } Hz`;

    $('#voice-badge').textContent =
      `${state.activeVoices || 0
      } voice${state.activeVoices === 1
        ? ''
        : 's'
      }`;

    $('#scope-note').textContent =
      midiName(
        state.previewNote ?? 60,
      );

    syncMidiPreview(
      state.activeNotes || [],
      state.releaseNotes || [],
    );

    if (!ui.layout.interacting) {
      syncControls(
        state.parameters || [],
        state.controls || [],
      );
    }
  } catch (error) {
    statusElement.className =
      'compile-status error';

    statusElement.textContent =
      'UIブリッジを利用できません';

    console.error(error);
  } finally {
    ui.pollInFlight = false;
  }
}

// -----------------------------------------------------------------------------
// Presets
// -----------------------------------------------------------------------------

function loadCustomPresetLibrary() {
  try {
    const parsed =
      JSON.parse(
        localStorage.getItem(
          CUSTOM_PRESET_STORAGE_KEY,
        ) || '[]',
      );

    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed
      .filter(
        item =>
          item &&
          typeof item.name === 'string' &&
          typeof item.source === 'string',
      )
      .slice(0, 100)
      .map(item => ({
        name:
          item.name.slice(0, 60),

        source:
          item.source,

        parameterValues:
          Array.isArray(
            item.parameterValues,
          )
            ? item.parameterValues
            : [],

        controls:
          Array.isArray(
            item.controls,
          )
            ? item.controls
            : [],
      }));
  } catch {
    return [];
  }
}

function persistCustomPresetLibrary() {
  try {
    localStorage.setItem(
      CUSTOM_PRESET_STORAGE_KEY,
      JSON.stringify(
        ui.presets.custom,
      ),
    );

    return true;
  } catch {
    showToast(
      'Custom presetを保存できませんでした',
    );

    return false;
  }
}

ui.presets.custom =
  loadCustomPresetLibrary();

function presetOptionValue(
  kind,
  name = '',
) {
  return `${kind}:${name}`;
}

function parsePresetOption(value) {
  const text =
    String(value || '');

  const separator =
    text.indexOf(':');

  return {
    kind:
      separator < 0
        ? 'factory'
        : text.slice(
          0,
          separator,
        ),

    name:
      separator < 0
        ? text
        : text.slice(
          separator + 1,
        ),
  };
}

function rebuildPresetOptions(
  presets = ui.presets.factory,
) {
  ui.presets.factory =
    Array.isArray(presets)
      ? presets
      : [];

  const fragment =
    document.createDocumentFragment();

  const categories =
    new Map();

  for (
    const preset
    of ui.presets.factory
  ) {
    const category =
      preset.category ||
      'Factory';

    if (
      !categories.has(category)
    ) {
      categories.set(
        category,
        [],
      );
    }

    categories
      .get(category)
      .push(preset);
  }

  for (
    const [category, items]
    of categories
  ) {
    const group =
      document.createElement(
        'optgroup',
      );

    group.label = category;

    for (const preset of items) {
      group.append(
        Object.assign(
          document.createElement(
            'option',
          ),
          {
            value:
              presetOptionValue(
                'factory',
                preset.name,
              ),

            textContent:
              preset.name,
          },
        ),
      );
    }

    fragment.append(group);
  }

  const customGroup =
    document.createElement(
      'optgroup',
    );

  customGroup.label = 'Custom';

  customGroup.append(
    Object.assign(
      document.createElement(
        'option',
      ),
      {
        value:
          presetOptionValue(
            'unsaved',
          ),

        textContent:
          'Unsaved Code',
      },
    ),
  );

  for (
    const preset
    of ui.presets.custom
  ) {
    customGroup.append(
      Object.assign(
        document.createElement(
          'option',
        ),
        {
          value:
            presetOptionValue(
              'custom',
              preset.name,
            ),

          textContent:
            preset.name,
        },
      ),
    );
  }

  fragment.append(customGroup);
  presetSelect.replaceChildren(fragment);
}

function selectionForState(state) {
  if (ui.editor.dirty) {
    return presetOptionValue(
      'unsaved',
    );
  }

  const custom =
    ui.presets.custom.find(
      preset =>
        preset.source ===
        editor.getValue(),
    );

  if (custom) {
    return presetOptionValue(
      'custom',
      custom.name,
    );
  }

  if (
    state.selectedPreset &&
    state.selectedPreset !== 'Custom'
  ) {
    return presetOptionValue(
      'factory',
      state.selectedPreset,
    );
  }

  return presetOptionValue(
    'unsaved',
  );
}

function syncPresetSelection(state) {
  const value =
    selectionForState(state);

  if (
    [...presetSelect.options]
      .some(
        option =>
          option.value === value,
      )
  ) {
    presetSelect.value = value;
  }

  const selected =
    parsePresetOption(
      presetSelect.value,
    );

  const custom =
    selected.kind === 'custom';

  const nameInput =
    $('#custom-preset-name');

  const deleteButton =
    $('#delete-custom-preset');

  if (
    custom &&
    document.activeElement !==
    nameInput
  ) {
    nameInput.value =
      selected.name;
  }

  deleteButton.disabled =
    !custom;

  if (!custom) {
    ui.presets.deleteArmed = '';
    deleteButton.textContent =
      'Delete';
  }
}

function presetRequest(value) {
  const parsed =
    parsePresetOption(value);

  if (
    parsed.kind === 'factory'
  ) {
    return ui.presets.factory
      .some(
        preset =>
          preset.name ===
          parsed.name,
      )
      ? {
        kind: 'factory',
        name: parsed.name,
        label: parsed.name,
      }
      : null;
  }

  if (
    parsed.kind === 'custom'
  ) {
    const preset =
      ui.presets.custom.find(
        candidate =>
          candidate.name ===
          parsed.name,
      );

    return preset
      ? {
        kind: 'custom',
        name: preset.name,
        label: preset.name,
        source: preset.source,
        parameterValues:
          preset.parameterValues,
        controls:
          preset.controls,
      }
      : null;
  }

  return null;
}

function resetLayoutSyncState() {
  ui.layout.initialChecked =
    false;

  ui.layout.initialRepairJustApplied =
    false;

  ui.layout.fingerprint =
    '';

  ui.layout.pendingFingerprint =
    null;

  ui.layout.pendingControls =
    null;
}

function performPresetLoad(
  request,
  switchToEditor = false,
) {
  if (!request) return;

  resetLayoutSyncState();

  clearTimeout(
    ui.editor.compileTimer,
  );

  if (
    request.kind === 'custom'
  ) {
    replaceEditorSource(
      request.source,
      {
        clean: true,
        resetView: true,
      },
    );

    ui.editor.lastBackendSource =
      null;

    ui.editor.pendingLoad =
      null;

    send({
      cmd: 'loadCustomPreset',
      source: request.source,
      parameterValues:
        request.parameterValues ||
        [],
      controls:
        request.controls ||
        [],
    });
  } else {
    ui.editor.pendingLoad = {
      kind: 'factory',
      name: request.name,
      generationAtRequest:
        Number(
          ui.latestState
            ?.status
            ?.generation ?? -1,
        ),
      sourceAtRequest:
        String(
          ui.latestState
            ?.source ?? '',
        ),
    };

    send({
      cmd: 'loadPreset',
      name: request.name,
    });
  }

  $('#custom-preset-name').value =
    request.kind === 'custom'
      ? request.name
      : '';

  if (switchToEditor) {
    setMode('editor');
  }
}

function closePresetConfirm() {
  closeModal(
    '#preset-confirm',
  );

  ui.presets.pendingConfirm =
    null;

  if (ui.latestState) {
    syncPresetSelection(
      ui.latestState,
    );
  }
}

function openPresetConfirm(
  request,
  switchToEditor,
) {
  ui.presets.pendingConfirm = {
    request,
    switchToEditor,
  };

  $('#preset-confirm-name')
    .textContent =
    request.label;

  openModal(
    '#preset-confirm',
  );

  setTimeout(
    () =>
      $('#confirm-preset-load')
        .focus(),
    0,
  );
}

function loadPreset(
  value,
  switchToEditor = false,
) {
  const normalized =
    String(value).includes(':')
      ? value
      : presetOptionValue(
        'factory',
        value,
      );

  const request =
    presetRequest(normalized);

  if (!request) {
    if (ui.latestState) {
      syncPresetSelection(
        ui.latestState,
      );
    }

    return;
  }

  if (ui.editor.dirty) {
    openPresetConfirm(
      request,
      switchToEditor,
    );

    return;
  }

  performPresetLoad(
    request,
    switchToEditor,
  );
}

presetSelect.addEventListener(
  'change',
  () => {
    const selected =
      parsePresetOption(
        presetSelect.value,
      );

    if (
      selected.kind === 'custom'
    ) {
      $('#custom-preset-name')
        .value =
        selected.name;
    }

    loadPreset(
      presetSelect.value,
    );
  },
);

$('#load-preset')
  .addEventListener(
    'click',
    () =>
      loadPreset(
        presetSelect.value,
      ),
  );

$('#cancel-preset-load')
  .addEventListener(
    'click',
    closePresetConfirm,
  );

$('#confirm-preset-load')
  .addEventListener(
    'click',
    () => {
      const pending =
        ui.presets.pendingConfirm;

      closePresetConfirm();

      if (pending) {
        performPresetLoad(
          pending.request,
          pending.switchToEditor,
        );
      }
    },
  );

$('#preset-confirm')
  .addEventListener(
    'pointerdown',
    event => {
      if (
        event.target.id ===
        'preset-confirm'
      ) {
        closePresetConfirm();
      }
    },
  );

$('#save-custom-preset')
  .addEventListener(
    'click',
    () => {
      const name =
        $('#custom-preset-name')
          .value
          .trim();

      if (!name) {
        showToast(
          'Preset名を入力してください',
        );

        $('#custom-preset-name')
          .focus();

        return;
      }

      const source =
        editor.getValue();

      const parameterValues =
        (
          ui.latestState
            ?.parameters || []
        ).map(
          parameter => ({
            name:
              parameter.name,

            normalized:
              parameter.normalized,
          }),
        );

      const controls =
        cloneControls(
          ui.latestState
            ?.controls,
        );

      const existing =
        ui.presets.custom.find(
          preset =>
            preset.name === name,
        );

      if (existing) {
        Object.assign(
          existing,
          {
            source,
            parameterValues,
            controls,
          },
        );
      } else {
        ui.presets.custom.push({
          name,
          source,
          parameterValues,
          controls,
        });
      }

      ui.presets.custom.sort(
        (a, b) =>
          a.name.localeCompare(
            b.name,
            'ja',
          ),
      );

      if (
        !persistCustomPresetLibrary()
      ) {
        return;
      }

      rebuildPresetOptions();

      presetSelect.value =
        presetOptionValue(
          'custom',
          name,
        );

      $('#delete-custom-preset')
        .disabled =
        false;

      compileNow(false);

      ui.editor.dirty =
        false;

      showToast(
        existing
          ? 'Custom presetを更新しました'
          : 'Custom presetを保存しました',
      );
    },
  );

$('#delete-custom-preset')
  .addEventListener(
    'click',
    () => {
      const selected =
        parsePresetOption(
          presetSelect.value,
        );

      if (
        selected.kind !==
        'custom'
      ) {
        return;
      }

      const button =
        $('#delete-custom-preset');

      if (
        ui.presets.deleteArmed !==
        selected.name
      ) {
        ui.presets.deleteArmed =
          selected.name;

        button.textContent =
          'Confirm delete';

        clearTimeout(
          ui.presets.deleteTimer,
        );

        ui.presets.deleteTimer =
          setTimeout(
            () => {
              ui.presets.deleteArmed =
                '';

              button.textContent =
                'Delete';
            },
            3000,
          );

        return;
      }

      ui.presets.custom =
        ui.presets.custom.filter(
          preset =>
            preset.name !==
            selected.name,
        );

      if (
        !persistCustomPresetLibrary()
      ) {
        return;
      }

      clearTimeout(
        ui.presets.deleteTimer,
      );

      ui.presets.deleteArmed =
        '';

      rebuildPresetOptions();

      presetSelect.value =
        presetOptionValue(
          'unsaved',
        );

      $('#custom-preset-name')
        .value =
        '';

      button.textContent =
        'Delete';

      button.disabled =
        true;

      showToast(
        'Custom presetを削除しました',
      );
    },
  );

// -----------------------------------------------------------------------------
// Performance controls / layout
// -----------------------------------------------------------------------------

function decimalsFor(step) {
  if (!step) return 3;
  if (step >= 1) return 0;

  return Math.min(
    4,
    Math.max(
      1,
      Math.ceil(
        -Math.log10(step),
      ),
    ),
  );
}

function normalizedDefault(spec) {
  const span =
    spec.max - spec.min;

  return span === 0
    ? 0
    : clamp01(
      (spec.default - spec.min) /
      span,
    );
}

function currentSpec(index) {
  return ui.latestState
    ?.parameters
    ?.find(
      parameter =>
        parameter.index === index,
    );
}

function displayValue(
  spec,
  normalized,
) {
  let value =
    spec.min +
    normalized *
    (spec.max - spec.min);

  if (spec.step > 0) {
    value =
      spec.min +
      Math.round(
        (value - spec.min) /
        spec.step,
      ) *
      spec.step;
  }

  return value.toFixed(
    decimalsFor(spec.step),
  );
}

function controlFingerprint(
  controls,
) {
  return JSON.stringify(
    (controls || []).map(
      control => [
        control.name,
        control.kind,
        control.x,
        control.y,
        control.width,
        control.height,
      ],
    ),
  );
}

function repairInitialLayout(
  controls,
) {
  if (
    ui.layout.initialChecked ||
    controls.length <= 1
  ) {
    return controls;
  }

  ui.layout.initialChecked =
    true;

  const overlaps =
    controls.some(
      (left, index) =>
        controls
          .slice(index + 1)
          .some(
            right =>
              left.x <
              right.x +
              right.width &&
              left.x +
              left.width >
              right.x &&
              left.y <
              right.y +
              right.height &&
              left.y +
              left.height >
              right.y,
          ),
    );

  if (!overlaps) {
    return controls;
  }

  const repaired =
    cloneControls(controls);

  const columns =
    Math.min(
      4,
      Math.max(
        2,
        Math.ceil(
          Math.sqrt(
            repaired.length,
          ),
        ),
      ),
    );

  const width =
    Math.min(
      22,
      96 / columns,
    );

  repaired.forEach(
    (control, index) => {
      const column =
        index % columns;

      const row =
        Math.floor(
          index / columns,
        );

      Object.assign(
        control,
        {
          x:
            2 +
            column *
            (width + 2),

          y:
            4 +
            row * 28,

          width,

          height:
            22,
        },
      );
    },
  );

  ui.layout.pendingControls =
    cloneControls(repaired);

  ui.layout.pendingFingerprint =
    controlFingerprint(repaired);

  ui.layout.fingerprint =
    '';

  ui.layout.initialRepairJustApplied =
    true;

  send({
    cmd: 'setLayout',
    controls:
      ui.layout.pendingControls,
  });

  return repaired;
}

function reconcileLayoutControls(
  controls,
) {
  let next =
    repairInitialLayout(
      cloneControls(controls),
    );

  let fingerprint =
    controlFingerprint(next);

  if (
    ui.layout.pendingFingerprint &&
    fingerprint !==
    ui.layout.pendingFingerprint &&
    ui.layout.pendingControls
  ) {
    next =
      cloneControls(
        ui.layout.pendingControls,
      );

    fingerprint =
      ui.layout.pendingFingerprint;

    if (ui.latestState) {
      ui.latestState.controls =
        cloneControls(next);
    }
  } else if (
    ui.layout.pendingFingerprint ===
    fingerprint
  ) {
    if (
      ui.layout
        .initialRepairJustApplied
    ) {
      ui.layout
        .initialRepairJustApplied =
        false;
    } else {
      ui.layout.pendingFingerprint =
        null;

      ui.layout.pendingControls =
        null;
    }
  }

  return {
    controls: next,
    fingerprint,
  };
}

function syncControls(
  parameters,
  controls,
) {
  const reconciled =
    reconcileLayoutControls(
      controls,
    );

  if (
    reconciled.fingerprint !==
    ui.layout.fingerprint
  ) {
    ui.layout.fingerprint =
      reconciled.fingerprint;

    renderControls(
      parameters,
      reconciled.controls,
    );
  }

  for (const spec of parameters) {
    updateControlValue(
      spec.index,
      spec.normalized,
      spec,
    );
  }

  $('#empty-controls').hidden =
    parameters.length > 0;

  $('#parameter-count')
    .textContent =
    `${parameters.length} parameter${parameters.length === 1
      ? ''
      : 's'
    }`;
}

function renderControls(
  parameters,
  controls,
) {
  stage
    .querySelectorAll(
      '.parameter-control',
    )
    .forEach(
      element =>
        element.remove(),
    );

  const byName =
    new Map(
      parameters.map(
        parameter => [
          parameter.name,
          parameter,
        ],
      ),
    );

  for (
    const layout
    of controls
  ) {
    const spec =
      byName.get(
        layout.name,
      );

    if (!spec) continue;

    const control =
      document.createElement(
        'div',
      );

    control.className =
      'parameter-control';

    control.dataset.name =
      spec.name;

    control.dataset.index =
      spec.index;

    Object.assign(
      control.style,
      {
        left:
          `${layout.x}%`,

        top:
          `${layout.y}%`,

        width:
          `${layout.width}%`,

        height:
          `${layout.height}%`,
      },
    );

    const label =
      Object.assign(
        document.createElement(
          'div',
        ),
        {
          className:
            'control-label',

          textContent:
            spec.label,

          title:
            spec.name,
        },
      );

    const value =
      Object.assign(
        document.createElement(
          'output',
        ),
        {
          className:
            'control-value',
        },
      );

    if (
      layout.kind === 'slider'
    ) {
      const input =
        Object.assign(
          document.createElement(
            'input',
          ),
          {
            type: 'range',
            min: '0',
            max: '1',
            step: '0.001',
            className:
              'parameter-slider',
          },
        );

      input.addEventListener(
        'input',
        () =>
          setParameter(
            spec.index,
            Number(input.value),
          ),
      );

      control.append(
        label,
        input,
        value,
      );
    } else if (
      layout.kind === 'toggle'
    ) {
      const toggle =
        Object.assign(
          document.createElement(
            'button',
          ),
          {
            className:
              'parameter-toggle',

            title:
              'トグル',
          },
        );

      toggle.addEventListener(
        'click',
        () => {
          if (
            ui.layout.arranging
          ) {
            return;
          }

          const fresh =
            currentSpec(
              spec.index,
            );

          if (fresh) {
            setParameter(
              spec.index,
              fresh.normalized >= 0.5
                ? 0
                : 1,
            );
          }
        },
      );

      control.append(
        label,
        toggle,
        value,
      );
    } else {
      const knob =
        Object.assign(
          document.createElement(
            'div',
          ),
          {
            className: 'knob',
            role: 'slider',
            tabIndex: 0,
          },
        );

      installKnob(
        knob,
        spec.index,
      );

      control.append(
        label,
        knob,
        value,
      );
    }

    control.addEventListener(
      'dblclick',
      event => {
        if (
          ui.layout.arranging
        ) {
          return;
        }

        event.preventDefault();

        const fresh =
          currentSpec(
            spec.index,
          );

        if (fresh) {
          setParameter(
            spec.index,
            normalizedDefault(
              fresh,
            ),
          );
        }
      },
    );

    installArrangeDrag(control);
    stage.append(control);
  }

  for (
    const spec
    of parameters
  ) {
    updateControlValue(
      spec.index,
      spec.normalized,
      spec,
    );
  }
}

function setParameter(
  index,
  normalized,
) {
  const value =
    clamp01(normalized);

  const spec =
    currentSpec(index);

  if (spec) {
    spec.normalized =
      value;
  }

  updateControlValue(
    index,
    value,
    spec,
  );

  send({
    cmd: 'setUserParameter',
    index,
    value,
  });
}

function updateControlValue(
  index,
  normalized,
  spec,
) {
  const control =
    stage.querySelector(
      `.parameter-control[data-index="${index}"]`,
    );

  if (!control) return;

  const knob =
    control.querySelector(
      '.knob',
    );

  const slider =
    control.querySelector(
      '.parameter-slider',
    );

  const toggle =
    control.querySelector(
      '.parameter-toggle',
    );

  const output =
    control.querySelector(
      '.control-value',
    );

  if (knob) {
    knob.style.setProperty(
      '--value',
      normalized,
    );
  }

  if (
    slider &&
    document.activeElement !==
    slider
  ) {
    slider.value =
      normalized;
  }

  toggle
    ?.classList
    .toggle(
      'on',
      normalized >= 0.5,
    );

  if (
    output &&
    spec
  ) {
    output.textContent =
      displayValue(
        spec,
        normalized,
      );
  }

  knob?.setAttribute(
    'aria-valuenow',
    spec
      ? displayValue(
        spec,
        normalized,
      )
      : normalized.toFixed(3),
  );
}

function installKnob(
  knob,
  index,
) {
  knob.addEventListener(
    'pointerdown',
    event => {
      if (
        ui.layout.arranging
      ) {
        return;
      }

      event.preventDefault();

      ui.layout.interacting =
        true;

      knob.setPointerCapture(
        event.pointerId,
      );

      const startY =
        event.clientY;

      const start =
        currentSpec(index)
          ?.normalized || 0;

      const move =
        moveEvent =>
          setParameter(
            index,
            start +
            (
              startY -
              moveEvent.clientY
            ) /
            140,
          );

      const finish = () => {
        ui.layout.interacting =
          false;

        knob.removeEventListener(
          'pointermove',
          move,
        );

        knob.removeEventListener(
          'pointerup',
          finish,
        );

        knob.removeEventListener(
          'pointercancel',
          finish,
        );
      };

      knob.addEventListener(
        'pointermove',
        move,
      );

      knob.addEventListener(
        'pointerup',
        finish,
      );

      knob.addEventListener(
        'pointercancel',
        finish,
      );
    },
  );

  knob.addEventListener(
    'wheel',
    event => {
      event.preventDefault();

      const spec =
        currentSpec(index);

      if (!spec) return;

      const span =
        spec.max -
        spec.min;

      const amount =
        spec.step > 0 &&
          span !== 0
          ? spec.step / span
          : 0.01;

      setParameter(
        index,
        spec.normalized +
        (
          event.deltaY < 0
            ? amount
            : -amount
        ),
      );
    },
    {
      passive: false,
    },
  );

  knob.addEventListener(
    'keydown',
    event => {
      if (
        ![
          'ArrowUp',
          'ArrowRight',
          'ArrowDown',
          'ArrowLeft',
        ].includes(event.key)
      ) {
        return;
      }

      event.preventDefault();

      const spec =
        currentSpec(index);

      if (!spec) return;

      const direction =
        [
          'ArrowUp',
          'ArrowRight',
        ].includes(event.key)
          ? 1
          : -1;

      setParameter(
        index,
        spec.normalized +
        direction * 0.01,
      );
    },
  );
}

function layoutFor(name) {
  return ui.latestState
    ?.controls
    ?.find(
      control =>
        control.name === name,
    );
}

function installArrangeDrag(
  control,
) {
  control.addEventListener(
    'pointerdown',
    event => {
      if (
        !ui.layout.arranging ||
        event.button !== 0
      ) {
        return;
      }

      event.preventDefault();

      ui.layout.interacting =
        true;

      control.setPointerCapture(
        event.pointerId,
      );

      const workingControls =
        cloneControls(
          ui.latestState
            ?.controls,
        );

      const layout =
        workingControls.find(
          item =>
            item.name ===
            control.dataset.name,
        );

      if (!layout) {
        ui.layout.interacting =
          false;

        return;
      }

      if (ui.latestState) {
        ui.latestState.controls =
          workingControls;
      }

      const bounds =
        stage
          .getBoundingClientRect();

      const box =
        control
          .getBoundingClientRect();

      const resizing =
        event.clientX >
        box.right - 14 &&
        event.clientY >
        box.bottom - 14;

      const start = {
        x: event.clientX,
        y: event.clientY,
        left: layout.x,
        top: layout.y,
        width: layout.width,
        height: layout.height,
      };

      const move =
        moveEvent => {
          if (resizing) {
            layout.width =
              Math.max(
                7,
                Math.min(
                  100 -
                  layout.x,

                  start.width +
                  (
                    moveEvent.clientX -
                    start.x
                  ) /
                  bounds.width *
                  100,
                ),
              );

            layout.height =
              Math.max(
                14,
                Math.min(
                  100 -
                  layout.y,

                  start.height +
                  (
                    moveEvent.clientY -
                    start.y
                  ) /
                  bounds.height *
                  100,
                ),
              );
          } else {
            layout.x =
              Math.max(
                0,
                Math.min(
                  100 -
                  layout.width,

                  start.left +
                  (
                    moveEvent.clientX -
                    start.x
                  ) /
                  bounds.width *
                  100,
                ),
              );

            layout.y =
              Math.max(
                0,
                Math.min(
                  100 -
                  layout.height,

                  start.top +
                  (
                    moveEvent.clientY -
                    start.y
                  ) /
                  bounds.height *
                  100,
                ),
              );
          }

          Object.assign(
            control.style,
            {
              left:
                `${layout.x}%`,

              top:
                `${layout.y}%`,

              width:
                `${layout.width}%`,

              height:
                `${layout.height}%`,
            },
          );
        };

      let finished = false;

      const finish = () => {
        if (finished) return;

        finished = true;

        saveLayout(
          workingControls,
        );

        ui.layout.interacting =
          false;

        control.removeEventListener(
          'pointermove',
          move,
        );

        control.removeEventListener(
          'pointerup',
          finish,
        );

        control.removeEventListener(
          'pointercancel',
          finish,
        );
      };

      control.addEventListener(
        'pointermove',
        move,
      );

      control.addEventListener(
        'pointerup',
        finish,
      );

      control.addEventListener(
        'pointercancel',
        finish,
      );
    },
  );
}

function saveLayout(
  controls =
    ui.latestState?.controls,
) {
  if (!ui.latestState) {
    return;
  }

  const snapshot =
    cloneControls(controls);

  ui.latestState.controls =
    snapshot;

  ui.layout.fingerprint =
    controlFingerprint(
      snapshot,
    );

  ui.layout.pendingFingerprint =
    ui.layout.fingerprint;

  ui.layout.pendingControls =
    cloneControls(snapshot);

  send({
    cmd: 'setLayout',
    controls:
      ui.layout.pendingControls,
  });
}

function setControlKind(
  name,
  kind,
) {
  const layout =
    layoutFor(name);

  if (!layout) return;

  layout.kind = kind;

  saveLayout();

  ui.layout.fingerprint = '';

  syncControls(
    ui.latestState.parameters || [],
    ui.latestState.controls || [],
  );
}

function toggleArrange(force) {
  ui.layout.arranging =
    typeof force === 'boolean'
      ? force
      : !ui.layout.arranging;

  stage.classList.toggle(
    'arranging',
    ui.layout.arranging,
  );

  $('#arrange-button')
    .classList
    .toggle(
      'active',
      ui.layout.arranging,
    );

  $('#arrange-hint')
    .textContent =
    ui.layout.arranging
      ? 'Layout mode: ドラッグで移動、右下のハンドルでサイズ変更、プロジェクトに保存'
      : 'Guide: p.name = param(default, min, max, step, cc_link?) · 右クリックでVST操作';
}

$('#arrange-button')
  .addEventListener(
    'click',
    () => toggleArrange(),
  );

$('#reset-parameters')
  .addEventListener(
    'click',
    () => {
      for (
        const spec
        of ui.latestState
          ?.parameters || []
      ) {
        setParameter(
          spec.index,
          normalizedDefault(spec),
        );
      }

      showToast(
        'すべてのパラメーターをリセットしました',
      );
    },
  );

// -----------------------------------------------------------------------------
// Guides / context menu
// -----------------------------------------------------------------------------

function openParameterGuide(
  event,
) {
  event?.preventDefault();

  openModal(
    '#parameter-guide',
  );
}

function openEditorGuide(
  event,
) {
  event?.preventDefault();

  openModal(
    '#editor-guide',
  );
}

function closeParameterGuide(
  event,
) {
  event?.preventDefault();
  event?.stopPropagation();

  closeModal(
    '#parameter-guide',
  );
}

function closeEditorGuide(
  event,
) {
  event?.preventDefault();
  event?.stopPropagation();

  closeModal(
    '#editor-guide',
  );
}

$('#parameter-guide-button')
  .addEventListener(
    'click',
    openParameterGuide,
  );

$('#editor-guide-button')
  .addEventListener(
    'click',
    openEditorGuide,
  );

$('#close-parameter-guide')
  .addEventListener(
    'click',
    closeParameterGuide,
  );

$('#close-editor-guide')
  .addEventListener(
    'click',
    closeEditorGuide,
  );

$('#load-parameter-guide')
  .addEventListener(
    'click',
    () => {
      closeParameterGuide();

      loadPreset(
        'Parameter Guide',
        true,
      );
    },
  );

$('#parameter-guide')
  .addEventListener(
    'pointerdown',
    event => {
      if (
        event.target.id ===
        'parameter-guide'
      ) {
        closeParameterGuide();
      }
    },
  );

$('#editor-guide')
  .addEventListener(
    'pointerdown',
    event => {
      if (
        event.target.id ===
        'editor-guide'
      ) {
        closeEditorGuide();
      }
    },
  );

function menuItemsFor(control) {
  if (control) {
    const index =
      Number(
        control.dataset.index,
      );

    const spec =
      currentSpec(index);

    const layout =
      layoutFor(
        control.dataset.name,
      );

    if (
      !spec ||
      !layout
    ) {
      return [];
    }

    return [
      {
        label:
          `Reset ${spec.label}`,

        shortcut:
          `${spec.default}`,

        action:
          () =>
            setParameter(
              index,
              normalizedDefault(
                spec,
              ),
            ),
      },

      {
        label:
          'Copy parameter value',

        action: () => {
          ui.copiedParameter =
            spec.normalized;

          showToast(
            `${spec.label} copied`,
          );
        },
      },

      {
        label:
          'Paste parameter value',

        disabled:
          ui.copiedParameter ===
          null,

        action:
          () =>
            setParameter(
              index,
              ui.copiedParameter,
            ),
      },

      null,

      {
        label:
          'Display as knob',

        checked:
          layout.kind ===
          'knob',

        action:
          () =>
            setControlKind(
              layout.name,
              'knob',
            ),
      },

      {
        label:
          'Display as slider',

        checked:
          layout.kind ===
          'slider',

        action:
          () =>
            setControlKind(
              layout.name,
              'slider',
            ),
      },

      {
        label:
          'Display as toggle',

        checked:
          layout.kind ===
          'toggle',

        action:
          () =>
            setControlKind(
              layout.name,
              'toggle',
            ),
      },

      null,

      {
        label:
          ui.layout.arranging
            ? 'Finish arranging'
            : 'Arrange controls',

        action:
          () =>
            toggleArrange(),
      },
    ];
  }

  return [
    {
      label:
        'Compile program',

      shortcut:
        'Ctrl+S',

      action:
        () =>
          compileNow(false),
    },

    {
      label:
        'Preview current note',

      shortcut:
        'Ctrl+Enter',

      action:
        () =>
          compileNow(true),
    },

    null,

    {
      label:
        ui.mode === 'editor'
          ? 'Switch to Play'
          : 'Switch to Editor',

      action:
        () =>
          setMode(
            ui.mode === 'editor'
              ? 'play'
              : 'editor',
          ),
    },

    {
      label:
        'Reset all parameters',

      action:
        () =>
          $('#reset-parameters')
            .click(),
    },

    {
      label:
        'Parameter & layout guide',

      action:
        openParameterGuide,
    },

    {
      label:
        'Release all notes',

      action:
        releaseAllNotes,
    },

    null,

    {
      label:
        'Copy synth source',

      action:
        async () => {
          try {
            await navigator
              .clipboard
              .writeText(
                editor.getValue(),
              );

            showToast(
              'Source copied',
            );
          } catch {
            showToast(
              'Clipboard unavailable in this host',
            );
          }
        },
    },
  ];
}

function openContextMenu(
  x,
  y,
  items,
) {
  contextMenu.replaceChildren();

  for (const item of items) {
    if (!item) {
      contextMenu.append(
        document.createElement(
          'hr',
        ),
      );

      continue;
    }

    const button =
      document.createElement(
        'button',
      );

    button.disabled =
      Boolean(item.disabled);

    const label =
      document.createElement(
        'span',
      );

    label.textContent =
      `${item.checked ? '● ' : ''}${item.label}`;

    const shortcut =
      document.createElement(
        'kbd',
      );

    shortcut.textContent =
      item.shortcut || '';

    button.append(
      label,
      shortcut,
    );

    button.addEventListener(
      'click',
      () => {
        contextMenu.hidden =
          true;

        item.action?.();
      },
    );

    contextMenu.append(button);
  }

  contextMenu.hidden = false;

  const width = 220;
  const height =
    contextMenu.offsetHeight;

  contextMenu.style.left =
    `${Math.max(
      4,
      Math.min(
        x,
        innerWidth -
        width -
        6,
      ),
    )
    }px`;

  contextMenu.style.top =
    `${Math.max(
      4,
      Math.min(
        y,
        innerHeight -
        height -
        6,
      ),
    )
    }px`;
}

document.addEventListener(
  'contextmenu',
  event => {
    event.preventDefault();

    openContextMenu(
      event.clientX,
      event.clientY,
      menuItemsFor(
        event.target.closest(
          '.parameter-control',
        ),
      ),
    );
  },
);

document.addEventListener(
  'pointerdown',
  event => {
    if (
      !contextMenu.hidden &&
      !contextMenu.contains(
        event.target,
      )
    ) {
      contextMenu.hidden =
        true;
    }
  },
  true,
);

document.addEventListener(
  'keydown',
  event => {
    if (
      event.key !==
      'Escape'
    ) {
      return;
    }

    contextMenu.hidden =
      true;

    closePresetConfirm();
    closeParameterGuide();
    closeEditorGuide();

    if (
      ui.layout.arranging
    ) {
      toggleArrange(false);
    }
  },
);

// -----------------------------------------------------------------------------
// Keyboard / MIDI preview
// -----------------------------------------------------------------------------

const localActiveNotes =
  new Set();

const localNoteVelocities =
  new Map();

let midiNoteVelocities =
  new Map();

let releasingNoteVelocities =
  new Map();

let keyboardOctave = 4;
let keyboardVelocity = 0.9;
let sustainHeld = false;

const sustainedNotes =
  new Set();

function renderKeyHighlight(
  note,
) {
  const localVelocity =
    localNoteVelocities
      .get(note) || 0;

  const midiVelocity =
    midiNoteVelocities
      .get(note) || 0;

  const velocity =
    Math.max(
      localVelocity,
      midiVelocity,
    );

  const releaseVelocity =
    releasingNoteVelocities
      .get(note) || 0;

  const pressed =
    velocity > 0;

  const releasing =
    releaseVelocity > 0;

  document
    .querySelectorAll(
      `.key[data-note="${note}"]`,
    )
    .forEach(
      key => {
        key.classList.toggle(
          'active',
          localVelocity > 0,
        );

        key.classList.toggle(
          'midi-active',
          midiVelocity > 0,
        );

        key.classList.toggle(
          'release-active',
          releasing,
        );

        if (pressed) {
          const brightness =
            Math.round(
              132 +
              velocity * 112,
            );

          const black =
            key.classList
              .contains(
                'black',
              );

          const base =
            black
              ? Math.round(
                70 +
                velocity * 166,
              )
              : brightness;

          key.style.backgroundColor =
            `rgb(${base} ${base} ${Math.max(
              0,
              base -
              (
                black
                  ? 7
                  : 8
              ),
            )
            })`;
        } else {
          key.style
            .removeProperty(
              'background-color',
            );
        }

        if (releasing) {
          const outline =
            Math.round(
              112 +
              releaseVelocity *
              143,
            );

          const inset =
            Math.round(
              72 +
              releaseVelocity *
              130,
            );

          key.style.setProperty(
            '--release-outline',
            `rgb(${outline} ${outline} ${Math.max(
              0,
              outline - 8,
            )
            })`,
          );

          key.style.setProperty(
            '--release-inset',
            `rgb(${inset} ${inset} ${Math.max(
              0,
              inset - 8,
            )
            })`,
          );
        } else {
          key.style.removeProperty(
            '--release-outline',
          );

          key.style.removeProperty(
            '--release-inset',
          );
        }
      },
    );
}

function syncMidiPreview(
  notes,
  releaseNotes,
) {
  const next =
    new Map(
      (notes || []).map(
        item => {
          const note =
            typeof item ===
              'number'
              ? item
              : item.note;

          const velocity =
            typeof item ===
              'number'
              ? 0.9
              : item.velocity;

          return [
            Number(note),
            Math.max(
              0.01,
              clamp01(
                velocity ||
                0.9,
              ),
            ),
          ];
        },
      ),
    );

  const nextReleases =
    new Map(
      (releaseNotes || [])
        .map(
          item => [
            Number(item.note),
            clamp01(
              item.velocity,
            ),
          ],
        )
        .filter(
          ([, velocity]) =>
            velocity > 0,
        ),
    );

  const changed =
    new Set([
      ...midiNoteVelocities.keys(),
      ...next.keys(),
      ...releasingNoteVelocities.keys(),
      ...nextReleases.keys(),
    ]);

  midiNoteVelocities =
    next;

  releasingNoteVelocities =
    nextReleases;

  for (const note of changed) {
    renderKeyHighlight(note);
  }

  const indicator =
    $('#midi-input-indicator');

  const latest =
    [...midiNoteVelocities.entries()]
      .at(-1);

  if (latest) {
    const [note, velocity] =
      latest;

    const additional =
      midiNoteVelocities.size > 1
        ? ` +${midiNoteVelocities.size - 1}`
        : '';

    indicator.textContent =
      `MIDI IN ${midiName(note)} · ${Math.round(
        velocity * 100,
      )
      }%${additional}`;

    indicator.classList.add(
      'active',
    );
  } else {
    indicator.textContent =
      'MIDI IN —';

    indicator.classList.remove(
      'active',
    );
  }
}

function noteOn(
  note,
  _key,
  velocity = 0.9,
) {
  if (!ui.endpoints.hasNote) return;

  if (
    localActiveNotes.has(note)
  ) {
    return;
  }

  sustainedNotes.delete(note);

  localActiveNotes.add(note);

  localNoteVelocities.set(
    note,
    velocity,
  );

  renderKeyHighlight(note);

  send({
    cmd: 'setParameter',
    name: 'previewNote',
    value: note,
  });

  send({
    cmd: 'noteOn',
    note,
    velocity,
  });
}

function noteOff(
  note,
  _key,
) {
  if (
    !localActiveNotes.delete(
      note,
    )
  ) {
    return;
  }

  localNoteVelocities.delete(
    note,
  );

  renderKeyHighlight(note);

  if (sustainHeld) {
    sustainedNotes.add(note);
    return;
  }

  send({
    cmd: 'noteOff',
    note,
  });
}

function releaseAllNotes() {
  sustainHeld = false;

  $('#sustain-button')
    .classList
    .remove('active');

  sustainedNotes.clear();

  for (
    const note
    of [...localActiveNotes]
  ) {
    localActiveNotes.delete(
      note,
    );

    localNoteVelocities.delete(
      note,
    );

    renderKeyHighlight(note);

    send({
      cmd: 'noteOff',
      note,
    });
  }
}

function setSustain(held) {
  sustainHeld =
    Boolean(held);

  $('#sustain-button')
    .classList
    .toggle(
      'active',
      sustainHeld,
    );

  if (sustainHeld) {
    return;
  }

  for (
    const note
    of [...sustainedNotes]
  ) {
    sustainedNotes.delete(note);

    send({
      cmd: 'noteOff',
      note,
    });
  }
}

function previewNote() {
  if (!ui.endpoints.hasNote) return;

  const note =
    ui.latestState
      ?.previewNote ?? 60;

  const key =
    document.querySelector(
      `.key[data-note="${note}"]`,
    );

  noteOn(
    note,
    key,
    keyboardVelocity,
  );

  setTimeout(
    () =>
      noteOff(
        note,
        key,
      ),
    420,
  );
}

function buildKeyboard() {
  const keyboard =
    $('#keyboard');

  const start =
    (keyboardOctave + 1) *
    12;

  const isBlack =
    note =>
      [1, 3, 6, 8, 10]
        .includes(
          note % 12,
        );

  const visibleWidth =
    Math.max(
      420,
      keyboard.parentElement
        ?.clientWidth || 900,
    );

  const whiteTarget =
    Math.max(
      29,
      Math.min(
        61,
        Math.ceil(
          visibleWidth / 28,
        ),
      ),
    );

  const notes = [];

  let whiteNotes = 0;
  let note = start;

  while (
    whiteNotes <
    whiteTarget
  ) {
    notes.push(note);

    if (!isBlack(note)) {
      whiteNotes += 1;
    }

    note += 1;
  }

  const whiteCount =
    notes.filter(
      item =>
        !isBlack(item),
    ).length;

  const whiteWidth =
    100 / whiteCount;

  let whiteIndex = 0;

  keyboard.replaceChildren();
  keyboard.style.minWidth = '0';

  for (
    const midiNote
    of notes
  ) {
    const black =
      isBlack(midiNote);

    const key =
      document.createElement(
        'button',
      );

    key.className =
      `key ${black
        ? 'black'
        : 'white'
      }`;

    key.dataset.note =
      midiNote;

    key.title =
      midiName(midiNote);

    if (black) {
      key.style.left =
        `${whiteIndex *
        whiteWidth -
        whiteWidth *
        0.31
        }%`;

      key.style.width =
        `${whiteWidth *
        0.62
        }%`;
    } else {
      key.style.left =
        `${whiteIndex *
        whiteWidth
        }%`;

      key.style.width =
        `${whiteWidth}%`;

      key.textContent =
        midiNote % 12 === 0
          ? midiName(
            midiNote,
          )
          : '';

      whiteIndex += 1;
    }

    key.addEventListener(
      'pointerdown',
      event => {
        event.preventDefault();

        key.setPointerCapture?.(
          event.pointerId,
        );

        noteOn(
          midiNote,
          key,
          keyboardVelocity,
        );
      },
    );

    const release =
      event => {
        event.preventDefault();

        noteOff(
          midiNote,
          key,
        );
      };

    key.addEventListener(
      'pointerup',
      release,
    );

    key.addEventListener(
      'pointercancel',
      release,
    );

    keyboard.append(key);

    renderKeyHighlight(
      midiNote,
    );
  }
}

function setKeyboardOctave(
  delta,
) {
  keyboardOctave =
    Math.max(
      0,
      Math.min(
        8,
        keyboardOctave +
        delta,
      ),
    );

  $('#keyboard-octave')
    .textContent =
    `C${keyboardOctave}`;

  releaseAllNotes();
  buildKeyboard();
}

$('#octave-down')
  .addEventListener(
    'click',
    () =>
      setKeyboardOctave(-1),
  );

$('#octave-up')
  .addEventListener(
    'click',
    () =>
      setKeyboardOctave(1),
  );

$('#panic-button')
  .addEventListener(
    'click',
    releaseAllNotes,
  );

$('#sustain-button')
  .addEventListener(
    'pointerdown',
    event => {
      event.preventDefault();
      setSustain(true);
    },
  );

$('#sustain-button')
  .addEventListener(
    'pointerup',
    () =>
      setSustain(false),
  );

$('#sustain-button')
  .addEventListener(
    'pointerleave',
    () =>
      setSustain(false),
  );

$('#keyboard-velocity')
  .addEventListener(
    'input',
    event => {
      keyboardVelocity =
        Number(
          event.target.value,
        ) / 127;

      $('#velocity-value')
        .textContent =
        `${Math.round(
          keyboardVelocity *
          100,
        )
        }%`;
    },
  );

new ResizeObserver(
  () => buildKeyboard(),
).observe(
  $('#keyboard').parentElement,
);

const computerKeys = {
  z: 48,
  s: 49,
  x: 50,
  d: 51,
  c: 52,
  v: 53,
  g: 54,
  b: 55,
  h: 56,
  n: 57,
  j: 58,
  m: 59,
  ',': 60,
  q: 60,
  '2': 61,
  w: 62,
  '3': 63,
  e: 64,
  r: 65,
  '5': 66,
  t: 67,
  '6': 68,
  y: 69,
  '7': 70,
  u: 71,
};

const heldComputerKeys =
  new Set();

window.addEventListener(
  'keydown',
  event => {
    if (
      event.repeat ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey ||
      event.target.closest?.(
        '.monaco-editor, input, select',
      )
    ) {
      return;
    }

    if (!ui.endpoints.hasNote) return;

    const keyName =
      event.key.toLowerCase();

    const note =
      computerKeys[keyName];

    if (
      note === undefined
    ) {
      return;
    }

    event.preventDefault();

    heldComputerKeys.add(
      keyName,
    );

    noteOn(
      note,
      document.querySelector(
        `.key[data-note="${note}"]`,
      ),
      keyboardVelocity,
    );
  },
);

window.addEventListener(
  'keyup',
  event => {
    const keyName =
      event.key.toLowerCase();

    if (
      !heldComputerKeys.delete(
        keyName,
      )
    ) {
      return;
    }

    const note =
      computerKeys[keyName];

    noteOff(
      note,
      document.querySelector(
        `.key[data-note="${note}"]`,
      ),
    );
  },
);

window.addEventListener(
  'blur',
  () => {
    heldComputerKeys.clear();
    releaseAllNotes();
  },
);

function midiName(note) {
  const value =
    Number(note) || 0;

  const names = [
    'C',
    'C♯',
    'D',
    'E♭',
    'E',
    'F',
    'F♯',
    'G',
    'A♭',
    'A',
    'B♭',
    'B',
  ];

  return `${names[
    (
      (value % 12) +
      12
    ) % 12
  ]
    }${Math.floor(
      value / 12,
    ) - 1
    }`;
}

// -----------------------------------------------------------------------------
// Waveform scope
// -----------------------------------------------------------------------------

const SCOPE_FRAME_LENGTH = 768;

let previousWave = null;
let lastWaveOffset = 0;

function phaseLockedFrame(raw) {
  if (!raw?.length) {
    return [];
  }

  const samples =
    raw.map(
      value =>
        Number.isFinite(value)
          ? value
          : 0,
    );

  const frameLength =
    Math.min(
      SCOPE_FRAME_LENGTH,
      samples.length,
    );

  const maxOffset =
    Math.max(
      0,
      samples.length -
      frameLength,
    );

  const energy =
    samples.reduce(
      (sum, value) =>
        sum +
        value * value,
      0,
    );

  if (
    energy /
    samples.length <
    1e-8
  ) {
    previousWave = null;
    lastWaveOffset = 0;

    return samples.slice(
      0,
      frameLength,
    );
  }

  let offset = 0;

  if (
    previousWave?.length ===
    frameLength
  ) {
    let best = -Infinity;

    const compare =
      Math.min(
        128,
        frameLength,
      );

    for (
      let shift = 0;
      shift <= maxOffset;
      shift += 2
    ) {
      let dot = 0;
      let oldPower = 0;
      let newPower = 0;

      for (
        let index = 0;
        index < compare;
        index += 1
      ) {
        const a =
          previousWave[index];

        const b =
          samples[
          index + shift
          ];

        dot += a * b;
        oldPower += a * a;
        newPower += b * b;
      }

      const score =
        dot /
        Math.sqrt(
          Math.max(
            1e-12,
            oldPower *
            newPower,
          ),
        );

      if (score > best) {
        best = score;
        offset = shift;
      }
    }
  } else {
    let strongest =
      -Infinity;

    for (
      let index = 1;
      index <= maxOffset;
      index += 1
    ) {
      const rise =
        samples[index] -
        samples[index - 1];

      if (
        samples[index - 1] <= 0 &&
        samples[index] > 0 &&
        rise > strongest
      ) {
        strongest = rise;
        offset = index;
      }
    }
  }

  const frame =
    samples.slice(
      offset,
      offset +
      frameLength,
    );

  lastWaveOffset =
    offset;

  previousWave =
    frame.slice();

  return frame;
}

function drawScope(
  rawLeft,
  rawRight,
) {
  const left =
    phaseLockedFrame(
      rawLeft,
    );

  const right =
    (rawRight || [])
      .map(
        value =>
          Number.isFinite(value)
            ? value
            : 0,
      )
      .slice(
        lastWaveOffset,
        lastWaveOffset +
        left.length,
      );

  const rect =
    scope.getBoundingClientRect();

  const ratio =
    devicePixelRatio || 1;

  const width =
    Math.max(
      1,
      Math.round(
        rect.width *
        ratio,
      ),
    );

  const height =
    Math.max(
      1,
      Math.round(
        rect.height *
        ratio,
      ),
    );

  if (
    scope.width !== width ||
    scope.height !== height
  ) {
    scope.width = width;
    scope.height = height;
  }

  const ctx =
    scope.getContext('2d');

  ctx.clearRect(
    0,
    0,
    width,
    height,
  );

  const centerY =
    height / 2;

  const plotHalfHeight =
    Math.min(
      centerY,
      height - centerY,
    ) * 0.85;

  ctx.strokeStyle =
    '#282828';

  ctx.lineWidth =
    ratio;

  ctx.beginPath();

  ctx.moveTo(
    0,
    centerY,
  );

  ctx.lineTo(
    width,
    centerY,
  );

  ctx.stroke();

  let peak = 0;

  const drawChannel =
    (samples, color) => {
      ctx.strokeStyle =
        color;

      ctx.lineWidth =
        1.35 * ratio;

      ctx.beginPath();

      samples.forEach(
        (sample, index) => {
          peak =
            Math.max(
              peak,
              Math.abs(sample),
            );

          const inset =
            ctx.lineWidth *
            0.5;

          const x =
            inset +
            index /
            Math.max(
              1,
              samples.length - 1,
            ) *
            (
              width -
              inset * 2
            );

          const y =
            centerY -
            Math.max(
              -1.5,
              Math.min(
                1.5,
                sample,
              ),
            ) /
            1.5 *
            plotHalfHeight;

          if (index) {
            ctx.lineTo(x, y);
          } else {
            ctx.moveTo(x, y);
          }
        },
      );

      ctx.stroke();
    };

  drawChannel(left, 'rgba(210, 210, 205, 0.78)');
  drawChannel(right, 'rgba(169, 183, 230, 0.78)');

  $('#output-meter')
    .style.width =
    `${Math.min(
      100,
      peak /
      1.5 *
      100,
    )
    }%`;

  $('#peak-value')
    .textContent =
    peak.toFixed(2);
}

async function updateScope() {
  try {
    const response =
      await fetch(
        './api/waveform',
        {
          cache: 'no-store',
        },
      );

    if (!response.ok) {
      return;
    }

    const wave =
      await response.json();

    const tap =
      wave.tap === 'output'
        ? 'out'
        : 'mix + in';

    $('#scope-mode')
      .textContent =
      wave.live
        ? `live · ${wave.activeVoices} voices · ${tap}`
        : `compiled preview · ${tap}`;

    drawScope(
      wave.left,
      wave.right,
    );
  } catch {
    // Retry on the next interval.
  }
}

$$('[data-wave-tap]')
  .forEach(
    button => {
      button.addEventListener(
        'click',
        () => {
          const tap =
            button.dataset.waveTap;

          $$('[data-wave-tap]')
            .forEach(
              item => {
                item.classList.toggle(
                  'active',
                  item === button,
                );
              },
            );

          send({
            cmd: 'setWaveformTap',
            tap,
          });

          void updateScope();
        },
      );
    },
  );

// -----------------------------------------------------------------------------
// Startup
// -----------------------------------------------------------------------------

buildKeyboard();
syncEndpointUi(editor.getValue());

window.__CODE_SYNTH_UI_READY__ =
  true;

$('#boot-status')?.remove();

send({
  cmd: 'uiReady',
});

void pollState();
void updateScope();

setInterval(
  () => void pollState(),
  POLL_INTERVAL_MS,
);

setInterval(
  () => void updateScope(),
  SCOPE_INTERVAL_MS,
);