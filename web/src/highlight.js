// Syntax highlighting from the engine's classified tokens.
//
// There is no CodeMirror language mode here and no grammar of any kind. The
// engine sends a list of `{from, to, class}` and this turns it into decorations.
//
// That is the whole point. A CodeMirror mode would be a second description of
// what the language contains, and the moment the engine learned a unit the mode
// did not, the editor would colour a worksheet differently from how it computes
// — which is the split CalcpadCE has between its core and its highlighter and
// which the design note calls a permanent liability. A list of ranges cannot
// disagree with the engine, because it is not deciding anything.
//
// It also does better than a grammar could: `m` is coloured as a unit or as a
// variable depending on whether this worksheet bound it, which is knowable only
// after evaluation.

import { Decoration, EditorView } from "@codemirror/view";
import { StateEffect, StateField, RangeSetBuilder } from "@codemirror/state";

/** Replace the current highlighting. */
export const setTokens = StateEffect.define();

const marks = new Map(
  [
    "number",
    "comment",
    "keyword",
    "operator",
    "bracket",
    "separator",
    "variable",
    "unit",
    "function",
    "constant",
    "text",
    "unresolved",
  ].map((name) => [name, Decoration.mark({ class: `tok-${name}` })]),
);

/**
 * Build a decoration set, skipping anything the document no longer covers.
 *
 * Tokens arrive from a worker-like boundary and describe the text as it was when
 * analysis ran. A keystroke can land first, so a range may point past the end of
 * the document; CodeMirror throws on that rather than ignoring it.
 */
function build(tokens, docLength) {
  const builder = new RangeSetBuilder();
  for (const token of tokens) {
    if (token.from >= token.to) continue;
    if (token.to > docLength) break;
    const mark = marks.get(token.class);
    if (mark) builder.add(token.from, token.to, mark);
  }
  return builder.finish();
}

export const highlighting = StateField.define({
  create() {
    return Decoration.none;
  },
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setTokens)) {
        return build(effect.value, tr.state.doc.length);
      }
    }
    // Between analyses, move the existing marks with the text so highlighting
    // does not visibly lag a keystroke.
    return tr.docChanged ? value.map(tr.changes) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});
