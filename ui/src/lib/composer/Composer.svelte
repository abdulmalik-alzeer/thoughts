<script lang="ts">
  // The message box of a chat, formatted as it is typed (mdLive.ts): what is sent is exactly the
  // markdown that was typed. Built on CodeMirror, which keeps the browser's own text handling —
  // input methods (Arabic, Japanese…), the selection, spelling, screen readers (it is a text box,
  // `role="textbox"`, multi-line) — and adds its own undo and redo. The owner talks to it as to a
  // <textarea>: `value` is bound, `box` has the selection calls the chat and dictation use, and
  // keydown and paste go to the owner first (calling preventDefault keeps them from the editor).
  import { onMount, untrack } from 'svelte';
  import { Annotation, Compartment, EditorState, Prec, type Extension } from '@codemirror/state';
  import { EditorView, keymap, placeholder as placeholderExt } from '@codemirror/view';
  import { history, historyKeymap, insertNewline, standardKeymap } from '@codemirror/commands';
  import { markdownLive } from './mdLive';
  import type { TextBox } from './textBox';

  let {
    value = $bindable(''), box = $bindable(), placeholder = '', label = '', id = undefined,
    describedby = undefined, controls = undefined, activedescendant = undefined,
    disabled = false, expanded = false, onkeydown, onpaste, oninput, class: cls = '',
  }: {
    value?: string; box?: (TextBox & { element: HTMLElement }) | undefined; placeholder?: string; label?: string; id?: string;
    describedby?: string; controls?: string; activedescendant?: string;
    disabled?: boolean; expanded?: boolean; class?: string;
    onkeydown?: (e: KeyboardEvent) => void; onpaste?: (e: ClipboardEvent) => void; oninput?: () => void;
  } = $props();

  let host = $state<HTMLDivElement>();
  let view: EditorView | undefined;
  /** A change made by the owner (`value` set from outside), not typed. */
  const External = Annotation.define<boolean>();
  const attrsC = new Compartment(), phC = new Compartment(), editC = new Compartment();

  const attrs = () => {
    const a: Record<string, string> = { 'aria-multiline': 'true', spellcheck: 'true', autocorrect: 'on', autocapitalize: 'sentences', 'data-composer': '' };
    if (label) a['aria-label'] = label;
    if (id) a.id = id;
    if (describedby) a['aria-describedby'] = describedby;
    if (controls) a['aria-controls'] = controls;
    if (activedescendant) a['aria-activedescendant'] = activedescendant;
    if (disabled) a['aria-disabled'] = 'true';
    return a;
  };
  const editable = (): Extension => [EditorView.editable.of(!disabled), EditorState.readOnly.of(disabled)];

  // The editing keys of a text box, without the editor's extras that would take the app's own
  // shortcuts (⌘[ and ⌘] go back and forward, ⌘Enter sends). Enter adds a plain line break.
  const keys = [
    ...standardKeymap.filter((k) => k.key !== 'Enter'),
    { key: 'Enter', run: insertNewline, shift: insertNewline },
    ...historyKeymap,
  ];

  function extensions(): Extension[] {
    return [
      history(),
      Prec.highest(EditorView.domEventHandlers({
        keydown: (e) => { onkeydown?.(e); return e.defaultPrevented; },
        paste: (e) => { onpaste?.(e); return e.defaultPrevented; },
        // Files dropped on the box are attached by the chat around it, never read into the text.
        drop: (e) => {
          if (![...(e.dataTransfer?.types ?? [])].includes('Files')) return false;
          e.preventDefault();
          return true;
        },
      })),
      keymap.of(keys),
      EditorView.lineWrapping,
      markdownLive,
      attrsC.of(EditorView.contentAttributes.of(attrs())),
      phC.of(placeholder ? placeholderExt(placeholder) : []),
      editC.of(editable()),
      EditorView.updateListener.of((u) => {
        if (!u.docChanged) return;
        const doc = u.state.doc.toString();
        if (doc !== value) value = doc;
        if (u.transactions.some((tr) => tr.docChanged && !tr.annotation(External))) oninput?.();
      }),
    ];
  }
  const fresh = (doc: string) => EditorState.create({ doc, selection: { anchor: doc.length }, extensions: extensions() });

  onMount(() => {
    view = new EditorView({ parent: host!, state: fresh(untrack(() => value)) });
    return () => { view?.destroy(); view = undefined; };
  });

  // `value` set by the owner: a message sent (the box empties, with nothing left to undo into, as a
  // text box does) or words put in (a mention, a suggestion, a draft): a change that can be undone.
  $effect(() => {
    const v = value ?? '';
    if (!view) return;
    untrack(() => {
      const cur = view!.state.doc.toString();
      if (v === cur) return;
      if (!v) { const focused = view!.hasFocus; view!.setState(fresh('')); if (focused) view!.focus(); return; }
      let a = 0;
      while (a < v.length && a < cur.length && v[a] === cur[a]) a++;
      let b = 0;
      while (b < v.length - a && b < cur.length - a && v[v.length - 1 - b] === cur[cur.length - 1 - b]) b++;
      view!.dispatch({ changes: { from: a, to: cur.length - b, insert: v.slice(a, v.length - b) }, annotations: External.of(true) });
    });
  });
  $effect(() => { const a = attrs(); view?.dispatch({ effects: attrsC.reconfigure(EditorView.contentAttributes.of(a)) }); });
  $effect(() => { const p = placeholder; view?.dispatch({ effects: phC.reconfigure(p ? placeholderExt(p) : []) }); });
  $effect(() => { disabled; view?.dispatch({ effects: editC.reconfigure(editable()) }); });

  const clamp = (n: number) => Math.max(0, Math.min(n | 0, view?.state.doc.length ?? 0));
  box = {
    get value() { return view?.state.doc.toString() ?? value ?? ''; },
    get selectionStart() { return view?.state.selection.main.from ?? 0; },
    get selectionEnd() { return view?.state.selection.main.to ?? 0; },
    get element() { return (view?.contentDOM ?? host)!; },
    focus(o?: FocusOptions) {
      if (!view) return;
      if (o?.preventScroll) view.contentDOM.focus({ preventScroll: true }); else view.focus();
    },
    setSelectionRange(s: number, e: number) {
      if (!view) return;
      view.dispatch({ selection: { anchor: clamp(s), head: clamp(e) }, scrollIntoView: true });
    },
    setRangeText(text: string, s: number, e: number, mode: SelectionMode = 'preserve') {
      if (!view) return;
      const from = clamp(s), to = clamp(e), end = from + text.length;
      // 'preserve': the selection moves with the text, as the editor maps it.
      const selection = mode === 'end' ? { anchor: end } : mode === 'start' ? { anchor: from } : mode === 'select' ? { anchor: from, head: end } : undefined;
      view.dispatch({ changes: { from, to, insert: text }, selection, scrollIntoView: true, userEvent: 'input' });
    },
    dispatchEvent: () => true,
  };
</script>

<div class="composer-box {cls}" class:expanded class:disabled bind:this={host}></div>

<style>
  .composer-box { min-width: 0; }
  .composer-box :global(.cm-editor) { background: transparent; color: var(--fg); font: inherit; }
  .composer-box :global(.cm-editor.cm-focused) { outline: none; }
  .composer-box :global(.cm-scroller) { font-family: inherit; line-height: 1.5; min-height: var(--composer-min, 56px); max-height: var(--composer-max, 40vh); overflow-y: auto; }
  .composer-box.expanded :global(.cm-scroller) { height: var(--composer-tall, 55vh); max-height: none; }
  .composer-box :global(.cm-content) { padding: 6px 2px; caret-color: var(--fg); }
  .composer-box :global(.cm-line) { padding: 0 4px; text-align: start; }
  .composer-box :global(.cm-placeholder) { color: var(--muted); }
  .composer-box.disabled :global(.cm-content) { opacity: .6; cursor: default; }

  /* Markdown as it is typed: the marks stay, dimmed; what they mark takes its look. */
  .composer-box :global(.cmd-mark) { color: var(--muted); }
  .composer-box :global(.cmd-strong) { font-weight: 650; }
  .composer-box :global(.cmd-em) { font-style: italic; }
  .composer-box :global(.cmd-strike) { text-decoration: line-through; }
  .composer-box :global(.cmd-code) { font-family: var(--mono); font-size: .9em; background: var(--code-bg); border-radius: 4px; padding: 1px 0; }
  .composer-box :global(.cmd-link) { color: var(--accent); text-decoration: underline; text-underline-offset: 2px; }
  .composer-box :global(.cmd-label), .composer-box :global(.cmd-info) { color: var(--muted); }
  .composer-box :global(.cmd-h) { font-weight: 650; }
  .composer-box :global(.cmd-h1) { font-size: 1.3em; }
  .composer-box :global(.cmd-h2) { font-size: 1.18em; }
  .composer-box :global(.cmd-h3) { font-size: 1.08em; }
  .composer-box :global(.cmd-quote) { border-inline-start: 3px solid var(--border); padding-inline-start: 10px; color: var(--muted); }
  .composer-box :global(.cmd-block) { font-family: var(--mono); font-size: .9em; background: var(--code-bg); padding-inline: 10px; }
  .composer-box :global(.cmd-block .cmd-code) { background: none; padding: 0; }
  .composer-box :global(.cmd-block-first) { border-start-start-radius: 8px; border-start-end-radius: 8px; padding-top: 4px; }
  .composer-box :global(.cmd-block-last) { border-end-start-radius: 8px; border-end-end-radius: 8px; padding-bottom: 4px; }
</style>
