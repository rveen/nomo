// Completion, hover and go-to-definition, from what the engine knows.
//
// None of this parses anything. The engine already knows which names a
// worksheet binds, what each came to and where it was written, and it says so
// in the analysis payload after every edit; the editor's job is to put that in
// front of a reader at the moment it answers a question they were about to ask.
//
// That division is the same one the highlighting follows, and for the same
// reason: a second parser in the front end is a second answer to "what does this
// worksheet mean", and the two can disagree. See `docs/design-note.md` §10 on
// CalcpadCE, which maintains two.

import { autocompletion } from "@codemirror/autocomplete";
import { EditorView, hoverTooltip } from "@codemirror/view";

/// What the last analysis said. Replaced wholesale after every edit.
let current = { symbols: [], vocabulary: null };

/** Record the engine's latest answer, for the three features below to read. */
export function setSymbols(symbols) {
  current.symbols = symbols ?? [];
}

/** Record the one-time vocabulary: units, functions, packs, keywords. */
export function setVocabulary(vocabulary) {
  current.vocabulary = vocabulary;
}

/** The name under `pos`, and where it starts and ends. */
function nameAt(state, pos) {
  const line = state.doc.lineAt(pos);
  const text = line.text;
  const at = pos - line.from;
  // The same shape the lexer accepts: a letter or underscore, then letters,
  // digits and underscores. Written here rather than imported because the
  // editor needs it over a *partial* line, which the lexer never sees.
  const isNamePart = (c) => c !== undefined && /[\p{L}\p{N}_.]/u.test(c);
  let from = at;
  while (from > 0 && isNamePart(text[from - 1])) from -= 1;
  let to = at;
  while (to < text.length && isNamePart(text[to])) to += 1;
  if (from === to) return null;
  return { text: text.slice(from, to), from: line.from + from, to: line.from + to };
}

/**
 * Completions, ordered by how likely they are to be what was meant.
 *
 * A name this worksheet binds comes first — it is the only kind the reader
 * definitely knows about, because they wrote it — then units, then functions,
 * then the keywords and packs. Each carries what it is worth knowing at the
 * moment of choosing: a variable's value, a unit's dimension, a function's
 * name. `ksi` and `kip` are one letter apart and mean different things, and the
 * dimension beside them is what tells them apart.
 */
function completions(context) {
  const word = context.matchBefore(/[\p{L}_][\p{L}\p{N}_.]*/u);
  if (!word || (word.from === word.to && !context.explicit)) return null;

  const v = current.vocabulary ?? { units: [], functions: [], packs: [], keywords: [] };
  const options = [
    ...current.symbols.map((s) => ({
      label: s.name,
      type: s.kind === "function" ? "function" : "variable",
      detail: s.detail,
      boost: s.kind === "pack" ? 1 : 2,
    })),
    ...v.units.map((u) => ({ label: u.name, type: "type", detail: u.detail, boost: 0 })),
    ...v.functions.map((f) => ({ label: f, type: "function", boost: -1 })),
    ...v.packs.map((p) => ({ label: p.name, type: "namespace", detail: p.detail, boost: -1 })),
    ...v.keywords.map((k) => ({ label: k, type: "keyword", boost: -2 })),
  ];

  return { from: word.from, options };
}

/**
 * What a name is, shown where it is written.
 *
 * The question a reader asks of a worksheet they did not write is "what is that,
 * and in what units" — and the answer is three hundred lines above, or in a
 * pack, or in the unit table. This is that answer without the journey.
 */
const hover = hoverTooltip((view, pos) => {
  const found = nameAt(view.state, pos);
  if (!found) return null;

  const symbol = current.symbols.find((s) => s.name === found.text);
  const v = current.vocabulary;
  const unit = v?.units.find((u) => u.name === found.text);
  const isFunction = v?.functions.includes(found.text);
  const pack = v?.packs.find((p) => p.name === found.text);

  let title;
  let detail;
  if (symbol) {
    title = symbol.name;
    detail =
      symbol.kind === "function"
        ? symbol.detail
        : `${symbol.detail}${symbol.kind === "pack" ? "  — from a pack" : ""}`;
  } else if (unit) {
    title = `${unit.name} — unit`;
    detail = unit.detail;
  } else if (pack) {
    title = `${pack.name} — pack`;
    detail = pack.detail;
  } else if (isFunction) {
    title = `${found.text} — built in`;
    detail = "";
  } else {
    return null;
  }

  return {
    pos: found.from,
    end: found.to,
    above: true,
    create() {
      const dom = document.createElement("div");
      dom.className = "cm-nomo-tooltip";
      const name = document.createElement("strong");
      name.textContent = title;
      dom.append(name);
      if (detail) {
        const value = document.createElement("div");
        value.textContent = detail;
        dom.append(value);
      }
      return { dom };
    },
  };
});

/**
 * Jump to where the name under the cursor was defined.
 *
 * The engine gives the defining occurrence rather than the first mention, so
 * this lands on the line that binds the name — and, for a name a pack supplied,
 * on the `use` line that brought it in, which is the line in *this* worksheet
 * responsible for it.
 */
export function goToDefinition(view) {
  const found = nameAt(view.state, view.state.selection.main.head);
  if (!found) return false;
  const symbol = current.symbols.find((s) => s.name === found.text);
  if (!symbol) return false;
  view.dispatch({
    selection: { anchor: symbol.from, head: symbol.to },
    scrollIntoView: true,
  });
  return true;
}

/** The three of them, as one extension. */
export const assist = [
  autocompletion({ override: [completions], activateOnTyping: true }),
  hover,
  EditorView.theme({
    ".cm-nomo-tooltip": {
      padding: "0.35rem 0.5rem",
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: "0.85rem",
      lineHeight: "1.4",
    },
  }),
];
