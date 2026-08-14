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
};
const OUTPUT_DOCS = {
  wave: '必須のモノラルボイス出力。', pan: '任意のボイスパン、-1–1。',
  l_limit: '必須。ノートを離してからボイスを終了するまでの秒数。',
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
  mix: ['mix(a, b, amount)', 'Linear interpolation.'], cc: ['cc(number)', 'MIDI CC, 0–1.'], noise: ['noise()', 'White noise.'],
  saw: ['saw(freq, t)', 'Band-limited saw.'], triangle: ['triangle(freq, t)', 'Triangle oscillator.'],
  square: ['square(freq, t, duty)', 'Band-limited pulse.'], pulse: ['pulse(freq, t, duty)', 'Alias for square.'],
};

monaco.languages.register({ id: 'synth-dsl' });
monaco.languages.setMonarchTokensProvider('synth-dsl', { tokenizer: { root: [
  [/(#|\/\/).*$/, 'comment'], [/\bp_[A-Za-z0-9_]*\b/, 'parameter'], [/\b(?:TAU|PI|E|PHI)\b/, 'constant'],
  [new RegExp(`\\b(?:${Object.keys(FUNCTION_DOCS).join('|')}|param)\\b(?=\\s*\\()`), 'function'],
  [new RegExp(`\\b(?:${Object.keys(INPUT_DOCS).join('|')})\\b`), 'variable.predefined'],
  [/\b(?:wave|pan|l_limit)\b(?=\s*=)/, 'type.identifier'], [/[A-Za-z_][\w]*/, 'identifier'],
  [/(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/i, 'number'], [/[=+\-*\/%^]/, 'operator'], [/[(),]/, 'delimiter'],
] } });
monaco.languages.setLanguageConfiguration('synth-dsl', {
  wordPattern: /[A-Za-z_][A-Za-z0-9_]*/,
  brackets: [['(', ')']],
  autoClosingPairs: [{ open: '(', close: ')' }],
  surroundingPairs: [{ open: '(', close: ')' }],
});

monaco.editor.defineTheme('math-synth', {
  base: 'vs-dark', inherit: true,
  rules: [
    { token: 'comment', foreground: '666666', fontStyle: 'italic' }, { token: 'constant', foreground: 'b8a985' },
    { token: 'function', foreground: 'a9b7e6' }, { token: 'parameter', foreground: 'd4b6cf' },
    { token: 'variable.predefined', foreground: '91a6b8' }, { token: 'type.identifier', foreground: 'cccccc', fontStyle: 'bold' },
    { token: 'number', foreground: 'b9aa9d' }, { token: 'operator', foreground: '999999' },
  ],
  colors: {
    'editor.background': '#1a1a1a', 'editor.foreground': '#999999', 'editorLineNumber.foreground': '#444444',
    'editorLineNumber.activeForeground': '#a9b7e6', 'editorCursor.foreground': '#a9b7e6',
    'editor.selectionBackground': '#6f7a9566', 'editor.lineHighlightBackground': '#202020',
    'editorWidget.background': '#1a1a1a', 'editorWidget.border': '#6f7a95',
    'editorSuggestWidget.background': '#1a1a1a', 'editorSuggestWidget.border': '#6f7a95',
    'editorSuggestWidget.selectedBackground': '#282828', 'editorHoverWidget.background': '#1a1a1a',
    'editorHoverWidget.border': '#6f7a95', 'input.background': '#121212',
  },
});

const editor = monaco.editor.create(document.querySelector('#editor'), {
  value: '', language: 'synth-dsl', theme: 'math-synth', automaticLayout: true, minimap: { enabled: false },
  contextmenu: false, glyphMargin: true, folding: false, fontFamily: "'UDEV Gothic HSLG', 'Cascadia Code', Consolas, monospace",
  fontSize: 13, lineHeight: 21, padding: { top: 12, bottom: 12 }, scrollBeyondLastLine: false,
  smoothScrolling: true, bracketPairColorization: { enabled: true }, guides: { bracketPairs: true, indentation: false },
  overviewRulerBorder: false, renderLineHighlight: 'all', wordWrap: 'on',
  quickSuggestions: { other: true, comments: false, strings: false }, quickSuggestionsDelay: 0,
  suggest: { showWords: true, showSnippets: true, preview: true, snippetsPreventQuickSuggestions: false },
  snippetSuggestions: 'top', wordBasedSuggestions: 'off', suggestOnTriggerCharacters: true,
  acceptSuggestionOnEnter: 'on', suggestSelection: 'first', parameterHints: { enabled: true }, tabSize: 2,
});

editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Space, () => {
  editor.trigger('synth-dsl', 'editor.action.triggerSuggest', {});
});

function sourceSymbols(model) {
  return [...new Set([...model.getValue().matchAll(/^\s*([A-Za-z_][\w]*)\s*=/gm)].map(match => match[1]))];
}

monaco.languages.registerCompletionItemProvider('synth-dsl', {
  triggerCharacters: ['_', '('],
  provideCompletionItems(model, position) {
    const word = model.getWordUntilPosition(position);
    const range = new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn);
    const suggestions = [];
    for (const [label, documentation] of Object.entries(INPUT_DOCS)) {
      suggestions.push({ label, kind: monaco.languages.CompletionItemKind.Variable, insertText: label, detail: '実行時入力', documentation, range, sortText: `2-${label}` });
    }
    for (const [label, documentation] of Object.entries(OUTPUT_DOCS)) {
      suggestions.push({ label, kind: monaco.languages.CompletionItemKind.Field, insertText: `${label} = ` + '${1:0}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, detail: 'シンセ出力', documentation, range, sortText: `1-${label}` });
    }
    for (const [label, [signature, documentation]] of Object.entries(FUNCTION_DOCS)) {
      const raw = signature.slice(signature.indexOf('(') + 1, -1);
      const args = raw ? raw.split(', ').map((arg, index) => '${' + (index + 1) + ':' + arg + '}').join(', ') : '';
      suggestions.push({ label, kind: monaco.languages.CompletionItemKind.Function, insertText: `${label}(${args})`, insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, detail: signature, documentation, range, sortText: `3-${label}` });
    }
    suggestions.push({
      label: 'param', kind: monaco.languages.CompletionItemKind.Snippet,
      insertText: 'p_${1:name} = param(${2:0.5}, ${3:0}, ${4:1}, ${5:0.01})',
      insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
      detail: 'Play mode用VSTパラメーターを宣言',
      documentation: 'ホストからオートメーションでき、Play modeで自由に配置できるコントロールを作成します。', range, sortText: '0-param',
    });
    suggestions.push({
      label: 'voice template', kind: monaco.languages.CompletionItemKind.Snippet,
      insertText: 'attack = clamp(t / ${1:0.008}, 0, 1)\nrelease = exp(-${2:6} * l)\nwave = s * attack * release * ${3|sin(TAU * freq * t),saw(freq, t),triangle(freq, t)|}\npan = midi_pan\nl_limit = ${4:1.0}',
      insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, detail: '演奏可能なボイスのひな形', range, sortText: '0-template',
    });
    for (const label of sourceSymbols(model)) {
      suggestions.push({ label, kind: label.startsWith('p_') ? monaco.languages.CompletionItemKind.Property : monaco.languages.CompletionItemKind.Variable, insertText: label, detail: label.startsWith('p_') ? 'ユーザーパラメーター' : 'ローカル式', range, sortText: `1-${label}` });
    }
    return { suggestions };
  },
});

monaco.languages.registerHoverProvider('synth-dsl', {
  provideHover(model, position) {
    const word = model.getWordAtPosition(position)?.word;
    if (!word) return null;
    if (FUNCTION_DOCS[word]) return { contents: [{ value: `\`${FUNCTION_DOCS[word][0]}\`` }, { value: FUNCTION_DOCS[word][1] }] };
    if (INPUT_DOCS[word]) return { contents: [{ value: `**${word}** — 実行時入力` }, { value: INPUT_DOCS[word] }] };
    if (OUTPUT_DOCS[word]) return { contents: [{ value: `**${word}** — output` }, { value: OUTPUT_DOCS[word] }] };
    if (word.startsWith('p_')) return { contents: [{ value: `**${word}** — ホストオートメーション対応パラメーター` }, { value: 'Play modeのコントロールを右クリックして、ノブ・スライダー・トグルを選択できます。' }] };
    return null;
  },
});

monaco.languages.registerSignatureHelpProvider('synth-dsl', {
  signatureHelpTriggerCharacters: ['(', ','], signatureHelpRetriggerCharacters: [','],
  provideSignatureHelp(model, position) {
    const prefix = model.getValueInRange(new monaco.Range(position.lineNumber, 1, position.lineNumber, position.column));
    const name = prefix.match(/([A-Za-z_]\w*)\s*\([^()]*$/)?.[1];
    const entry = name === 'param' ? ['param(default, min, max, step?)', 'Declare a p_* VST parameter.'] : FUNCTION_DOCS[name];
    if (!entry) return null;
    const activeParameter = (prefix.slice(prefix.lastIndexOf('(') + 1).match(/,/g) || []).length;
    const labels = entry[0].slice(entry[0].indexOf('(') + 1, -1).split(', ').filter(Boolean);
    return { value: { signatures: [{ label: entry[0], documentation: entry[1], parameters: labels.map(label => ({ label })) }], activeSignature: 0, activeParameter }, dispose() {} };
  },
});

monaco.languages.registerCodeActionProvider('synth-dsl', {
  provideCodeActions(model, _range, context) {
    const actions = [];
    for (const marker of context.markers) {
      const suggestion = marker.message.match(/Did you mean `([^`]+)`/i)?.[1];
      const word = model.getWordAtPosition({ lineNumber: marker.startLineNumber, column: marker.startColumn });
      if (suggestion) actions.push({
        title: `「${suggestion}」に置き換える`, kind: 'quickfix', isPreferred: true,
        edit: { edits: [{ resource: model.uri, versionId: model.getVersionId(), textEdit: { range: word ? new monaco.Range(marker.startLineNumber, word.startColumn, marker.startLineNumber, word.endColumn) : new monaco.Range(marker.startLineNumber, marker.startColumn, marker.endLineNumber, marker.endColumn), text: suggestion } }] },
      });
    }
    return { actions, dispose() {} };
  },
});

const $ = selector => document.querySelector(selector);
const app = $('#app');
const presetSelect = $('#preset');
const statusElement = $('#status');
const diagnostic = $('#diagnostic');
const diagnosticMessage = $('#diagnostic-message');
const stage = $('#control-stage');
const contextMenu = $('#context-menu');
const scope = $('#scope');
let initialized = false;
let statePollInFlight = false;
let editorDirty = false;
let applyingRemote = false;
let compileTimer = 0;
let suggestTimer = 0;
let lastGeneration = -1;
let latestState = null;
let currentMode = 'editor';
let arranging = false;
let layoutFingerprint = '';
let pendingLayoutFingerprint = null;
let pendingLayoutControls = null;
let initialRepairJustApplied = false;
let initialLayoutChecked = false;
let interacting = false;
let copiedParameter = null;
let toastTimer = 0;
let pendingPresetLoad = null;

function send(message) { window.ipc?.postMessage?.(JSON.stringify(message)); }
function showToast(message) {
  const toast = $('#toast'); toast.textContent = message; toast.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => { toast.hidden = true; }, 1800);
}
function compileNow(preview = false) {
  clearTimeout(compileTimer); statusElement.className = 'compile-status pending'; statusElement.textContent = 'Compiling…';
  send({ cmd: 'setExpression', source: editor.getValue() }); if (preview) previewNote();
}

editor.onDidChangeModelContent(event => {
  if (!initialized || applyingRemote) return;
  editorDirty = true;
  if (editor.hasTextFocus() && event.changes.some(change => change.text.length === 1 && /[A-Za-z_]/.test(change.text))) {
    clearTimeout(suggestTimer);
    suggestTimer = setTimeout(() => editor.trigger('keyboard', 'editor.action.triggerSuggest', {}), 0);
  }
  clearTimeout(compileTimer); statusElement.className = 'compile-status pending'; statusElement.textContent = 'Compiling…';
  diagnostic.className = 'editor-foot'; diagnosticMessage.textContent = '式を確認中…';
  compileTimer = setTimeout(() => compileNow(false), 260);
});
editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => compileNow(true));
editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => compileNow(false));
$('#compile-button').addEventListener('click', () => compileNow(false));
$('#preview-button').addEventListener('click', () => compileNow(true));

function setEditorSource(source, clean = false) {
  applyingRemote = true; editor.setValue(source); editor.setScrollTop(0); applyingRemote = false;
  if (clean) editorDirty = false;
}
function renderStatus(status) {
  if (status.generation < lastGeneration) return;
  lastGeneration = status.generation;
  const warnings = Array.isArray(status.warnings) ? status.warnings : [];
  const level = status.ok ? (warnings.length ? 'warning' : 'ok') : 'error';
  statusElement.className = `compile-status ${level}`;
  statusElement.textContent = status.ok
    ? `● Compiled · Generation ${status.generation}${warnings.length ? ` · ${warnings.length} warning${warnings.length === 1 ? '' : 's'}` : ''}`
    : `● ${status.line}:${status.column} ${status.message}`;
  diagnostic.className = `editor-foot ${level}`;
  diagnosticMessage.textContent = status.ok
    ? (warnings[0] ? `Warning · ${warnings[0]}` : 'Ready · audio program updated')
    : `${status.line}:${status.column} ${status.message}${status.hint ? ` — ${status.hint}` : ''}`;
  diagnosticMessage.title = status.ok ? warnings.join('\n') : (status.hint || '');
  const markerMessage = status.hint ? `${status.message}\nHint: ${status.hint}` : status.message;
  const markers = status.ok
    ? (warnings.length ? [{
      startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 2,
      message: warnings.join('\n'), severity: monaco.MarkerSeverity.Warning, source: 'Code Synthesizer',
    }] : [])
    : [{
      startLineNumber: Math.max(1, status.line), startColumn: Math.max(1, status.column), endLineNumber: Math.max(1, status.line),
      endColumn: Math.max(2, status.column + 1), message: markerMessage, severity: monaco.MarkerSeverity.Error, source: 'Code Synthesizer',
    }];
  monaco.editor.setModelMarkers(editor.getModel(), 'synth-compiler', markers);
}

diagnosticMessage.addEventListener('click', () => {
  const status = latestState?.status;
  if (!status?.ok) { editor.setPosition({ lineNumber: Math.max(1, status.line), column: Math.max(1, status.column) }); editor.focus(); }
});

function setMode(mode, persist = true) {
  currentMode = mode === 'play' ? 'play' : 'editor'; app.dataset.mode = currentMode;
  document.querySelectorAll('.mode-button').forEach(button => button.classList.toggle('active', button.dataset.mode === currentMode));
  if (persist) send({ cmd: 'setMode', mode: currentMode });
  setTimeout(() => editor.layout(), 0);
}
document.querySelectorAll('.mode-button').forEach(button => button.addEventListener('click', () => setMode(button.dataset.mode)));

async function pollState() {
  if (statePollInFlight) return;
  statePollInFlight = true;
  try {
    const response = await fetch('./api/state', { cache: 'no-store' });
    if (!response.ok) throw new Error(String(response.status));
    const state = await response.json();
    // ドラッグ中に到着した古いpoll結果で、編集中の位置・サイズを上書きしない。
    if (interacting && latestState?.controls) state.controls = latestState.controls;
    latestState = state;
    if (!initialized) {
      presetSelect.replaceChildren(...[...state.presets, 'Custom'].map(name => Object.assign(document.createElement('option'), { value: name, textContent: name })));
      setEditorSource(state.source, true); setMode(state.mode || 'editor', false); initialized = true;
    } else if (state.selectedPreset !== 'Custom' && editor.getValue() !== state.source) {
      setEditorSource(state.source, true);
    }
    presetSelect.value = state.selectedPreset; renderStatus(state.status);
    $('#sample-rate-badge').textContent = `${Math.round(state.sampleRate || 48000).toLocaleString()} Hz`;
    $('#voice-badge').textContent = `${state.activeVoices} voice${state.activeVoices === 1 ? '' : 's'}`;
    $('#scope-note').textContent = midiName(state.previewNote);
    syncMidiPreview(state.activeNotes || [], state.releaseNotes || []);
    if (!interacting) syncControls(state.parameters || [], state.controls || []);
  } catch (_error) {
    statusElement.className = 'compile-status error'; statusElement.textContent = 'UIブリッジを利用できません';
  } finally {
    statePollInFlight = false;
  }
}

function performPresetLoad(name, switchToEditor = false) {
  editorDirty = false; initialLayoutChecked = false; initialRepairJustApplied = false;
  layoutFingerprint = ''; pendingLayoutFingerprint = null; pendingLayoutControls = null;
  send({ cmd: 'loadPreset', name });
  if (switchToEditor) setMode('editor');
}
function closePresetConfirm() {
  const modal = $('#preset-confirm');
  modal.hidden = true; modal.setAttribute('aria-hidden', 'true'); pendingPresetLoad = null;
  presetSelect.value = latestState?.selectedPreset || 'Custom';
}
function openPresetConfirm(name, switchToEditor) {
  pendingPresetLoad = { name, switchToEditor };
  $('#preset-confirm-name').textContent = name;
  const modal = $('#preset-confirm');
  modal.hidden = false; modal.setAttribute('aria-hidden', 'false');
  presetSelect.value = latestState?.selectedPreset || 'Custom';
  setTimeout(() => $('#confirm-preset-load').focus(), 0);
}
function loadPreset(name, switchToEditor = false) {
  if (name === 'Custom') return;
  if (editorDirty) { openPresetConfirm(name, switchToEditor); return; }
  performPresetLoad(name, switchToEditor);
}
presetSelect.addEventListener('change', () => loadPreset(presetSelect.value));
$('#load-preset').addEventListener('click', () => loadPreset(presetSelect.value));
$('#cancel-preset-load').addEventListener('click', closePresetConfirm);
$('#confirm-preset-load').addEventListener('click', () => {
  const request = pendingPresetLoad;
  closePresetConfirm();
  if (request) performPresetLoad(request.name, request.switchToEditor);
});
$('#preset-confirm').addEventListener('pointerdown', event => {
  if (event.target.id === 'preset-confirm') closePresetConfirm();
});

function decimalsFor(step) { if (!step) return 3; if (step >= 1) return 0; return Math.min(4, Math.max(1, Math.ceil(-Math.log10(step)))); }
function normalizedDefault(spec) { return Math.max(0, Math.min(1, (spec.default - spec.min) / (spec.max - spec.min))); }
function currentSpec(index) { return latestState?.parameters?.find(parameter => parameter.index === index); }
function displayValue(spec, normalized) {
  let value = spec.min + normalized * (spec.max - spec.min);
  if (spec.step > 0) value = spec.min + Math.round((value - spec.min) / spec.step) * spec.step;
  return value.toFixed(decimalsFor(spec.step));
}

function syncControls(parameters, controls) {
  if (!initialLayoutChecked && controls.length > 1) {
    initialLayoutChecked = true;
    const overlaps = controls.some((left, leftIndex) => controls.slice(leftIndex + 1).some(right =>
      left.x < right.x + right.width && left.x + left.width > right.x &&
      left.y < right.y + right.height && left.y + left.height > right.y));
    if (overlaps) {
      const columns = Math.min(4, Math.max(2, Math.ceil(Math.sqrt(controls.length))));
      const width = Math.min(22, 96 / columns);
      controls.forEach((control, index) => {
        const column = index % columns; const row = Math.floor(index / columns);
        Object.assign(control, { x: 2 + column * (width + 2), y: 4 + row * 28, width, height: 22 });
      });
      pendingLayoutControls = controls.map(control => ({ ...control }));
      pendingLayoutFingerprint = JSON.stringify(controls.map(control => [control.name, control.kind, control.x, control.y, control.width, control.height]));
      layoutFingerprint = ''; initialRepairJustApplied = true;
      send({ cmd: 'setLayout', controls: pendingLayoutControls });
    }
  }
  let fingerprint = JSON.stringify(controls.map(control => [control.name, control.kind, control.x, control.y, control.width, control.height]));
  if (pendingLayoutFingerprint && fingerprint !== pendingLayoutFingerprint && pendingLayoutControls) {
    controls = pendingLayoutControls.map(control => ({ ...control }));
    latestState.controls = controls;
    fingerprint = pendingLayoutFingerprint;
  } else if (pendingLayoutFingerprint === fingerprint) {
    if (initialRepairJustApplied) initialRepairJustApplied = false;
    else { pendingLayoutFingerprint = null; pendingLayoutControls = null; }
  }
  if (fingerprint !== layoutFingerprint) { layoutFingerprint = fingerprint; renderControls(parameters, controls); }
  for (const spec of parameters) updateControlValue(spec.index, spec.normalized, spec);
  $('#empty-controls').hidden = parameters.length > 0;
  $('#parameter-count').textContent = `${parameters.length} parameter${parameters.length === 1 ? '' : 's'}`;
}

function renderControls(parameters, controls) {
  stage.querySelectorAll('.parameter-control').forEach(element => element.remove());
  const byName = new Map(parameters.map(parameter => [parameter.name, parameter]));
  for (const layout of controls) {
    const spec = byName.get(layout.name); if (!spec) continue;
    const control = document.createElement('div');
    control.className = 'parameter-control'; control.dataset.name = spec.name; control.dataset.index = spec.index;
    Object.assign(control.style, { left: `${layout.x}%`, top: `${layout.y}%`, width: `${layout.width}%`, height: `${layout.height}%` });
    const label = Object.assign(document.createElement('div'), { className: 'control-label', textContent: spec.label, title: spec.name });
    const value = Object.assign(document.createElement('output'), { className: 'control-value' });
    if (layout.kind === 'slider') {
      const input = Object.assign(document.createElement('input'), { type: 'range', min: '0', max: '1', step: '0.001', className: 'parameter-slider' });
      input.addEventListener('input', () => setParameter(spec.index, Number(input.value)));
      control.append(label, input, value);
    } else if (layout.kind === 'toggle') {
      const toggle = Object.assign(document.createElement('button'), { className: 'parameter-toggle', title: 'トグル' });
      toggle.addEventListener('click', () => { if (!arranging) { const fresh = currentSpec(spec.index); setParameter(spec.index, fresh?.normalized >= .5 ? 0 : 1); } });
      control.append(label, toggle, value);
    } else {
      const knob = Object.assign(document.createElement('div'), { className: 'knob', role: 'slider', tabIndex: 0 });
      installKnob(knob, spec.index); control.append(label, knob, value);
    }
    control.addEventListener('dblclick', event => { if (!arranging) { event.preventDefault(); const fresh = currentSpec(spec.index); if (fresh) setParameter(spec.index, normalizedDefault(fresh)); } });
    installArrangeDrag(control); stage.append(control);
  }
  for (const spec of parameters) updateControlValue(spec.index, spec.normalized, spec);
}

function setParameter(index, normalized) {
  normalized = Math.max(0, Math.min(1, normalized));
  const spec = currentSpec(index); if (spec) spec.normalized = normalized;
  updateControlValue(index, normalized, spec); send({ cmd: 'setUserParameter', index, value: normalized });
}

function updateControlValue(index, normalized, spec) {
  const control = stage.querySelector(`.parameter-control[data-index="${index}"]`); if (!control) return;
  const knob = control.querySelector('.knob'); if (knob) knob.style.setProperty('--value', normalized);
  const slider = control.querySelector('.parameter-slider'); if (slider && document.activeElement !== slider) slider.value = normalized;
  control.querySelector('.parameter-toggle')?.classList.toggle('on', normalized >= .5);
  const output = control.querySelector('.control-value'); if (output && spec) output.textContent = displayValue(spec, normalized);
  knob?.setAttribute('aria-valuenow', spec ? displayValue(spec, normalized) : normalized.toFixed(3));
}

function installKnob(knob, index) {
  knob.addEventListener('pointerdown', event => {
    if (arranging) return;
    event.preventDefault(); interacting = true; knob.setPointerCapture(event.pointerId);
    const startY = event.clientY; const start = currentSpec(index)?.normalized || 0;
    const move = moveEvent => setParameter(index, start + (startY - moveEvent.clientY) / 140);
    const up = () => { interacting = false; knob.removeEventListener('pointermove', move); knob.removeEventListener('pointerup', up); knob.removeEventListener('pointercancel', up); };
    knob.addEventListener('pointermove', move); knob.addEventListener('pointerup', up); knob.addEventListener('pointercancel', up);
  });
  knob.addEventListener('wheel', event => {
    event.preventDefault(); const spec = currentSpec(index); if (!spec) return;
    const amount = spec.step > 0 ? spec.step / (spec.max - spec.min) : .01;
    setParameter(index, spec.normalized + (event.deltaY < 0 ? amount : -amount));
  }, { passive: false });
  knob.addEventListener('keydown', event => {
    if (!['ArrowUp', 'ArrowRight', 'ArrowDown', 'ArrowLeft'].includes(event.key)) return;
    event.preventDefault(); const spec = currentSpec(index); if (spec) setParameter(index, spec.normalized + (['ArrowUp', 'ArrowRight'].includes(event.key) ? .01 : -.01));
  });
}

function layoutFor(name) { return latestState?.controls?.find(control => control.name === name); }
function installArrangeDrag(control) {
  control.addEventListener('pointerdown', event => {
    if (!arranging || event.button !== 0) return;
    event.preventDefault(); interacting = true; control.setPointerCapture(event.pointerId);
    // 1回の操作専用スナップショットを作り、pollの応答とは独立して編集する。
    const workingControls = (latestState?.controls || []).map(layout => ({ ...layout }));
    const layout = workingControls.find(layout => layout.name === control.dataset.name);
    if (!layout) { interacting = false; return; }
    latestState.controls = workingControls;
    const bounds = stage.getBoundingClientRect(); const box = control.getBoundingClientRect();
    const resizing = event.clientX > box.right - 14 && event.clientY > box.bottom - 14;
    const start = { x: event.clientX, y: event.clientY, left: layout.x, top: layout.y, width: layout.width, height: layout.height };
    const move = moveEvent => {
      if (resizing) {
        layout.width = Math.max(7, Math.min(100 - layout.x, start.width + (moveEvent.clientX - start.x) / bounds.width * 100));
        layout.height = Math.max(14, Math.min(100 - layout.y, start.height + (moveEvent.clientY - start.y) / bounds.height * 100));
      } else {
        layout.x = Math.max(0, Math.min(100 - layout.width, start.left + (moveEvent.clientX - start.x) / bounds.width * 100));
        layout.y = Math.max(0, Math.min(100 - layout.height, start.top + (moveEvent.clientY - start.y) / bounds.height * 100));
      }
      Object.assign(control.style, { left: `${layout.x}%`, top: `${layout.y}%`, width: `${layout.width}%`, height: `${layout.height}%` });
    };
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true; saveLayout(workingControls); interacting = false;
      control.removeEventListener('pointermove', move); control.removeEventListener('pointerup', finish); control.removeEventListener('pointercancel', finish);
    };
    control.addEventListener('pointermove', move); control.addEventListener('pointerup', finish); control.addEventListener('pointercancel', finish);
  });
}

function saveLayout(controls = latestState?.controls) {
  if (!latestState) return;
  const snapshot = (controls || []).map(control => ({ ...control }));
  latestState.controls = snapshot;
  layoutFingerprint = JSON.stringify(snapshot.map(control => [control.name, control.kind, control.x, control.y, control.width, control.height]));
  pendingLayoutFingerprint = layoutFingerprint;
  pendingLayoutControls = snapshot.map(control => ({ ...control }));
  send({ cmd: 'setLayout', controls: pendingLayoutControls });
}
function setControlKind(name, kind) {
  const layout = layoutFor(name); if (!layout) return;
  layout.kind = kind; saveLayout(); layoutFingerprint = ''; syncControls(latestState.parameters, latestState.controls);
}
function toggleArrange(force) {
  arranging = typeof force === 'boolean' ? force : !arranging;
  stage.classList.toggle('arranging', arranging); $('#arrange-button').classList.toggle('active', arranging);
  $('#arrange-hint').textContent = arranging
    ? 'Layout mode: ドラッグで移動、右下のハンドルでサイズ変更、プロジェクトに保存'
    : 'Guide: p_name = param(default, min, max, step) · 右クリックでVST操作';
}
$('#arrange-button').addEventListener('click', () => toggleArrange());
$('#reset-parameters').addEventListener('click', () => {
  for (const spec of latestState?.parameters || []) setParameter(spec.index, normalizedDefault(spec));
  showToast('すべてのパラメーターをリセットしました');
});

function openParameterGuide(event) {
  event?.preventDefault();
  const modal = $('#parameter-guide'); modal.hidden = false; modal.setAttribute('aria-hidden', 'false');
}
function openEditorGuide(event) {
  event?.preventDefault();
  const modal = $('#editor-guide'); modal.hidden = false; modal.setAttribute('aria-hidden', 'false');
}
function closeParameterGuide(event) {
  event?.preventDefault(); event?.stopPropagation();
  const modal = $('#parameter-guide'); modal.hidden = true; modal.setAttribute('aria-hidden', 'true');
}
function closeEditorGuide(event) {
  event?.preventDefault(); event?.stopPropagation();
  const modal = $('#editor-guide'); modal.hidden = true; modal.setAttribute('aria-hidden', 'true');
}
$('#parameter-guide-button').addEventListener('click', openParameterGuide);
$('#editor-guide-button').addEventListener('click', openEditorGuide);
$('#close-parameter-guide').addEventListener('click', closeParameterGuide);
$('#close-editor-guide').addEventListener('click', closeEditorGuide);
$('#load-parameter-guide').addEventListener('click', () => {
  closeParameterGuide(); loadPreset('Parameter Guide', true);
});
$('#parameter-guide').addEventListener('pointerdown', event => { if (event.target.id === 'parameter-guide') closeParameterGuide(); });

function menuItemsFor(control) {
  if (control) {
    const index = Number(control.dataset.index); const spec = currentSpec(index); const layout = layoutFor(control.dataset.name);
    return [
      { label: `Reset ${spec.label}`, shortcut: `${spec.default}`, action: () => setParameter(index, normalizedDefault(spec)) },
      { label: 'Copy parameter value', action: () => { copiedParameter = spec.normalized; showToast(`${spec.label} copied`); } },
      { label: 'Paste parameter value', disabled: copiedParameter === null, action: () => setParameter(index, copiedParameter) }, null,
      { label: 'Display as knob', checked: layout.kind === 'knob', action: () => setControlKind(layout.name, 'knob') },
      { label: 'Display as slider', checked: layout.kind === 'slider', action: () => setControlKind(layout.name, 'slider') },
      { label: 'Display as toggle', checked: layout.kind === 'toggle', action: () => setControlKind(layout.name, 'toggle') }, null,
      { label: arranging ? 'Finish arranging' : 'Arrange controls', action: () => toggleArrange() },
    ];
  }
  return [
    { label: 'Compile program', shortcut: 'Ctrl+S', action: () => compileNow(false) },
    { label: 'Preview current note', shortcut: 'Ctrl+Enter', action: () => compileNow(true) }, null,
    { label: currentMode === 'editor' ? 'Switch to Play' : 'Switch to Editor', action: () => setMode(currentMode === 'editor' ? 'play' : 'editor') },
    { label: 'Reset all parameters', action: () => $('#reset-parameters').click() },
    { label: 'Parameter & layout guide', action: openParameterGuide },
    { label: 'Release all notes', action: releaseAllNotes }, null,
    { label: 'Copy synth source', action: async () => {
      try { await navigator.clipboard.writeText(editor.getValue()); showToast('Source copied'); }
      catch { showToast('Clipboard unavailable in this host'); }
    } },
  ];
}

function openContextMenu(x, y, items) {
  contextMenu.replaceChildren();
  for (const item of items) {
    if (!item) { contextMenu.append(document.createElement('hr')); continue; }
    const button = document.createElement('button'); button.disabled = Boolean(item.disabled);
    const label = document.createElement('span'); label.textContent = `${item.checked ? '● ' : ''}${item.label}`;
    const shortcut = document.createElement('kbd'); shortcut.textContent = item.shortcut || '';
    button.append(label, shortcut);
    button.addEventListener('click', () => { contextMenu.hidden = true; item.action?.(); });
    contextMenu.append(button);
  }
  contextMenu.hidden = false;
  const width = 220; const height = contextMenu.offsetHeight;
  contextMenu.style.left = `${Math.max(4, Math.min(x, innerWidth - width - 6))}px`;
  contextMenu.style.top = `${Math.max(4, Math.min(y, innerHeight - height - 6))}px`;
}

document.addEventListener('contextmenu', event => {
  event.preventDefault(); openContextMenu(event.clientX, event.clientY, menuItemsFor(event.target.closest('.parameter-control')));
});
document.addEventListener('pointerdown', event => {
  if (!contextMenu.hidden && !contextMenu.contains(event.target)) contextMenu.hidden = true;
}, true);
document.addEventListener('keydown', event => {
  if (event.key === 'Escape') {
    contextMenu.hidden = true;
    closePresetConfirm(); closeParameterGuide(); closeEditorGuide();
    if (arranging) toggleArrange(false);
  }
});

let activeNotes = new Set();
const localNoteVelocities = new Map();
let midiNoteVelocities = new Map();
let releasingNoteVelocities = new Map();
function renderKeyHighlight(note) {
  const localVelocity = localNoteVelocities.get(note) || 0;
  const midiVelocity = midiNoteVelocities.get(note) || 0;
  const velocity = Math.max(localVelocity, midiVelocity);
  const pressed = velocity > 0;
  const releaseVelocity = releasingNoteVelocities.get(note) || 0;
  const releasing = releaseVelocity > 0;
  document.querySelectorAll(`.key[data-note="${note}"]`).forEach(key => {
    key.classList.toggle('active', localVelocity > 0);
    key.classList.toggle('midi-active', midiVelocity > 0);
    key.classList.toggle('release-active', releasing);
    if (pressed) {
      const brightness = Math.round(132 + velocity * 112);
      const black = key.classList.contains('black');
      const base = black ? Math.round(70 + velocity * 166) : brightness;
      key.style.backgroundColor = `rgb(${base} ${base} ${Math.max(0, base - (black ? 7 : 8))})`;
    } else {
      key.style.removeProperty('background-color');
    }
    if (releasing) {
      const outline = Math.round(112 + releaseVelocity * 143);
      const inset = Math.round(72 + releaseVelocity * 130);
      key.style.setProperty('--release-outline', `rgb(${outline} ${outline} ${Math.max(0, outline - 8)})`);
      key.style.setProperty('--release-inset', `rgb(${inset} ${inset} ${Math.max(0, inset - 8)})`);
    } else {
      key.style.removeProperty('--release-outline');
      key.style.removeProperty('--release-inset');
    }
  });
}
function syncMidiPreview(notes, releaseNotes) {
  const next = new Map(notes.map(item => {
    const note = typeof item === 'number' ? item : item.note;
    const velocity = typeof item === 'number' ? 0.9 : item.velocity;
    return [Number(note), Math.max(0.01, Math.min(1, Number(velocity) || 0.9))];
  }));
  const nextReleases = new Map(releaseNotes.map(item => [
    Number(item.note), Math.max(0, Math.min(1, Number(item.velocity) || 0)),
  ]).filter(([, velocity]) => velocity > 0));
  const changed = new Set([
    ...midiNoteVelocities.keys(), ...next.keys(),
    ...releasingNoteVelocities.keys(), ...nextReleases.keys(),
  ]);
  midiNoteVelocities = next;
  releasingNoteVelocities = nextReleases;
  for (const note of changed) renderKeyHighlight(note);
  const indicator = $('#midi-input-indicator');
  const latest = [...midiNoteVelocities.entries()].at(-1);
  if (latest) {
    const [note, velocity] = latest;
    const additional = midiNoteVelocities.size > 1 ? ` +${midiNoteVelocities.size - 1}` : '';
    indicator.textContent = `MIDI IN ${midiName(note)} · ${Math.round(velocity * 100)}%${additional}`;
    indicator.classList.add('active');
  } else {
    indicator.textContent = 'MIDI IN —'; indicator.classList.remove('active');
  }
}
let keyboardOctave = 4;
let keyboardVelocity = .9;
let sustainHeld = false;
const sustainedNotes = new Set();
function noteOn(note, key, velocity = .9) {
  if (activeNotes.has(note)) return;
  sustainedNotes.delete(note);
  activeNotes.add(note); localNoteVelocities.set(note, velocity); renderKeyHighlight(note);
  send({ cmd: 'setParameter', name: 'previewNote', value: note });
  send({ cmd: 'noteOn', note, velocity });
}
function noteOff(note, key) {
  if (!activeNotes.delete(note)) return;
  localNoteVelocities.delete(note); renderKeyHighlight(note);
  if (sustainHeld) { sustainedNotes.add(note); return; }
  send({ cmd: 'noteOff', note });
}
function releaseAllNotes() {
  sustainHeld = false; $('#sustain-button').classList.remove('active'); sustainedNotes.clear();
  for (const note of [...activeNotes]) {
    activeNotes.delete(note); localNoteVelocities.delete(note); renderKeyHighlight(note); send({ cmd: 'noteOff', note });
  }
}
function setSustain(held) {
  sustainHeld = held; $('#sustain-button').classList.toggle('active', held);
  if (!held) for (const note of [...sustainedNotes]) { sustainedNotes.delete(note); send({ cmd: 'noteOff', note }); }
}
function previewNote() {
  const note = latestState?.previewNote ?? 60; const key = document.querySelector(`.key[data-note="${note}"]`);
  noteOn(note, key); setTimeout(() => noteOff(note, key), 420);
}

function buildKeyboard() {
  const keyboard = $('#keyboard'); const start = (keyboardOctave + 1) * 12;
  const isBlack = note => [1, 3, 6, 8, 10].includes(note % 12);
  const visibleWidth = Math.max(420, keyboard.parentElement?.clientWidth || 900);
  const whiteTarget = Math.max(29, Math.min(61, Math.ceil(visibleWidth / 28)));
  const notes = []; let whiteNotes = 0; let note = start;
  while (whiteNotes < whiteTarget) { notes.push(note); if (!isBlack(note)) whiteNotes += 1; note += 1; }
  const whiteCount = notes.filter(note => !isBlack(note)).length;
  keyboard.replaceChildren(); keyboard.style.minWidth = '0';
  const whiteWidth = 100 / whiteCount; let whiteIndex = 0;
  for (const note of notes) {
    const black = isBlack(note); const key = document.createElement('button');
    key.className = `key ${black ? 'black' : 'white'}`; key.dataset.note = note; key.title = midiName(note);
    if (black) { key.style.left = `${whiteIndex * whiteWidth - whiteWidth * .31}%`; key.style.width = `${whiteWidth * .62}%`; }
    else { key.style.left = `${whiteIndex * whiteWidth}%`; key.style.width = `${whiteWidth}%`; key.textContent = note % 12 === 0 ? midiName(note) : ''; whiteIndex += 1; }
    key.addEventListener('pointerdown', event => { event.preventDefault(); key.setPointerCapture?.(event.pointerId); noteOn(note, key, keyboardVelocity); });
    const up = event => { event.preventDefault(); noteOff(note, key); };
    key.addEventListener('pointerup', up); key.addEventListener('pointercancel', up); keyboard.append(key); renderKeyHighlight(note);
  }
}

function setKeyboardOctave(delta) {
  keyboardOctave = Math.max(0, Math.min(8, keyboardOctave + delta));
  $('#keyboard-octave').textContent = `C${keyboardOctave}`; releaseAllNotes(); buildKeyboard();
}
$('#octave-down').addEventListener('click', () => setKeyboardOctave(-1));
$('#octave-up').addEventListener('click', () => setKeyboardOctave(1));
$('#panic-button').addEventListener('click', releaseAllNotes);
$('#sustain-button').addEventListener('pointerdown', event => { event.preventDefault(); setSustain(true); });
$('#sustain-button').addEventListener('pointerup', () => setSustain(false));
$('#sustain-button').addEventListener('pointerleave', () => setSustain(false));
$('#keyboard-velocity').addEventListener('input', event => {
  keyboardVelocity = Number(event.target.value) / 127;
  $('#velocity-value').textContent = `${Math.round(keyboardVelocity * 100)}%`;
});
new ResizeObserver(() => buildKeyboard()).observe($('#keyboard').parentElement);

const computerKeys = {
  z: 48, s: 49, x: 50, d: 51, c: 52, v: 53, g: 54, b: 55, h: 56, n: 57, j: 58, m: 59,
  ',': 60, q: 60, '2': 61, w: 62, '3': 63, e: 64, r: 65, '5': 66, t: 67, '6': 68, y: 69, '7': 70, u: 71,
};
const heldComputerKeys = new Set();
window.addEventListener('keydown', event => {
  if (event.repeat || event.ctrlKey || event.metaKey || event.altKey || event.target.closest?.('.monaco-editor, input, select')) return;
  const keyName = event.key.toLowerCase(); const note = computerKeys[keyName]; if (note === undefined) return;
  event.preventDefault(); heldComputerKeys.add(keyName); noteOn(note, document.querySelector(`.key[data-note="${note}"]`));
});
window.addEventListener('keyup', event => {
  const keyName = event.key.toLowerCase(); if (!heldComputerKeys.delete(keyName)) return;
  const note = computerKeys[keyName]; noteOff(note, document.querySelector(`.key[data-note="${note}"]`));
});
window.addEventListener('blur', () => { heldComputerKeys.clear(); releaseAllNotes(); });

function midiName(note) {
  const names = ['C', 'C♯', 'D', 'E♭', 'E', 'F', 'F♯', 'G', 'A♭', 'A', 'B♭', 'B'];
  return `${names[note % 12]}${Math.floor(note / 12) - 1}`;
}

const SCOPE_FRAME_LENGTH = 768;
let previousWave = null;
function phaseLockedFrame(raw) {
  if (!raw?.length) return [];
  const samples = raw.map(value => Number.isFinite(value) ? value : 0);
  const frameLength = Math.min(SCOPE_FRAME_LENGTH, samples.length);
  const maxOffset = Math.max(0, samples.length - frameLength);
  const energy = samples.reduce((sum, value) => sum + value * value, 0);
  if (energy / samples.length < 1e-8) { previousWave = null; return samples.slice(0, frameLength); }
  let offset = 0;
  if (previousWave?.length === frameLength) {
    let best = -Infinity; const compare = Math.min(128, frameLength);
    for (let shift = 0; shift <= maxOffset; shift += 2) {
      let dot = 0; let oldPower = 0; let newPower = 0;
      for (let index = 0; index < compare; index += 1) {
        const a = previousWave[index]; const b = samples[index + shift];
        dot += a * b; oldPower += a * a; newPower += b * b;
      }
      const score = dot / Math.sqrt(Math.max(1e-12, oldPower * newPower));
      if (score > best) { best = score; offset = shift; }
    }
  } else {
    let strongest = -Infinity;
    for (let index = 1; index <= maxOffset; index += 1) {
      if (samples[index - 1] <= 0 && samples[index] > 0 && samples[index] - samples[index - 1] > strongest) {
        strongest = samples[index] - samples[index - 1]; offset = index;
      }
    }
  }
  // 回転させません。信号が完全な周期でない場合、ライブフレームを折り返すと
  // 右端に不連続が見えてしまいます。
  const frame = samples.slice(offset, offset + frameLength);
  previousWave = frame.slice(); return frame;
}

function drawScope(raw) {
  const samples = phaseLockedFrame(raw); const rect = scope.getBoundingClientRect(); const ratio = devicePixelRatio || 1;
  const width = Math.max(1, Math.round(rect.width * ratio)); const height = Math.max(1, Math.round(rect.height * ratio));
  if (scope.width !== width || scope.height !== height) { scope.width = width; scope.height = height; }
  const ctx = scope.getContext('2d'); ctx.clearRect(0, 0, width, height);
  const centerY = height / 2;
  const plotHalfHeight = Math.min(centerY, height - centerY) * .85;
  ctx.strokeStyle = '#282828'; ctx.lineWidth = ratio;
  ctx.beginPath(); ctx.moveTo(0, centerY); ctx.lineTo(width, centerY); ctx.stroke();
  ctx.strokeStyle = '#a9b7e6'; ctx.lineWidth = 1.35 * ratio; ctx.beginPath(); let peak = 0;
  samples.forEach((sample, index) => {
    peak = Math.max(peak, Math.abs(sample)); const inset = ctx.lineWidth * .5;
    const x = inset + index / Math.max(1, samples.length - 1) * (width - inset * 2);
    const y = centerY - Math.max(-1.5, Math.min(1.5, sample)) / 1.5 * plotHalfHeight;
    index ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
  });
  ctx.stroke(); $('#output-meter').style.width = `${Math.min(100, peak / 1.5 * 100)}%`; $('#peak-value').textContent = peak.toFixed(2);
}

async function updateScope() {
  try {
    const response = await fetch('./api/waveform', { cache: 'no-store' }); const wave = await response.json();
    $('#scope-mode').textContent = wave.live ? `live · ${wave.activeVoices} voices` : 'compiled preview'; drawScope(wave.samples);
  } catch { /* 次のフレームで再試行 */ }
}

buildKeyboard();
window.__CODE_SYNTH_UI_READY__ = true;
$('#boot-status')?.remove();
send({ cmd: 'uiReady' });
pollState(); updateScope();
setInterval(pollState, 80);
setInterval(updateScope, 40);
