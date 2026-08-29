# Third-party material

Nomo itself is MIT licensed, and everything under
`crates/`, `web/`, `examples/`, `tests/`, `scripts/` and `docs/` is this
project's own work released under those terms.

Nothing third-party is redistributed from this repository. What follows is what
the project *uses* — where it comes from, what it is for, and how to obtain it.

## The SMath corpora

The `.sm` worksheets the importer is measured against are other people's
documents. They carry no redistribution terms, so they are fetched on demand
rather than committed:

```bash
./scripts/fetch-corpora.sh          # download and verify
./scripts/fetch-corpora.sh --verify # check what is already there
```

`corpora/` is gitignored. What *is* committed is the provenance — every source
URL and the SHA-256 of every file it must yield, under `scripts/corpora/` — so a
fetched corpus is demonstrably the same one the baselines in `tests/corpus/`
were recorded against. A corpus that drifted upstream fails the check rather
than quietly moving the numbers.

| Set | 54 + 60 worksheets | Source | Terms |
|---|---|---|---|
| `nomo-corpus/` | All 48 published SMath wiki examples, unpacking to 54 `.sm` files, versions 0.82–0.98 | <https://smath.com/wiki/Examples.ashx> | Third-party worksheets by named authors. No redistribution terms are stated on the examples page. |
| `technical-mechanics-samples/` | 60 worksheets from Martin Kraska's *Technische Mechanik mit SMath Studio*, SMath 1.3–1.5 | <https://github.com/sn-code-inside/Technische-Mechanik-mit-SMath> | Springer Nature book-companion material ([ISBN 978-3-658-50591-2](https://doi.org/10.1007/978-3-658-50592-9)). The repository states no licence, so all rights are reserved. |

Both are used here as **measurement inputs** — the importer reads them, and
their stored answers act as an oracle for the engine. No part of either is
copied into Nomo's own code or examples. Design note §8 is the analysis built
on them; the numbers it quotes are measurements, not excerpts.

If you are running CI, you may set `NOMO_CORPORA_MIRROR` to a location holding
the same archives by name. It is optional — the script fetches from the sites
above unaided — but it is the reliable route: the wiki serves files behind a
cookie-detection redirect, and GitHub's generated tarballs are not byte-stable.
Per-file hashes are checked either way, which is what makes a mirror
trustworthy. A `file://` mirror built from corpora already on disk is also how
the fetch path is tested without asking anything of the upstream sites; the
recipe is in `docs/STATUS.md`.

## Two industrial worksheets, no longer here

Development used two real engineering worksheets from an on-board-charger
project — a resonant-converter design and an interlock-monitor design. They were
the worked examples much of the importer was built against, and several comments
in `crates/nomo-smath/` still cite what they measured: figures placed at
two-thirds of their stored width, a page header storing a date as arithmetic, a
second `<regions>` block, a definition free in symbols the file never binds.

They were **customer documents, not this project's to publish**, and they have
been removed along with everything derived from them. The measurements stand;
the files do not. Where a comment says "the converter worksheet" or "the
interlock worksheet", that is what it means.

`examples/llc.nomo` and `examples/interlock.nomo` are clean-room replacements,
written here from public engineering practice — the first-harmonic method for
resonant converters, and the two-channel measurement that separates an interlock
loop's four states. They share no text, no numbers and no figures with what they
replace. Their diagrams are drawn by `scripts/make-example-figures.py`, which
depends on nothing outside the Python standard library.

## The reference codebases

`reference/CalcpadCE/` and `reference/EngineeringPaper.xyz-main/` are two
unrelated third-party projects solving the same problem with different
architectures. They are gitignored, have never been committed, and exist **to be
learned from, not extended** — design note §10 records what to take from each.
Obtain them yourself if you want them; they are not required by any build or
test.

## Build dependencies

| Dependency | Used by | Licence |
|---|---|---|
| [`libm`](https://crates.io/crates/libm) | `nomo-core` | MIT OR Apache-2.0 |
| [`roxmltree`](https://crates.io/crates/roxmltree) | `nomo-smath` | MIT OR Apache-2.0 |
| [CodeMirror 6](https://codemirror.net/) (`@codemirror/*`) | `web/` | MIT |
| [esbuild](https://esbuild.github.io/) | `web/` build only | MIT |

`web/dist/` bundles the CodeMirror packages; see `NOTICE` for the attribution
that ships with it.
