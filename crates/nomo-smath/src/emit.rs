//! Writing a Nomo worksheet from what the reader understood.
//!
//! The output is `.nomo` **source text**, not an engine data structure. That is
//! the deliverable a migrating user actually receives, it diffs and reviews like
//! code, and going through the real parser means the importer is tested against
//! the language rather than against a private back door into it.
//!
//! # Nothing is dropped and nothing is guessed
//!
//! Every construct that cannot be translated becomes a marker comment in the
//! output — visible to the person reviewing the migration — and a [`Note`]
//! carrying its line, so the same information is countable. A worksheet that
//! imports with twelve markers is a worksheet somebody has to look at, and
//! saying so is the entire job (design note §8.7 items 21 and 23).
//!
//! # Where SMath and Nomo genuinely disagree
//!
//! Two gaps are structural rather than a matter of unimplemented functions, and
//! both are recorded as notes rather than papered over:
//!
//! * **A globally-scoped function.** SMath's `≡` binds at document scope, and 19
//!   corpus regions use it to define a *function*. Nomo's `global` takes a name
//!   only, and `fn` is positional, so the scope is flattened and the note says
//!   so. If such a function is called above its definition, the imported
//!   worksheet will not resolve it — which is a missing Nomo feature, not a bad
//!   import.
//! * **Names.** 289 of the 837 distinct operand names in the corpus are not
//!   legal Nomo names, almost all because SMath writes a subscript with a `.`.
//!   They are respelled, every respelling is a note, and any two names that
//!   would collide after respelling are an error rather than a silent merge.

use std::collections::{BTreeMap, BTreeSet};

use crate::expr::{Assign, Expr, Statement};
use crate::read::{decoded_len, Math, Payload, PlotView, ResultKind, Worksheet};

/// A Nomo worksheet, plus everything a reviewer needs to know about how it got
/// that way.
#[derive(Debug, Clone, Default)]
pub struct Emitted {
    pub source: String,
    pub notes: Vec<Note>,
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// 1-based line in `source`.
    pub line: usize,
    pub kind: NoteKind,
    pub detail: String,
}

/// An image lifted out of the body and into the trailer.
#[derive(Debug, Clone)]
struct Resource {
    /// What the body refers to it by: `figure1`, `figure2`, in reading order.
    name: String,
    format: String,
    /// Base64, exactly as SMath stored it.
    data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoteKind {
    /// No translation exists; a marker comment stands in for the construct.
    Unsupported,
    /// Carried into the output whole, but nothing displays it yet.
    ///
    /// Deliberately neither of the two things it is nearly. Not `Unsupported`:
    /// the data survives the import, and counting it there would say a
    /// worksheet had lost something it still has. Not silence either, because a
    /// reviewer opening the imported file still cannot see the figure, and the
    /// rule is that a gap is visible and counted.
    Carried,
    /// A name had to be respelled to be legal Nomo.
    Renamed,
    /// Two SMath names respell to one Nomo name. The worksheet is wrong as
    /// emitted, and this says where.
    Collision,
    /// A global function definition became a positional one, because Nomo has
    /// no `global fn`.
    ScopeFlattened,
}

/// An answer SMath stored, ready to be checked against what Nomo computes.
#[derive(Debug, Clone, PartialEq)]
pub struct Assertion {
    /// 1-based line in `source` whose result should match.
    pub line: usize,
    /// The stored answer as a Nomo expression. Always a literal — a stored
    /// answer never refers to a worksheet variable — so it evaluates on its own.
    pub expected: String,
    /// The first numeric literal in the stored answer, verbatim.
    ///
    /// This is where the answer's precision actually lives, and reading it off
    /// the literal beats deriving it from the document's `precision` setting.
    /// SMath stores a large result in scientific form — `1.8491*10^5` — and
    /// `precision` counts decimals **of the mantissa**, so treating it as
    /// decimals of the value makes the tolerance 100000 times too tight. The
    /// literal cannot lie about itself: `1.8491` says ±0.00005 of a mantissa,
    /// and scaling that by `expected / 1.8491` gives the tolerance in base SI
    /// whatever the units and whatever the exponent.
    pub mantissa: String,
    /// The first numeric literal of **each part**, when the stored answer has
    /// several; empty for every plain scalar answer.
    ///
    /// One mantissa cannot state the tolerance for a whole table: SMath writes
    /// each part to its own significant places, and `mat(0.757, 1.74, 2, 1)` is
    /// one answer to three decimals beside another to two. This arrived with
    /// the several-roots case that `solve` produces, and covers complex answers
    /// too — `196.18 - 20.29·i` is five significant figures beside four.
    pub elements: Vec<String>,
}

pub fn emit(w: &Worksheet) -> Emitted {
    emit_in(w, None)
}

/// Emit, preferring `language` wherever a region carries more than one.
///
/// The design note is explicit that which language an import keeps is a policy
/// decision rather than a first-match (§8.9, §8.10). Making it an argument is
/// what turns it into one: the caller chooses, the choice is recorded in the
/// notes, and a region without the requested language falls back to its first
/// rather than losing its prose.
pub fn emit_in(w: &Worksheet, language: Option<&str>) -> Emitted {
    let mut e = Emitter::new(w);
    e.language = language.map(str::to_string);
    e.run(w)
}

struct Emitter {
    out: String,
    line: usize,
    /// Definitions lifted out of the line currently being built, to be written
    /// above it. See [`Emitter::lift`].
    pending: Vec<String>,
    lifted: usize,
    /// The language an import prefers, if the caller named one.
    language: Option<String>,
    dropped_languages: usize,
    notes: Vec<Note>,
    assertions: Vec<Assertion>,
    names: Names,
    /// Every name the document defines, for deciding which are free. See
    /// [`Bound`].
    bound: Bound,
    /// The functions this worksheet defines for itself, so a call to one can be
    /// written out instead of reported as unknown. See [`defined_functions`].
    functions: BTreeSet<String>,
    /// The names a plot draws that are candidates for being functions of `x`.
    /// See [`curves_of_x`] and [`Emitter::curve_of_x`].
    curves: BTreeSet<String>,
    /// Those of [`Emitter::curves`] that were actually written as `fn n(x) =
    /// …`, which is what a plot may apply to `x`. A candidate whose definition
    /// did not import is still just a name, and a marker that called it `P(x)`
    /// would describe a function nothing wrote.
    curves_emitted: BTreeSet<String>,
    /// Names whose definition is a `sys(…)` — a *list of series*, which is a
    /// plot rather than a value — mapped to that list. See [`Emitter::curve`].
    series: BTreeMap<String, Expr>,
    /// The names that hold a value in the emitted worksheet, so far.
    ///
    /// [`Bound`] answers a different question: whether the *SMath file* defines
    /// a name anywhere, which is what deciding "free symbol" needs. This is
    /// whether the *import* defines it — a distinction that matters wherever a
    /// name is bound by something that did not translate. `NASA_atmosphere.sm`
    /// fills its table inside a `for` loop with `el(M, i, 1) ← …`: `M` is bound
    /// as far as the file is concerned and empty as far as the output is.
    ///
    /// It is transitive, because having a value is: `XY : augment(x, y)`
    /// imports perfectly and still has no value when `y` was built by a loop.
    /// So a live definition contributes its target only when everything it
    /// reads already has a value, and takes the target back out when it does
    /// not — a name can be redefined further down the page.
    ///
    /// Only [`Emitter::plot`] asks. Every other line is emitted live whether or
    /// not its inputs arrived, because the engine then reports the missing
    /// value on the line that wanted it, which is where a reviewer needs to see
    /// it. A plot is the exception: it is worth emitting only if it can be
    /// drawn, and a chart that cannot is worse than a marker saying why.
    ///
    /// Filled as the emission runs, which is enough because region order runs
    /// down the page in every file of both corpora.
    valued: BTreeSet<String>,
    /// The parameters of the definition being emitted, in their SMath
    /// spelling, or empty outside one. See [`Emitter::captures`].
    parameters: BTreeSet<String>,
    /// Rows and columns of every name this worksheet binds to a matrix whose
    /// shape the file states, in their SMath spelling. See
    /// [`Emitter::columns_of`], which is the only thing that reads it.
    shapes: BTreeMap<String, (usize, usize)>,
    /// While set, every line written is commented out.
    ///
    /// A statement can be perfectly readable and still not be something Nomo
    /// can evaluate. Nothing is dropped, so the marker says why the line is not
    /// live and the translated line stays underneath it for whoever reviews the
    /// migration.
    commenting: bool,
    /// Images seen so far, written out together by [`Emitter::write_trailer`].
    resources: Vec<Resource>,
}

impl Emitter {
    fn new(w: &Worksheet) -> Emitter {
        let curves = curves_of_x(w);
        Emitter {
            out: String::new(),
            line: 0,
            pending: Vec::new(),
            lifted: 0,
            language: None,
            dropped_languages: 0,
            notes: Vec::new(),
            assertions: Vec::new(),
            names: Names::build(w),
            bound: Bound::build(w),
            functions: defined_functions(w),
            curves,
            curves_emitted: BTreeSet::new(),
            series: BTreeMap::new(),
            parameters: BTreeSet::new(),
            shapes: BTreeMap::new(),
            valued: BTreeSet::new(),
            commenting: false,
            resources: Vec::new(),
        }
    }

    fn run(mut self, w: &Worksheet) -> Emitted {
        self.push("' nomo 1");
        if let Some(title) = &w.settings.title {
            self.push(&format!("' {title}"));
        }
        if let Some(author) = &w.settings.author {
            self.push(&format!("' {author}"));
        }
        // Radians is Nomo's only mode, so a document set to degrees would have
        // every trig result silently multiplied by π/180. Never let that pass
        // without saying so.
        if let Some(angle) = &w.settings.angle {
            if angle != "radians" {
                self.note(
                    NoteKind::Unsupported,
                    format!(
                        "document angle mode is `{angle}`; Nomo evaluates trigonometry in radians"
                    ),
                );
                self.push(&format!(
                    "' [import] angle mode `{angle}` is not supported; trigonometry below is in radians"
                ));
            }
        }
        self.push("");

        for note in self.names.collisions.clone() {
            self.note(NoteKind::Collision, note);
        }
        // A rename the reader must know about: the worksheet's own `m` is not
        // the `m` in the imported source, and every line that used it moved with
        // it. Reported rather than done quietly, because the alternative reading
        // — that the author meant the unit — is the one Nomo would have taken.
        for (original, moved) in self.names.shadowed.clone() {
            self.note(
                NoteKind::Renamed,
                format!(
                    "`{original}` is a variable here and a unit this worksheet also uses; \
                     the variable is spelled `{moved}` so the unit keeps its meaning"
                ),
            );
        }

        for region in w.flat() {
            match &region.payload {
                Payload::Text { variants } => {
                    let text = self.choose_language(variants);
                    for line in text.lines() {
                        let line = line.trim_end();
                        if line.is_empty() {
                            self.push("'");
                        } else {
                            self.push(&format!("' {line}"));
                        }
                    }
                }
                Payload::Math(m) => self.math(m),
                Payload::Plot { expr, tag, view } => {
                    self.plot(expr, tag, *view, (region.width, region.height))
                }
                Payload::Picture { format, data, size } => self.picture(format, data, *size),
                Payload::Area { title } => {
                    if !title.is_empty() {
                        self.push(&format!("' {title}"));
                    }
                }
                Payload::Unsupported { tag } => {
                    self.unsupported(&format!("a `{tag}` region"));
                }
            }
        }

        // The page was two-dimensional and this file is not. Said once, so a
        // reader does not assume every comment introduces the line beneath it.
        let side_by_side = w.side_by_side_rows();
        if side_by_side > 0 {
            self.note(
                NoteKind::ScopeFlattened,
                format!(
                    "{side_by_side} value(s) had prose beside them on the page; \
                     flattened to lines, that prose reads after the value"
                ),
            );
        }

        // Said once, at the end, rather than at each of the 900-odd regions it
        // applies to: a reviewer needs to know that other languages existed and
        // which one was kept, not to be told so on every line.
        if self.dropped_languages > 0 {
            let kept = self.language.clone().unwrap_or_else(|| "the first".into());
            self.note(
                NoteKind::ScopeFlattened,
                format!(
                    "{} translation(s) of prose dropped; kept {kept} in each region",
                    self.dropped_languages
                ),
            );
        }

        // The page header, if the file had one. Before the trailer, so the
        // resources stay last and the figures keep reading order.
        self.write_furniture(w);
        // Last, so the body of the worksheet reads without 800 KB of base64
        // running through the middle of it.
        self.write_trailer();

        Emitted {
            source: self.out,
            notes: self.notes,
            assertions: self.assertions,
        }
    }

    fn math(&mut self, m: &Math) {
        // SMath's own note on the region, above the line it annotates. Not
        // decoration: `description(x)` *reads* this text, so a worksheet that
        // labels a plot axis keeps the label here and nowhere else.
        if !m.description.is_empty() {
            let note = self.choose_language(&m.description);
            for line in note.lines().filter(|l| !l.trim().is_empty()) {
                self.push(&format!("' {}", line.trim_end()));
            }
        }
        // Plot configuration is not mathematics at all — see [`Emitter::body`] —
        // and its property names are free by construction. Answered before the
        // free-symbol check so that 88 corpus regions keep the marker that names
        // what they actually are.
        if let Statement::Bare(e) = &m.statement {
            if plot_configuration(e).is_some() {
                return self.body(m);
            }
        }
        // A name nothing in the worksheet defines. Nomo has no free symbols, so
        // the line cannot be emitted as source, but it is still the worksheet's
        // content: the marker names the symbols and the translated line follows
        // it as a comment. See [`Bound`] for why SMath accepted it.
        //
        // An `Equation` is exempt because it is already emitted as a comment —
        // it binds nothing and asks the engine for nothing, so a symbol in it is
        // documentation behaving exactly as documentation should.
        // A definition whose only free symbol is `x` and whose name a plot draws
        // is not a region missing a value: it is a function of `x` written the
        // only way SMath offers. See [`curves_of_x`].
        if !matches!(m.statement, Statement::Equation { .. })
            && self.curve_of_x(&m.statement).is_none()
        {
            let free = self.bound.free_in(&m.statement);
            if !free.is_empty() {
                let named = free
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let plural = if free.len() == 1 { "is" } else { "are" };
                let why = match m.optimize {
                    // The region's own setting says the author meant it: SMath's
                    // CAS kept the names as symbols rather than failing, which is
                    // why the file saved with no `error` attribute to find.
                    Some(2) => ", and SMath kept the region symbolic",
                    _ => "",
                };
                // A bare `i` used to be reported here as SMath's imaginary
                // unit, which Nomo then had no way to express. It is a Nomo
                // constant now, so `free_in` never returns it and the note has
                // nothing left to say: a region using `i` imports as ordinary
                // complex arithmetic.
                self.unsupported(&format!(
                    "{named} {plural} used here but defined nowhere in this worksheet{why}"
                ));
                // Everything below writes comments, including whatever the line
                // lifts out of itself, so nothing half-emitted is left standing.
                self.commenting = true;
                self.body(m);
                self.commenting = false;
                self.record_values(m, false);
                return;
            }
        }
        let before = self.notes.len();
        self.body(m);
        let arrived = !self.notes[before..]
            .iter()
            .any(|n| n.kind == NoteKind::Unsupported);
        self.record_values(m, arrived);
    }

    /// What this expression reads that has no value in the emitted worksheet.
    ///
    /// Variables *and* the worksheet's own functions: `polygon : U_beam(…)`
    /// imports as a live line and still has no value when `U_beam`'s own
    /// definition did not import. A builtin call is not asked about — Nomo
    /// brings its own — and neither is a unit or a constant, which are other
    /// `Expr` variants or answered by the engine.
    ///
    /// `skip` holds the names that are not reads at all: a definition's own
    /// parameters, and whatever the region is itself binding.
    fn unvalued(&self, e: &Expr, skip: &BTreeSet<&str>) -> Vec<String> {
        let mut missing: Vec<String> = Vec::new();
        let mut want = |n: &String| {
            if !skip.contains(n.as_str())
                && !self.valued.contains(n)
                && !nomo_core::eval::is_constant(n)
                && !missing.contains(n)
            {
                missing.push(n.clone());
            }
        };
        e.walk(&mut |e| match e {
            Expr::Name(n) => want(n),
            // A call to a name Nomo provides resolves whatever the worksheet
            // did with it: `log10` is refused above when a worksheet redefines
            // it in terms of itself, and the calls then mean the built-in — the
            // whole point of refusing it.
            Expr::Call { name, .. }
                if self.functions.contains(name) && nomo_function(name).is_none() =>
            {
                want(name)
            }
            _ => {}
        });
        missing.sort();
        missing
    }

    /// The name this statement defines as a function of `x`, if it is one.
    ///
    /// [`curves_of_x`] does the document-wide half of the decision — which
    /// names a plot draws, and which of those nothing else reads. The half left
    /// for here is the one only emission knows: whether `x` is the *only* thing
    /// the body is waiting for.
    ///
    /// - `x` must have no value at this point in the page. A worksheet that
    ///   writes `x : 5` above the definition means `f(5)`, a number, and
    ///   turning that into a function would invent a curve.
    /// - Everything else the body reads must already have one. `plot_Z :
    ///   Z_abs(x)` is a function of `x` when `Z_abs` imported and is a broken
    ///   live line when it did not, and the second belongs under the marker it
    ///   already gets.
    fn curve_of_x<'a>(&self, s: &'a Statement) -> Option<&'a str> {
        let Statement::Define {
            target: Expr::Name(name),
            value,
            ..
        } = s
        else {
            return None;
        };
        if !self.curves.contains(name) || self.valued.contains("x") {
            return None;
        }
        let mut skip = BTreeSet::new();
        skip.insert("x");
        self.unvalued(value, &skip)
            .is_empty()
            .then_some(name.as_str())
    }

    /// Update [`Emitter::valued`] with what this region did or did not give a
    /// value to.
    ///
    /// `arrived` is whether the region imported at all. Even when it did, its
    /// target only counts if everything the region *reads* already has a value:
    /// a definition whose input never arrived is a line that evaluates to an
    /// error, not to a table.
    fn record_values(&mut self, m: &Math, arrived: bool) {
        let bound = values_bound_by(&m.statement);
        if bound.is_empty() {
            return;
        }
        // A definition's own parameters are not read from the document — `f(x)
        // : x^2` reads nothing — so they do not have to have a value.
        let mut local: BTreeSet<&str> = BTreeSet::new();
        // `bound` is written, not read, and a region may read a name it is also
        // rebinding — `x : x + 1` at the root is SMath's own idiom for that.
        if let Statement::Define {
            target: Expr::Call { name, args },
            ..
        } = &m.statement
        {
            if name != "el" && name != "mat" {
                for a in args {
                    if let Expr::Name(p) = a {
                        local.insert(p);
                    }
                }
            }
        }
        // A curve's parameter is written nowhere in its target — SMath has no
        // place to write it — but it is a parameter all the same.
        if self.curve_of_x(&m.statement).is_some() {
            local.insert("x");
        }
        let mut skip = local;
        skip.extend(bound.iter().map(String::as_str));
        let inputs_arrived = arrived
            && match &m.statement {
                // Only the value side is read; the target is being written.
                Statement::Define { value, .. } => self.unvalued(value, &skip).is_empty(),
                Statement::Show { expr, .. } | Statement::Bare(expr) => {
                    self.unvalued(expr, &skip).is_empty()
                }
                Statement::Equation { .. } => true,
            };
        for name in bound {
            if inputs_arrived {
                self.valued.insert(name);
            } else {
                // A name that used to hold a value and was redefined by a region
                // that did not import no longer holds one.
                self.valued.remove(&name);
            }
        }
    }

    fn body(&mut self, m: &Math) {
        if let Statement::Bare(e) = &m.statement {
            if let Some(plot) = plot_configuration(e) {
                // A `line(…)` block assigning nothing but plot properties —
                // `XYPlot'Labels'XLabel : description(text.phi)`. It is not
                // mathematics at all but the side-channel that configures a plot
                // region (design note §8.7 item 31), and reporting it as an
                // unknown call to `description` would put 88 uses in the ranking
                // under a name that explains nothing.
                return self.unsupported(&format!("plot configuration for `{plot}`"));
            }
        }
        // A `for` loop standing on its own. It is not an expression Nomo can
        // write — there is nothing to display — but the common shape of it is a
        // vector built element by element, which `map` says exactly. See
        // [`Emitter::fill`].
        if let Statement::Bare(e) = &m.statement {
            if let Expr::Call { name, .. } = e {
                if name == "for" || name == "while" {
                    return self.loop_region(e);
                }
            }
        }
        match &m.statement {
            // A document declaring its own unit — `VA : W` aliasing the watt,
            // `a.0 : 1 m` naming a length scale. Day-one scope rather than an
            // extension (design note §8.7 item 13), and the target is an
            // `Expr::Unit` because `resolve` has already decided that this
            // styled symbol really is one.
            Statement::Define {
                kind: Assign::Positional,
                target: Expr::Unit(name),
                value,
            } => {
                let Some(n) = self.names.get(name) else {
                    return self.unsupported("a unit declaration with an unrepresentable name");
                };
                match self.expr(value) {
                    Ok(v) => self.push(&format!("unit {n} = {v}")),
                    Err(why) => self.unsupported(&format!("the unit declaration `{name}`: {why}")),
                }
            }

            Statement::Define {
                kind,
                target: Expr::Name(name),
                value,
            } if self.curve_of_x(&m.statement).is_some() => self.curve(name, value, *kind),

            Statement::Define {
                kind,
                target: Expr::Name(name),
                value,
            } => {
                let Some(n) = self.names.variable(name) else {
                    return self.unsupported(&format!("a binding of `{name}`"));
                };
                match self.expr(value) {
                    Ok(v) => {
                        let keyword = if *kind == Assign::Global {
                            "global "
                        } else {
                            ""
                        };
                        self.push(&format!("{keyword}{n} = {v}"));
                        // A name can be redefined further down the page, so the
                        // shape is replaced or forgotten rather than added to.
                        match self.shape_of(value) {
                            Some(shape) => self.shapes.insert(name.clone(), shape),
                            None => self.shapes.remove(name),
                        };
                        // Only `action="numeric"` is an answer, for the reason
                        // spelled out at `Statement::Bare` below.
                        let stored = m
                            .result
                            .as_ref()
                            .filter(|_| m.result_kind == Some(ResultKind::Numeric));
                        self.record_answer(stored, m.contract.as_ref());
                    }
                    Err(why) => self.unsupported(&format!("the definition of `{name}`: {why}")),
                }
            }

            Statement::Define {
                kind,
                target: Expr::Call { name, args },
                value,
            } if name == "mat" && args.len() >= 3 => self.destructure(args, value),

            // `el(A, i, j) : x` writes into an existing matrix. Nomo has no
            // such statement by design — a worksheet is a set of definitions,
            // not a script — so there is nothing to emit and the marker says
            // precisely that rather than complaining about `el`'s parameters.
            Statement::Define {
                target: Expr::Call { name, args },
                ..
            } if name == "el" => {
                let what = match args.first() {
                    Some(Expr::Name(base)) => {
                        format!("an assignment into `{base}`, which mutates an existing value")
                    }
                    _ => "an assignment into an indexed value, which mutates it".into(),
                };
                self.unsupported(&what);
            }

            Statement::Define {
                kind,
                target: Expr::Call { name, args },
                value,
            } => {
                let Some(n) = self.names.get(name) else {
                    return self.unsupported(&format!("a definition of `{name}`"));
                };
                let params: Option<Vec<String>> = args
                    .iter()
                    .map(|a| match a {
                        Expr::Name(p) => self.names.variable(p),
                        _ => None,
                    })
                    .collect();
                let Some(params) = params else {
                    return self.unsupported(&format!(
                        "a definition of `{name}` with a computed parameter"
                    ));
                };
                // A worksheet that redefines a function Nomo already has, and
                // calls that name inside the new body. `linfit_multiple.sm`
                // writes the regression model as `ln(Nu) : b₁ + b₂·ln(Re) +
                // b₃·ln(Pr)`, where the calls on the right are plainly the
                // logarithm — but here the definition shadows the builtin for
                // its own body too, so the same text would mean a function that
                // calls itself. Recursion is not the objection: it works, and
                // `gcd(a, b) : … gcd(b, mod(a, b))` translates. The objection is
                // that Nomo has no way to write "the builtin, not me", so the
                // only faithful translations are this marker — which leaves
                // every *other* line's `ln` meaning the logarithm, which is what
                // the worksheet meant.
                if nomo_function(name).is_some() && calls(value, name) {
                    return self.unsupported(&format!(
                        "a definition of `{name}` whose body calls `{name}`: in SMath that \
                         inner call is the built-in, and here it would be the definition itself"
                    ));
                }
                // The body is emitted with these parameters on record, so that
                // anything lifted out of it can refuse to capture one. See
                // [`Emitter::captures`].
                self.parameters = args
                    .iter()
                    .filter_map(|a| match a {
                        Expr::Name(p) | Expr::Unit(p) => Some(p.clone()),
                        _ => None,
                    })
                    .collect();
                let emitted = self.expr(value);
                self.parameters.clear();
                match emitted {
                    Ok(v) => {
                        self.push(&format!("fn {n}({}) = {v}", params.join(", ")));
                        if *kind == Assign::Global {
                            self.note(
                                NoteKind::ScopeFlattened,
                                format!(
                                    "`{name}` was defined globally in SMath; Nomo has no `global fn`, \
                                     so it is only visible below this line"
                                ),
                            );
                        }
                    }
                    Err(why) => self.unsupported(&format!("the definition of `{name}`: {why}")),
                }
            }

            Statement::Define { target, .. } => {
                self.unsupported(&format!("a binding whose target is {}", shape(target)))
            }

            // A display. Both eras end up here; they differ only in where the
            // answer was kept.
            // An equation the author wrote out for a reader. Nomo has no
            // statement that asserts a relation without binding anything, so it
            // becomes a comment carrying the equation verbatim: the worksheet
            // keeps saying what it said, and nothing pretends to compute.
            Statement::Equation { left, right } => match (self.expr(left), self.expr(right)) {
                (Ok(l), Ok(r)) => self.push(&format!("' {l} = {r}")),
                _ => self.unsupported("a stated equation that cannot be written out"),
            },

            Statement::Show { expr, stored } => self.query(expr, stored.as_ref(), None, m),
            Statement::Bare(expr) => {
                // Only `action="numeric"` is an answer. `symbolic` records what
                // SMath derived and `none` holds an unevaluated equation; both
                // are provenance for a human reading the migration, and feeding
                // either to the checker would assert something no numeric engine
                // can reproduce.
                let stored = m
                    .result
                    .as_ref()
                    .filter(|_| m.result_kind == Some(ResultKind::Numeric));
                self.query(expr, stored, m.contract.as_ref(), m)
            }
        }
    }

    fn query(&mut self, expr: &Expr, stored: Option<&Expr>, contract: Option<&Expr>, _m: &Math) {
        let text = match self.expr(expr) {
            Ok(t) => t,
            Err(why) => return self.unsupported(&format!("a displayed expression: {why}")),
        };
        // A `<contract>` is the unit the answer is shown in, and `->` is how
        // Nomo asks for exactly that.
        let line = match contract.map(|c| self.expr(c)) {
            Some(Ok(unit)) => format!("{text} -> {unit}"),
            Some(Err(_)) | None => text,
        };
        self.push(&line);
        self.record_answer(stored, contract);
    }

    /// Keep the answer SMath stored for the line just written, as something the
    /// oracle can check Nomo's own against.
    ///
    /// Called for a definition as well as for a display. The two eras differ in
    /// where they keep the number, not in what it means: `Cr : … ` with a
    /// `<result>` of `4.5655*10^-8 F` asserts the value of `Cr` exactly as a
    /// bare `Cr =` further down the page would. Checking only the displays threw
    /// away most of what a modern worksheet knows about itself — 28 of the 34
    /// answers in the converter worksheet sit on definitions, and the six that
    /// did not left almost every computed value in the file unguarded.
    fn record_answer(&mut self, stored: Option<&Expr>, contract: Option<&Expr>) {
        // A commented-out line computes nothing, so there is nothing for its
        // stored answer to be checked against. Asserting on it would report the
        // comment as a disagreement and blame the engine for a gap the marker
        // above it has already named.
        let Some(stored) = stored.filter(|_| !self.commenting) else {
            return;
        };
        // With a `<contract>`, the unit lives there and the stored answer is
        // a bare number; without one it carries its own. Multiplying covers
        // both without a special case.
        let full = match contract {
            Some(c) => Expr::Op {
                glyph: "*".into(),
                args: vec![stored.clone(), c.clone()],
            },
            None => stored.clone(),
        };
        if let (Ok(expected), Some(mantissa)) = (self.expr(&full), first_number(&full)) {
            self.assertions.push(Assertion {
                line: self.line,
                expected,
                mantissa,
                elements: element_numbers(&full),
            });
        }
    }

    /// Translate an expression, or say why it cannot be.
    fn expr(&mut self, e: &Expr) -> Result<String, String> {
        self.at(e, CONDITIONAL)
    }

    /// `parent` is the precedence the result must bind at least as tightly as.
    fn at(&mut self, e: &Expr, parent: u8) -> Result<String, String> {
        let (text, prec) = self.build(e)?;
        Ok(if prec < parent {
            format!("({text})")
        } else {
            text
        })
    }

    fn build(&mut self, e: &Expr) -> Result<(String, u8), String> {
        match e {
            Expr::Number(n) => Ok((n.clone(), ATOM)),
            Expr::Unit(u) => match self.names.get(u) {
                Some(n) => Ok((n, ATOM)),
                None => Err(format!("`{u}` is not a name Nomo can spell")),
            },
            Expr::Name(n) => {
                if n == "∞" {
                    return Ok(("inf".into(), ATOM));
                }
                match self.names.variable(n) {
                    Some(n) => Ok((n, ATOM)),
                    None => Err(format!("`{n}` is not a name Nomo can spell")),
                }
            }
            // A string literal. Nomo's carry no escapes, so one containing a
            // quote has no spelling and says so rather than being written out
            // with the quote swallowing the rest of the line.
            Expr::Text(t) => {
                if t.contains('"') {
                    return Err(format!(
                        "a string containing a quote (`{t}`), which Nomo has no escape for"
                    ));
                }
                Ok((format!("\"{t}\""), ATOM))
            }
            Expr::Unsupported { detail, .. } => Err(detail.clone()),
            Expr::Op { glyph, args } => self.op(glyph, args),
            Expr::Call { name, args } => self.call(name, args),
        }
    }

    fn op(&mut self, glyph: &str, args: &[Expr]) -> Result<(String, u8), String> {
        match (glyph, args.len()) {
            ("-", 1) => Ok((format!("-{}", self.at(&args[0], POWER)?), UNARY)),
            ("+", 1) => self.build(&args[0]),
            ("^", 2) => {
                // Right associative: the left operand must bind tighter.
                let l = self.at(&args[0], ATOM)?;
                let r = self.at(&args[1], POWER)?;
                Ok((format!("{l}^{r}"), POWER))
            }
            ("*", 2) => {
                // Two columns of the same length are an inner product in SMath
                // and an element-wise product in Nomo, and that is the whole
                // of the disagreement between the two `·`s. See
                // [`Emitter::dot_product`], which read the rule out of
                // `SMath.Math.Numeric.dll` rather than inferring it.
                if self.dot_product(&args[0], &args[1]) {
                    let l = self.at(&args[0], 0)?;
                    let r = self.at(&args[1], 0)?;
                    return Ok((format!("dot({l}, {r})"), ATOM));
                }
                let l = self.at(&args[0], PRODUCT)?;
                // A unit attaches by multiplication in both languages, and
                // juxtaposition is how Nomo writes it: `230 V`, not `230*V`.
                if matches!(args[1], Expr::Unit(_)) {
                    let r = self.at(&args[1], ATOM)?;
                    return Ok((format!("{l} {r}"), PRODUCT));
                }
                let r = self.at(&args[1], PRODUCT + 1)?;
                Ok((format!("{l}*{r}"), PRODUCT))
            }
            ("/", 2) => {
                let l = self.at(&args[0], PRODUCT)?;
                let r = self.at(&args[1], PRODUCT + 1)?;
                Ok((format!("{l}/{r}"), PRODUCT))
            }
            ("+", 2) => {
                let l = self.at(&args[0], SUM)?;
                let r = self.at(&args[1], SUM + 1)?;
                Ok((format!("{l} + {r}"), SUM))
            }
            ("<", 2) | (">", 2) | ("≤", 2) | ("≥", 2) | ("≠", 2) => {
                let l = self.at(&args[0], COMPARE)?;
                let r = self.at(&args[1], COMPARE + 1)?;
                Ok((format!("{l} {glyph} {r}"), COMPARE))
            }
            // `≡` reaches here only when it is nested inside an expression,
            // where it is an equality test rather than a global definition —
            // 81 of the corpus's 304 uses. `classify` has already taken the
            // ones at the root of a region.
            ("≡", 2) => {
                let l = self.at(&args[0], COMPARE)?;
                let r = self.at(&args[1], COMPARE + 1)?;
                Ok((format!("{l} == {r}"), COMPARE))
            }
            ("&", 2) => {
                let l = self.at(&args[0], AND)?;
                let r = self.at(&args[1], AND + 1)?;
                Ok((format!("{l} and {r}"), AND))
            }
            ("¬", 1) => Ok((format!("not {}", self.at(&args[0], COMPARE)?), NOT)),
            // The cross product, written as an infix dagger in SMath and as a
            // call in Nomo. 48 uses across 10 worksheets, always a moment
            // `r † F` or a normal `e.z † e.t(t)`, and never at a region root —
            // it is an ordinary binary operator inside a sum, not a binder.
            ("†", 2) => {
                let l = self.build(&args[0])?.0;
                let r = self.build(&args[1])?.0;
                Ok((format!("cross({l}, {r})"), ATOM))
            }
            ("-", 2) => {
                let l = self.at(&args[0], SUM)?;
                let r = self.at(&args[1], SUM + 1)?;
                Ok((format!("{l} - {r}"), SUM))
            }
            _ => Err(format!("the operator `{glyph}` (arity {})", args.len())),
        }
    }

    /// The Nomo spelling of a function this worksheet defines, if it does.
    ///
    /// A call is emitted whether or not that definition survived translation. If
    /// it did not, the line above it carries the marker saying why, and this
    /// line fails to evaluate with `… is not defined` — which is exactly what
    /// already happens to a *variable* whose definition could not be emitted.
    /// Refusing the call instead would report the same gap twice and hide which
    /// of the two was the cause.
    /// The function this expression already *is*, when it is nothing but a
    /// call to one of the worksheet's own functions at `var`.
    ///
    /// `int`, `solve` and `diff` all take an expression where Nomo takes the
    /// name of a function, so all three lift the expression into one. When the
    /// expression is already `Mg(f)` there is nothing to lift: `Mg` is that
    /// function, and inventing `integral_1` for it renames the worksheet's own
    /// vocabulary for no gain — and puts an invented name on the line that
    /// reports an error, which is where a reader looks first.
    fn already_a_function(&self, e: &Expr, var: &str) -> Option<String> {
        let Expr::Call { name, args } = e else {
            return None;
        };
        if !self.functions.contains(name) || args.len() != 1 {
            return None;
        }
        match &args[0] {
            Expr::Name(v) if v == var => self.names.get(name),
            _ => None,
        }
    }

    fn defined_function(&self, name: &str) -> Option<String> {
        self.functions
            .contains(name)
            .then(|| self.names.get(name))?
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<(String, u8), String> {
        match name {
            // `el(v, i)` and `el(m, i, j)` are indexing, and indexing is syntax
            // in Nomo. This is the single most common function in the corpus.
            "el" if args.len() == 2 || args.len() == 3 => {
                let base = self.at(&args[0], ATOM)?;
                let mut idx = Vec::new();
                for a in &args[1..] {
                    idx.push(self.at(a, 0)?);
                }
                Ok((format!("{base}[{}]", idx.join(", ")), ATOM))
            }
            // `mat(e11, e12, …, rows, cols)` is a literal, laid out row by row.
            // Row-major is not assumed: four corpus matrices name their elements
            // (`x1…x5`, `y1…y5`, `z1…z5` with rows=3, cols=5) and settle it.
            "mat" if args.len() >= 3 => {
                let (elems, shape) = args.split_at(args.len() - 2);
                let (Expr::Number(rows), Expr::Number(cols)) = (&shape[0], &shape[1]) else {
                    return Err("a matrix whose shape is computed".into());
                };
                let (rows, cols) = match (rows.parse::<usize>(), cols.parse::<usize>()) {
                    (Ok(r), Ok(c)) => (r, c),
                    _ => return Err("a matrix with a non-integer shape".into()),
                };
                if rows * cols != elems.len() {
                    return Err(format!(
                        "a {rows}×{cols} matrix given {} elements",
                        elems.len()
                    ));
                }
                let mut cells = Vec::with_capacity(elems.len());
                for e in elems {
                    cells.push(self.at(e, 0)?);
                }
                if cols == 1 || rows == 1 {
                    // A single row or column is a vector in Nomo, not a matrix
                    // of one row: `[1, 2, 3]`, which is what indexing expects.
                    return Ok((format!("[{}]", cells.join(", ")), ATOM));
                }
                let rows_text: Vec<String> = cells
                    .chunks(cols)
                    .map(|r| format!("[{}]", r.join(", ")))
                    .collect();
                Ok((format!("[{}]", rows_text.join(", ")), ATOM))
            }
            // `int(expr, x, a, b)`: an integrand with a free variable, plus the
            // variable's name and the limits.
            //
            // Nomo has no lambdas — `map` and `iterate` take the *name* of a
            // function, deliberately — so the integrand is lifted out into a
            // named definition and the call refers to it. That is a real
            // translation rather than a rename, and it is why this is worth
            // doing here instead of adding an expression-taking form to the
            // language.
            "int" if args.len() == 4 => {
                let Expr::Name(var) = &args[1] else {
                    return Err(String::from(
                        "`int` over something that is not a plain variable name",
                    ));
                };
                let Some(param) = self.names.variable(var) else {
                    return Err(format!("`{var}` is not a name Nomo can spell"));
                };
                if let Some(captured) = self.captures(&args[0], var) {
                    return Err(capture_refused("int", &captured));
                }
                let body = self.at(&args[0], 0)?;
                let from = self.at(&args[2], CONDITIONAL)?;
                let to = self.at(&args[3], CONDITIONAL)?;
                let named = self
                    .already_a_function(&args[0], var)
                    .unwrap_or_else(|| self.lift("integral", &param, &body));
                Ok((format!("integral({named}, {from}, {to})"), ATOM))
            }
            // `sum(expr, i, a, b)`: a summand free in an index, the index's
            // name, and its first and last values. 77 calls across 14
            // worksheets, against three of the one-argument form.
            //
            // This is `int`'s shape one line up and it translates the same
            // way — the summand is lifted into a named definition, because
            // `map` takes the *name* of a function and there are no lambdas.
            // What differs is that Nomo needs no new builtin for it: `map`
            // over a `range` is the index, and the one-argument `sum` is the
            // fold. SMath's index steps by one, which is exactly what a
            // two-argument `range` does — the three-argument form, whose
            // meaning here is unverified, is not needed and is not used.
            //
            // Until this existed the call was passed through to the built-in
            // registry, which spelled it `sum(expr, i, a, b)` — four arguments
            // to a function that takes one, with `i` reading as the imaginary
            // unit. That failed at evaluation rather than at import, so it
            // left no marker: the worksheet imported clean and then would not
            // run.
            "sum" if args.len() == 4 => {
                let Expr::Name(var) = &args[1] else {
                    return Err(String::from(
                        "`sum` over something that is not a plain variable name",
                    ));
                };
                let Some(param) = self.names.variable(var) else {
                    return Err(format!("`{var}` is not a name Nomo can spell"));
                };
                if let Some(captured) = self.captures(&args[0], var) {
                    return Err(capture_refused("sum", &captured));
                }
                let body = self.at(&args[0], 0)?;
                let from = self.at(&args[2], CONDITIONAL)?;
                let to = self.at(&args[3], CONDITIONAL)?;
                let named = self
                    .already_a_function(&args[0], var)
                    .unwrap_or_else(|| self.lift("term", &param, &body));
                Ok((format!("sum(map({named}, range({from}, {to})))"), ATOM))
            }
            // SMath's `roots(expr, x, guess)` is a *local* search from a
            // starting point, and Nomo's `roots(f, a, b)` scans a window.
            // Same spelling, different function, and the same three operands:
            // read as Nomo's it would take the guess for one end of a range
            // and the variable for a function name. `5.1.sm` is the proof that
            // the difference is not academic — it writes `roots(Q(x·m), x, -1)`
            // for `1.08` and `roots(Q(x·m), x, -1.1)` for `-3.08`, two guesses a
            // tenth apart landing on different roots, which is what a local
            // method does and a scan never does.
            //
            // Refused by name rather than left to the builtin lookup, so that a
            // Nomo builtin gaining a name cannot quietly change what an
            // imported worksheet means. That is exactly what happened when
            // `roots` was added to the language: 8 corpus regions started
            // translating, silently and wrongly, with nothing failing.
            // The guard is SMath's own resolution order, which the fallback
            // below already follows: a worksheet that defines the name means its
            // own function. `Finite differences.sm` writes `diff(y, x) ≡ 0`,
            // which is a stated equation and registers `diff` as a definition,
            // and every `diff` in its documentation equations renders because of
            // it. An unguarded refusal here took that away.
            //
            // Two arities, two refusals, and the reasons are different.
            // `roots(system, unknowns)` solves a system — Nomo has
            // `solve_linear` for that — but the unknowns arrive as a *name*
            // holding a vector of free symbols, so nothing in the call says what
            // they are or what dimension they have (design note §8.36).
            "roots" if args.len() == 2 && !self.functions.contains("roots") => Err(String::from(
                "`roots` of a system, whose unknowns are free symbols the file does not \
                 give a dimension",
            )),
            // `roots(expr, x, guess)` searches from a starting point. `5.1.sm`
            // writes `roots(Q(x·m), x, -1)` for `1.08` and the same with `-1.1`
            // for `-3.08`: two guesses a tenth apart landing on different roots,
            // which is what a local method does and a scan never does.
            "roots" if !self.functions.contains("roots") => Err(String::from(
                "`roots`, which in SMath searches from a starting guess rather than \
                 across a range",
            )),
            // SMath's `FindRoot(equations, unknown ≡ guess)` solves from a
            // starting point, and **which** root it lands on is decided by the
            // method rather than by the worksheet: `5.1.sm` starts at `L` and
            // gets `1.08 m`, and at `L/2` — nearer the other root — gets
            // `-3.08 m`. Reproducing that choice would mean reproducing the
            // algorithm, and the Nonlinear Solvers plugin is not on this machine
            // to be read the way `solve` was (§8.24).
            //
            // Nomo's own methods are a bracket (`root`) and a window scan
            // (`roots`), and neither is given one here; `solve_linear` wants a
            // system whose unknowns the call names, and the multi-unknown
            // worksheets keep theirs inside another name. §8.36.
            "FindRoot" if !self.functions.contains("FindRoot") => Err(String::from(
                "`FindRoot`, which solves from a starting guess — and which root that \
                 finds is the method's choice, not the worksheet's",
            )),
            // `solve(expr, x, a, b)` — SMath's search across a range.
            //
            // This used to be refused, and the refusal was right at the time:
            // the corpus showed that four regions hand `solve` a range across
            // which the expression does not change sign and SMath still
            // answers, so its limits are a search range rather than a bracket,
            // and mapping the name onto `root` would have been mapping it onto
            // a different algorithm. What was missing was what the algorithm
            // *is*, and inference from worksheets could not supply it.
            //
            // Reading `SpecialFunctions.dll` did (design note §8.24): 200
            // samples across the range, every sign change between neighbours
            // refined, deduplicated, and returned as a scalar or a column. That
            // is `roots` in Nomo, so this is a translation of the construct
            // rather than a substitution of a different one — and the earlier
            // measurement now reads as evidence *for* it, since a 200-point
            // scan sees the interior crossings an endpoint bracket test misses.
            "solve" if args.len() == 4 && !self.functions.contains("solve") => {
                let Expr::Name(var) = &args[1] else {
                    return Err(String::from(
                        "`solve` over something that is not a plain variable name",
                    ));
                };
                let Some(param) = self.names.variable(var) else {
                    return Err(format!("`{var}` is not a name Nomo can spell"));
                };
                if let Some(captured) = self.captures(&args[0], var) {
                    return Err(capture_refused("solve", &captured));
                }
                // `solve(f ≡ g, …)` is `solve(f − g, …)`. Not an interpretation:
                // SMath rewrites the last term from the equality to a minus
                // before it searches, which is the same rewrite written here.
                let difference;
                let expr = match &args[0] {
                    Expr::Op { glyph, args: sides } if glyph == "≡" && sides.len() == 2 => {
                        difference = Expr::Op {
                            glyph: String::from("-"),
                            args: sides.clone(),
                        };
                        &difference
                    }
                    other => other,
                };
                let body = self.at(expr, 0)?;
                let from = self.at(&args[2], CONDITIONAL)?;
                let to = self.at(&args[3], CONDITIONAL)?;
                let named = self
                    .already_a_function(expr, var)
                    .unwrap_or_else(|| self.lift("zero", &param, &body));
                Ok((format!("roots({named}, {from}, {to})"), ATOM))
            }
            // `solve(expr, x)` searches between `SolveFromPoint` and
            // `SolveToPoint`, which are **program options** rather than
            // anything the worksheet records — the same two the error message
            // "Cannot solve. Check program options." points at. So the file does
            // not say what range SMath searched, and neither could an import:
            // the range decides which roots are found, and inventing one would
            // invent the answer. §8.24.
            "solve" if args.len() == 2 && !self.functions.contains("solve") => Err(String::from(
                "`solve` with no range, which in SMath searches between two program options \
                 the worksheet does not record",
            )),
            // `diff(expr, x)` — SMath's derivative, which is **symbolic**: the
            // evaluator sets its optimization to symbolic and hands the
            // expression to the CAS (§8.24). Nomo's `derivative` is the other
            // thing a worksheet means by a derivative — the value of the slope
            // at a point, carried through the arithmetic rather than derived as
            // an expression (§8.27).
            //
            // Where the worksheet only ever *evaluates* the result, those are
            // the same number and this is a translation: the converter
            // worksheet writes `derivative_Mg(f) : diff(Mg(f), f)` and then
            // samples it with a root search, which is exactly what `derivative`
            // answers. Where a
            // worksheet manipulates the expression instead — the mechanics
            // corpus writes `diff(x.S, t, 2)` and hands the result to `Solve` —
            // they are not the same thing, and that pipeline is refused a step
            // later for needing the CAS anyway.
            "diff" if (args.len() == 2 || args.len() == 3) && !self.functions.contains("diff") => {
                let Expr::Name(var) = &args[1] else {
                    return Err(String::from(
                        "`diff` with respect to something that is not a plain variable name",
                    ));
                };
                let Some(param) = self.names.variable(var) else {
                    return Err(format!("`{var}` is not a name Nomo can spell"));
                };
                if let Some(captured) = self.captures(&args[0], var) {
                    return Err(capture_refused("diff", &captured));
                }
                // The expression has to *read* the variable, and this is where
                // the symbolic and the numeric readings part company. `9.3.sm`
                // writes `diff(y.B, t)` where `y.B` is a name the worksheet has
                // already bound: SMath differentiates the formula that name
                // stands for, and Nomo would differentiate the number it
                // evaluated to — answering zero. A wrong answer that looks like
                // an answer is the one outcome this importer must not produce,
                // so a differentiand that never mentions the variable is the
                // CAS case and stays refused.
                let mut reads_it = false;
                args[0].walk(&mut |e| {
                    if let Expr::Name(n) = e {
                        reads_it |= n == var;
                    }
                });
                if !reads_it {
                    return Err(format!(
                        "`diff` of an expression that does not read `{var}`, which in SMath \
                         differentiates the formula a name stands for"
                    ));
                }
                // The order, when the worksheet writes one. It has to be a
                // literal: `derivative` takes 1 or 2 and the choice decides
                // which rule runs, so an order computed at run time is a
                // question this cannot answer here.
                let order = match args.get(2) {
                    None => 1u32,
                    Some(Expr::Number(n)) => match n.parse::<u32>() {
                        Ok(order @ (1 | 2)) => order,
                        Ok(order) => {
                            return Err(format!(
                                "`diff` of order {order}, and Nomo's derivative goes to the \
                                 second"
                            ))
                        }
                        Err(_) => return Err(String::from("`diff` of a fractional order")),
                    },
                    Some(_) => return Err(String::from("`diff` of a computed order")),
                };
                let body = self.at(&args[0], 0)?;
                let named = self
                    .already_a_function(&args[0], var)
                    .unwrap_or_else(|| self.lift("slope", &param, &body));
                Ok((
                    if order == 1 {
                        format!("derivative({named}, {param})")
                    } else {
                        format!("derivative({named}, {param}, {order})")
                    },
                    ATOM,
                ))
            }
            // SMath's overbar. It forces an expression to be evaluated element
            // by element rather than as matrix algebra — and in Nomo that is
            // already what `*`, `^` and every scalar function do over a vector,
            // so the wrapper has nothing left to say. Dropped rather than
            // refused, but never silently: the note records that a semantic
            // marker was removed, because "it happened to mean the same thing
            // here" is a claim a reviewer should be able to check.
            "vectorize" if args.len() == 1 => {
                self.note(
                    NoteKind::ScopeFlattened,
                    "`vectorize` dropped: Nomo's operators and functions are element-wise \
                     over vectors already"
                        .into(),
                );
                self.build(&args[0])
            }
            // SMath's two-argument `range(a, b)` is Nomo's. Its three-argument
            // form is *not* translated: in Mathcad's lineage the third operand
            // is the second element rather than a step, and which one SMath
            // means has not been verified. 28 corpus uses stay unsupported
            // rather than be silently multiplied or divided by two.
            "range" if args.len() == 2 => {
                let from = self.at(&args[0], CONDITIONAL)?;
                let to = self.at(&args[1], CONDITIONAL)?;
                Ok((format!("range({from}, {to})"), ATOM))
            }
            "range" => Err(String::from(
                "`range` with a step, whose meaning in SMath is unverified",
            )),
            // SMath spells a conditional as a three-argument function. Nomo
            // has the expression form, and evaluates only the arm it takes,
            // which SMath's does not — so an imported guard becomes a real one.
            "if" if args.len() == 3 => {
                let cond = self.at(&args[0], CONDITIONAL + 1)?;
                let then = self.at(&args[1], CONDITIONAL + 1)?;
                // The `else` arm reaches as far as it can, so a chained
                // conditional needs no brackets there.
                let otherwise = self.at(&args[2], CONDITIONAL)?;
                Ok((
                    format!("if {cond} then {then} else {otherwise}"),
                    CONDITIONAL,
                ))
            }
            _ => {
                // A worksheet's own function is looked for before the built-in
                // registry, because that is the order SMath resolves in. No
                // corpus worksheet defines a function whose name Nomo also has,
                // so nothing today depends on the order — it is written this way
                // so that the one that eventually does is not silently sent to
                // the wrong function.
                let nomo = self
                    .defined_function(name)
                    .or_else(|| nomo_function(name).map(str::to_string))
                    .ok_or_else(|| format!("the function `{name}`"))?;
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    out.push(self.at(a, CONDITIONAL)?);
                }
                Ok((format!("{nomo}({})", out.join(", ")), ATOM))
            }
        }
    }

    /// A definition that is really a function of `x`, written as one.
    ///
    /// Which definitions these are, and why the reading is the worksheet's own
    /// rather than a guess, is in [`curves_of_x`].
    fn curve(&mut self, name: &str, value: &Expr, kind: Assign) {
        // `Multipleplots : sys(Mg(x), Mg2(x), 2, 1)` is not one curve but a
        // list of them, and SMath's `sys` is only meaningful inside a plot.
        // Nomo has no value for a list of series and needs none: the plot that
        // reads the name is where the list means something, so it is recorded
        // for that plot rather than written out here. Counted, not silent — and
        // if the plot cannot draw them after all, its own marker names this
        // name and says why.
        if let Some(listed) = series_of(value) {
            let count = listed.len();
            self.series.insert(name.to_string(), value.clone());
            self.note(
                NoteKind::Carried,
                format!(
                    "`{name}` names {count} series, which in SMath is a plot rather than a \
                     value; the plot that reads it draws them"
                ),
            );
            return;
        }
        // `defined_function` spells a call with `get`, so the definition has to
        // agree with it: this name is a function now, not a variable.
        let (Some(n), Some(param)) = (self.names.get(name), self.names.variable("x")) else {
            return self.unsupported(&format!("a definition of `{name}` as a function of `x`"));
        };
        let v = match self.expr(value) {
            Ok(v) => v,
            Err(why) => return self.unsupported(&format!("the definition of `{name}`: {why}")),
        };
        self.push(&format!("fn {n}({param}) = {v}"));
        // A function from here on: `defined_function` looks in `functions`, and
        // so does the check for whether a call has a value yet.
        self.functions.insert(name.to_string());
        self.curves_emitted.insert(name.to_string());
        if kind == Assign::Global {
            self.note(
                NoteKind::ScopeFlattened,
                format!(
                    "`{name}` was defined globally in SMath; Nomo has no `global fn`, \
                     so it is only visible below this line"
                ),
            );
        }
        self.note(
            NoteKind::Carried,
            format!(
                "`{name}` is defined in terms of `x` and drawn by a plot, so it is imported as \
                 the function of `x` that it is rather than as a value"
            ),
        );
    }

    /// `mat(a, b, c, 3, 1) : v` — one expression binding a whole vector of
    /// names, which is how a solver result is unpacked. 31 uses across the two
    /// corpora.
    ///
    /// Nomo has no destructuring statement, but it does not need one: binding
    /// the vector once and taking each element by index says the same thing with
    /// nothing added, and stays a set of definitions rather than becoming a
    /// script.
    fn destructure(&mut self, args: &[Expr], value: &Expr) {
        let (targets, _shape) = args.split_at(args.len() - 2);
        let names: Option<Vec<String>> = targets
            .iter()
            .map(|t| match t {
                Expr::Name(n) | Expr::Unit(n) => self.names.variable(n),
                _ => None,
            })
            .collect();
        let Some(names) = names else {
            return self
                .unsupported("a binding that unpacks a vector into names Nomo cannot spell");
        };
        let v = match self.expr(value) {
            Ok(v) => v,
            // The names were fine; it is what is being unpacked that could not
            // be translated. Saying so points at the construct that is actually
            // missing instead of at the binding.
            Err(why) => {
                return self.unsupported(&format!("a binding that unpacks a vector: {why}"))
            }
        };
        // One temporary per unpacking, named after the first target so that the
        // line reads as what it is rather than as `tmp1`, `tmp2`.
        let tmp = format!("{}_all", names[0]);
        self.push(&format!("{tmp} = {v}"));
        for (i, n) in names.iter().enumerate() {
            self.push(&format!("{n} = {tmp}[{}]", i + 1));
        }
    }

    /// Which language's prose to keep, out of the ones a region carries.
    ///
    /// The design note is explicit that this is a **policy decision someone has
    /// to make**, not something an importer may settle by taking the first and
    /// saying nothing: 902 text regions in the mechanics corpus hold the same
    /// paragraph in German, English and Russian, and 221 math regions carry a
    /// description the same way.
    ///
    /// The policy is: the language the caller asked for, if the region has it;
    /// otherwise the region's first, in document order. Whatever is dropped is
    /// counted and reported once, so a reviewer can see that other languages
    /// existed and choose again.
    fn choose_language(&mut self, variants: &[(Option<String>, String)]) -> String {
        let chosen = self
            .language
            .as_deref()
            .and_then(|want| {
                variants
                    .iter()
                    .find(|(lang, _)| lang.as_deref() == Some(want))
            })
            .or_else(|| variants.first());
        if variants.len() > 1 {
            self.dropped_languages += variants.len() - 1;
        }
        chosen.map(|(_, text)| text.clone()).unwrap_or_default()
    }

    /// The rows and columns of an expression, where the file states them.
    ///
    /// Deliberately shallow: a matrix literal, a name bound to one, a
    /// `transpose` of either, and scaling by anything else — which is how a
    /// worksheet writes a column of millimetres, `transpose(mat(…, 1, 11))·mm`.
    /// Anything it cannot see through is `None`, and every caller treats `None`
    /// as "do not act".
    fn shape_of(&self, e: &Expr) -> Option<(usize, usize)> {
        match e {
            // A parameter shadows the global of the same name for the body it
            // is in, and the global is what this map holds.
            Expr::Name(n) | Expr::Unit(n) if !self.parameters.contains(n) => {
                self.shapes.get(n).copied()
            }
            Expr::Call { name, args } if name == "mat" && args.len() >= 3 => {
                let (_, shape) = args.split_at(args.len() - 2);
                match (&shape[0], &shape[1]) {
                    (Expr::Number(r), Expr::Number(c)) => Some((r.parse().ok()?, c.parse().ok()?)),
                    _ => None,
                }
            }
            Expr::Call { name, args } if name == "transpose" && args.len() == 1 => {
                let (r, c) = self.shape_of(&args[0])?;
                Some((c, r))
            }
            // Scaling keeps the shape. Only the side that has one contributes,
            // so `v·mm` and `mm·v` both answer, and `v·w` — where both sides
            // are matrices — is left to the caller below.
            Expr::Op { glyph, args } if (glyph == "*" || glyph == "/") && args.len() == 2 => {
                match (self.shape_of(&args[0]), self.shape_of(&args[1])) {
                    (Some(l), None) => Some(l),
                    (None, Some(r)) if glyph == "*" => Some(r),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The length these two operands share, if `·` between them is SMath's
    /// **dot product** rather than an element-wise multiplication.
    ///
    /// # Read out of `SMath.Math.Numeric.dll`, not inferred
    ///
    /// `TMatrix::op_Multiply(TMatrix, TMatrix)` tests three things before it
    /// does anything else: both operands have one column, and they have the
    /// same number of rows. When all three hold it returns
    /// `Σᵢ c1[i,0]·c2[i,0]` as a **scalar** — an inner product — and only
    /// otherwise falls through to `c1.cols == c2.rows`, the ordinary matrix
    /// product, or throws.
    ///
    /// This is the one place SMath's `·` and Nomo's disagree. Nomo's is
    /// element-wise between two vectors on purpose (`docs/language.md`), which
    /// is what a tabulated calculation wants, and it spells the inner product
    /// `dot(a, b)`. Every other combination already agrees: matrix by matrix,
    /// matrix by column and scalar broadcast all mean the same thing in both.
    ///
    /// `Calc Area Properties…sm` is what found it. It writes its total area as
    /// `A.total ← b·h` over two `mat(…, 9, 1)` columns and stores **64 in²**,
    /// which is `Σ bᵢ·hᵢ`; Nomo computed nine values, and thirteen of that
    /// worksheet's twenty-one answers descended from it.
    fn dot_product(&self, l: &Expr, r: &Expr) -> bool {
        match (self.shape_of(l), self.shape_of(r)) {
            (Some((rows, 1)), Some((same, 1))) => rows == same,
            _ => false,
        }
    }

    /// The enclosing definition's parameter that lifting this body would
    /// capture, if any.
    ///
    /// A lifted helper is written *above* the definition it came out of, so the
    /// names in it resolve to the worksheet's globals rather than to the
    /// parameters that were in scope where it was written. Nomo has no
    /// closures, so there is nothing else to emit and the region says so.
    ///
    /// `simpsonrichardson.sm` is where this stopped being theoretical. Its
    /// Simpson rule is `simp(a, b, h, n) : … sum(f(a + k·h), k, 1, n−1) …`, and
    /// the summand lifts to `fn term(k) = f(a + k*h)` reading the *global* `a`
    /// and `h`. That worksheet calls `simp` with exactly those globals, so the
    /// first answer comes out right — by coincidence, which is the worst way
    /// for an answer to be right and the reason this is a refusal rather than a
    /// warning.
    fn captures(&self, body: &Expr, bound: &str) -> Option<String> {
        let mut found: Option<String> = None;
        body.walk(&mut |e| {
            let (Expr::Name(n) | Expr::Unit(n)) = e else {
                return;
            };
            if found.is_none() && n != bound && self.parameters.contains(n) {
                found = Some(n.clone());
            }
        });
        found
    }

    /// Write `fn <name>(<param>) = <body>` above the line being built, and
    /// answer with the name.
    ///
    /// The definition has to be emitted *before* the statement that uses it,
    /// which is why the emitter buffers a line rather than writing it as it
    /// goes: by the time an integrand is met, the line it belongs to is still
    /// being assembled.
    fn lift(&mut self, kind: &str, param: &str, body: &str) -> String {
        self.lifted += 1;
        let name = format!("{kind}_{}", self.lifted);
        self.pending.push(format!("fn {name}({param}) = {body}"));
        name
    }

    fn push(&mut self, line: &str) {
        // Anything lifted while this line was being built belongs above it.
        for definition in core::mem::take(&mut self.pending) {
            self.write(&definition);
        }
        self.write(line);
    }

    fn write(&mut self, line: &str) {
        // A marker is already a comment and says something the reader needs at
        // the left margin; commenting it again would bury it.
        if self.commenting && !line.starts_with('\'') && !line.is_empty() {
            self.out.push_str("' ");
        }
        self.out.push_str(line);
        self.out.push('\n');
        self.line += 1;
    }

    /// An embedded image: a reference where it stood, the data in the trailer.
    ///
    /// The reference carries the size SMath drew the figure at, because the
    /// pixels do not: the interlock worksheet's figures are photographs and
    /// diagrams placed at roughly two-thirds of their stored width, and an
    /// import that wrote only the base64 would show every one of them half a
    /// page wide. A
    /// worksheet whose region declared no box gets a bare reference, and the
    /// figure renders at its own size — the same as before this existed.
    /// A `for` or `while` loop written as a region of its own.
    ///
    /// # A worksheet is a set of definitions, not a script
    ///
    /// Nomo has no loop statement and no assignment into an element, which is
    /// deliberate: nothing mutates, so the dependency graph is the document.
    /// What real worksheet loops were *doing*, though, is mostly not mutation —
    /// it is building a vector whose *i*th element is a function of *i*, one
    /// element at a time because SMath had no other way to say it. That is
    /// `map` over a `range`, and this translates it.
    ///
    /// Across both corpora, 105 `for` loops: **25 are exactly this fill and
    /// stand as a region of their own**, which is what this handles. The rest
    /// are recurrences (`el(β, i) ← … el(β, i-1) …`), accumulators, conditional
    /// appends, or a fill nested inside a function body, and each keeps a
    /// marker that now says which.
    ///
    /// # `while` is a different problem, and not one to solve quietly
    ///
    /// Every `while` in both corpora is an iterative solver that stops on a
    /// tolerance — secant, Newton, Broyden, Richardson extrapolation. Nomo's
    /// `iterate` takes a *count*, because a count is reproducible and a
    /// tolerance test is not (design note §3, and the same reasoning that gives
    /// `root` a fixed number of bisections). Translating one would mean
    /// choosing the number of steps, and that number decides the answer. So
    /// they are reported, with the reason.
    fn loop_region(&mut self, e: &Expr) {
        let Expr::Call { name, args } = e else {
            return self.unsupported("a loop");
        };
        if name == "while" {
            return self.unsupported(
                "a `while` loop, which runs until a tolerance is met; Nomo's \
                 `iterate` takes a count, so the number of steps would have to \
                 be invented",
            );
        }
        let fill = match Fill::read(args) {
            Ok(fill) => fill,
            Err(why) => return self.unsupported(&format!("a `for` loop: {why}")),
        };
        // A loop that translates but reads something the import never gave a
        // value to. The translation is right and cannot run, which is neither
        // of the two things the emitter usually does — so it does what the
        // free-symbol case does: says why, and leaves the translated lines
        // underneath as comments. Emitting them live would trade a marker for a
        // red line and blame the loop for a gap that is somewhere above it;
        // dropping them would hide a translation that is correct.
        let mut skip: BTreeSet<&str> = fill.writes.iter().map(|w| w.target).collect();
        skip.insert(fill.var);
        let missing = self.unvalued(e, &skip);
        if !missing.is_empty() {
            let named = missing
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ");
            self.unsupported(&format!(
                "a `for` loop that reads {named}, which {} no value here",
                if missing.len() == 1 { "has" } else { "have" }
            ));
            self.commenting = true;
            let written = self.write_fill(&fill);
            // Still commenting, so a body that also fails to translate adds its
            // reason as a second marker line without counting the region twice:
            // it is one region for a human to look at, whatever is wrong with
            // it. See [`Emitter::unsupported`].
            if let Err(why) = written {
                self.unsupported(&format!("and its body does not translate: {why}"));
            }
            self.commenting = false;
            return;
        }
        if let Err(why) = self.write_fill(&fill) {
            self.unsupported(&format!("a `for` loop: {why}"));
        }
    }

    /// Write one fill loop out as `map` over a range, one definition per name it
    /// fills.
    fn write_fill(&mut self, fill: &Fill) -> Result<(), String> {
        let Some(param) = self.names.variable(fill.var) else {
            return Err(format!("`{}` is not a name Nomo can spell", fill.var));
        };
        // The range is written once and read by every curve the loop fills, so
        // that the vectors are the same length by construction.
        let from = self.at(fill.from, CONDITIONAL)?;
        let to = self.at(fill.to, SUM)?;
        let to = if fill.exclusive {
            // `i < 201` is `range(1, 200)`: Nomo's range includes both ends.
            // Folded when the end is a literal, because `range(1, 201 - 1)` is
            // a worse thing to hand a reader than the number SMath meant.
            match to.parse::<i64>() {
                Ok(n) => (n - 1).to_string(),
                Err(_) => format!("{to} - 1"),
            }
        } else {
            to
        };
        let span = format!("range({from}, {to})");

        for (target, writes) in fill.by_target() {
            let Some(name) = self.names.variable(target) else {
                return Err(format!("`{target}` is not a name Nomo can spell"));
            };
            // A name that already holds a value is being *modified* by the
            // loop, not built by it, and a `map` writes the whole vector: any
            // element the loop does not reach would be silently dropped.
            if self.valued.contains(target) {
                return Err(format!(
                    "`{target}` already has a value here, so the loop changes it \
                     rather than building it"
                ));
            }
            let mut columns: Vec<(usize, String)> = Vec::with_capacity(writes.len());
            for w in &writes {
                if let Some(captured) = self.captures(w.value, fill.var) {
                    return Err(capture_refused("for", &captured));
                }
                let body = self.expr(w.value)?;
                let kind = match w.column {
                    Some(c) if writes.len() > 1 => format!("{name}_col{c}"),
                    _ => format!("{name}_at"),
                };
                let lifted = self.lift(&kind, &param, &body);
                columns.push((w.column.unwrap_or(1), format!("map({lifted}, {span})")));
            }
            columns.sort_by_key(|(c, _)| *c);
            if columns.iter().enumerate().any(|(i, (c, _))| *c != i + 1) {
                return Err(format!(
                    "`{target}` is filled in columns that are not 1, 2, … in order"
                ));
            }
            let value = if columns.len() == 1 {
                columns.remove(0).1
            } else {
                let parts: Vec<String> = columns.into_iter().map(|(_, m)| m).collect();
                // The columns were written side by side, which is what `augment`
                // is. A single column is left as the vector it is — the same
                // reading `mat` already gives a one-column matrix.
                format!("augment({})", parts.join(", "))
            };
            self.push(&format!("{name} = {value}"));
        }
        Ok(())
    }

    /// A plot of a function of `x`, over the span the stored viewport implies.
    ///
    /// # Where the span comes from
    ///
    /// It is not in the file as a number, and for a long time this looked like
    /// a question the file could not answer. It can, because the viewport is a
    /// *complete* description of the frame once SMath's own arithmetic is
    /// known, and that arithmetic is in `PlotRegion.dll`:
    ///
    /// - Loading a 2D plot sets the frame to `10·(w/h)/1.66` pixels per unit —
    ///   `Renderer` is constructed with `limits_* = 10` and the 2D branch
    ///   multiplies by the region's aspect over 1.66.
    /// - Restoring a saved view calls `Renderer::Transpose(…)` and then
    ///   `Renderer::Scale(…)`, and `Scale` multiplies *both* the saved `scale_*`
    ///   and the live frame by the same factor. So the frame after loading is
    ///   `10·(w/h)/1.66·scale_y`.
    /// - The plotting method takes the visible x as `±w/2` divided by that
    ///   frame, then shifts it by `-transpose_x` divided by the same.
    ///
    /// The field names are crossed in SMath: the horizontal extent divides by
    /// `limits_y`, which is what `scale_y` scales. Reading `scale_x` instead
    /// gives spans that are absurd for every worksheet — a standard normal
    /// drawn over ±0.73 — which is a second, independent check on the reading.
    ///
    /// # What it was checked against
    ///
    /// Six worksheets, none of which records its domain anywhere: the standard
    /// normal comes out over ±4.8, Student's t over −4.1…3.9, χ² over −2…26, F
    /// over −0.4…2.8, a Newton-Raphson demo over −4.9…7.4 — and the converter
    /// worksheet's three-curve LLC gain plot over **25.5 kHz…202.6 kHz**, which
    /// is the span `examples/plots.nomo` draws a gain family over, arrived at
    /// there by reading a worksheet and here by arithmetic.
    ///
    /// # What is still approximate
    ///
    /// The width used is the region's stored box. SMath measures the canvas
    /// inside it, and a region carries a few pixels of frame — the same few
    /// pixels that make an imported `<picture>` about five pixels a side larger
    /// than SMath drew it. So the span is right to within a percent or so at
    /// the edges, not to the last bit, and the span is written to six figures
    /// rather than seventeen to say so.
    fn function_plot(&mut self, series: &[&Expr], tag: &str, lo: f64, hi: f64) {
        let mut drawn = Vec::with_capacity(series.len());
        for s in series {
            let body = match self.expr(s) {
                Ok(body) => body,
                // One series that will not translate takes the whole plot with
                // it, the same rule the table path keeps.
                Err(why) => return self.unsupported(&format!("a `{tag}`: {why}")),
            };
            // `plot` takes the name of a function, so a plotted expression is
            // lifted into one — the move `int` already makes for an integrand.
            // A series that is nothing but a call to one of the worksheet's own
            // functions at `x` already *is* that function, and lifting it would
            // rename it for nothing: `sys(Mg(x), Mg2(x), 2, 1)` draws as
            // `plot(Mg, Mg2, …)`, which is what the worksheet says.
            drawn.push(
                self.already_a_function(s, "x")
                    .unwrap_or_else(|| self.lift("curve", "x", &body)),
            );
        }
        self.push(&format!(
            "plot({}, {}, {})",
            drawn.join(", "),
            six_figures(lo),
            six_figures(hi)
        ));
        let what = if series.len() == 1 {
            String::from("a function of `x`")
        } else {
            format!("{} functions of `x`", series.len())
        };
        self.note(
            NoteKind::Carried,
            format!(
                "a `{tag}` of {what}, drawn over the span its stored \
                 viewport implies ({} to {})",
                six_figures(lo),
                six_figures(hi)
            ),
        );
    }

    /// The span a 2D `<plot>`'s stored viewport implies, or why there is none.
    ///
    /// The arithmetic and the evidence for it are in [`Emitter::function_plot`]
    /// and design note §8.21. It is separate from the drawing because it is the
    /// *first* question a plot of a function has to answer — before what its
    /// series are waiting for, and before whether they are all curves — and a
    /// marker is worth most when it names the obstacle a reader meets first.
    fn span(view: Option<PlotView>, size: (i64, i64)) -> Result<(f64, f64), &'static str> {
        // An `<xyplot>` keeps its own axes, and a 3D plot is a surface.
        let (Some(view), (w, h)) = (view, size) else {
            return Err("a function of `x`, and this region's kind records no span");
        };
        if w <= 0 || h <= 0 || !(view.scale_y.is_finite() && view.scale_y > 0.0) {
            return Err("a function of `x`, and its viewport is degenerate");
        }
        let frame = 10.0 * (w as f64 / h as f64) / 1.66 * view.scale_y;
        Ok((
            (-(w as f64) / 2.0 - view.transpose_x as f64) / frame,
            (w as f64 / 2.0 - view.transpose_x as f64) / frame,
        ))
    }

    /// What these series read that has no value yet, phrased for a marker.
    ///
    /// One list across the whole plot rather than one per series: half a chart
    /// is worse than a marker saying what the whole one is waiting for.
    fn missing(&self, series: &[&Expr], skip: &BTreeSet<&str>) -> Option<String> {
        let mut missing: Vec<String> = Vec::new();
        for s in series {
            for name in self.unvalued(s, skip) {
                if !missing.contains(&name) {
                    missing.push(name);
                }
            }
        }
        missing.sort();
        if missing.is_empty() {
            return None;
        }
        let named = missing
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "{named} {} no value here",
            if missing.len() == 1 { "has" } else { "have" }
        ))
    }

    /// An SMath `<plot>` or `<xyplot>`.
    ///
    /// # Two kinds, and the file says which
    ///
    /// A `<plot>` holds one expression and no domain — `scale_*`, `rotate_*`
    /// and `transpose_*` are a viewport, and two attempts to decode a domain
    /// from them disagreed by four orders of magnitude (design note §8.20). So
    /// a plot of a *function of x* is a plot nobody can say the domain of, and
    /// it keeps its marker until that question is settled with ground truth.
    ///
    /// A plot of a **table of points** has no such gap: the points brought
    /// their own x, and `plot(m)` needs no span. That is most of the corpus's
    /// plots — `XY = augment(x, y)` and then `XY` plotted, and the same shape
    /// under a dozen other names.
    ///
    /// # Free symbols tell them apart, and nothing else has to
    ///
    /// The discriminator is the one the worksheet already answers: a plot of a
    /// function of x mentions an `x` the document never binds, and a plot of a
    /// table names something the document defines. So the test is
    /// [`Bound::free_in`], the same machinery every math region goes through —
    /// no new rule, no guess about what a name is likely to hold, and it stays
    /// right when a name is bound by something the importer cannot translate:
    /// `NASA_atmosphere.sm` builds its table inside a `for` loop with `el(M, i,
    /// 1) ← …`, so `M` is free in the imported worksheet and its plot correctly
    /// keeps a marker naming `M` rather than drawing a chart of nothing.
    ///
    /// What the marker gains is the *reason*: it now names the symbols, which
    /// separates "this is a function plot and needs a span" from "this table is
    /// built by a construct that did not import".
    ///
    /// # `sys(…)`
    ///
    /// SMath's several-series plot. Nomo draws at most two tables on one plot,
    /// so two operands map straight onto `plot(a, b)` and more keep the marker
    /// rather than losing a series quietly.
    fn plot(&mut self, expr: &Expr, tag: &str, view: Option<PlotView>, size: (i64, i64)) {
        // The expression as the file holds it. Every refusal below keeps it
        // beside the reason, and it is the worksheet's own text rather than
        // anything the rewrites below turned it into.
        let of = match self.expr(expr) {
            Ok(e) => format!(" of {e}"),
            Err(_) => String::new(),
        };
        // Two rewrites, both undoing something SMath had to write because it
        // has no function-valued definition:
        //
        // - a name whose definition is a list of series *is* that list, since a
        //   plot is the only place a list of series means anything — see
        //   [`Emitter::curve`];
        // - a name the document defined as a function of `x` is applied to `x`,
        //   which is what SMath does with it: `plot_Z/ohm` is drawn as
        //   `plot_Z(x)/ohm` — see [`curves_of_x`].
        //
        // After both, the plot takes the same path as one written that way in
        // the file to begin with.
        let listed = match expr {
            Expr::Name(n) => self.series.get(n).cloned(),
            _ => None,
        };
        let applied = applied_to_x(
            &listed.unwrap_or_else(|| expr.clone()),
            &self.curves_emitted,
        );
        let expr = &applied;
        // `sys(s1, …, sn, n, 1)` is series, not arithmetic: its operands are the
        // plots, and the last two are the shape — the same `n`-plus-two arity
        // `mat` and `line` are written with. Anything else is one series.
        let series: Vec<&Expr> = match expr {
            Expr::Call { name, args } if name == "sys" && args.len() >= 3 => {
                match series_of(expr) {
                    Some(listed) => listed,
                    // A shape that is not `n` by 1 is not a list of series, and
                    // reading it as one would drop or invent a curve.
                    None => {
                        return self.unsupported(&format!("a `{tag}` whose series are not a list"))
                    }
                }
            }
            other => vec![other],
        };

        // A plot of a function of x: SMath's 2D plot variable is `x`, and every
        // function plot in both corpora is written in it — `f(x)`, `f1(x)`,
        // `fχ2(x, ν)`. Those are the plots that need the span the file does not
        // record, so mentioning `x` is enough to keep the marker. Conservative
        // on purpose and in the cheap direction: a table that happens to be
        // computed from a variable called `x` reports as untranslatable, which
        // is a marker to look at rather than a chart drawn over an invented
        // domain.
        let reads_x = |e: &&Expr| {
            let mut yes = false;
            e.walk(&mut |e| {
                if let Expr::Name(n) = e {
                    yes |= n == "x";
                }
            });
            yes
        };
        if series.iter().any(reads_x) {
            let (lo, hi) = match Emitter::span(view, size) {
                Ok(span) => span,
                Err(why) => return self.unsupported(&format!("a `{tag}`{of}: {why}")),
            };
            // Everything but the plot variable has to have a value, exactly as
            // for a plot of a table: `x` is about to become a parameter, so it
            // is the one name that does not.
            let mut skip = BTreeSet::new();
            skip.insert("x");
            if let Some(waiting) = self.missing(&series, &skip) {
                return self.unsupported(&format!("a `{tag}`{of}: {waiting}"));
            }
            // One chart in SMath, two in Nomo: a curve is sampled over a span
            // and a table brings its own points, and `plot` draws one kind at a
            // time. Asked last, because a mixed plot whose tables have no values
            // yet has a nearer problem than its shape. Refusing names the plot
            // that cannot be drawn; drawing the curves alone would lose a series
            // without saying so.
            if !series.iter().all(reads_x) {
                return self.unsupported(&format!(
                    "a `{tag}`{of}: some of its series are functions of `x` and some are not"
                ));
            }
            return self.function_plot(&series, tag, lo, hi);
        }

        // Every name the plot reads has to hold a value here, or there is
        // nothing to draw. This is the case the corpus is full of: the table is
        // real and SMath drew it, but it was filled by a loop that mutates, so
        // the import has the name and not the numbers.
        if let Some(waiting) = self.missing(&[expr], &BTreeSet::new()) {
            return self.unsupported(&format!("a `{tag}`{of}: {waiting}"));
        }
        if series.len() > 2 {
            return self.unsupported(&format!(
                "a `{tag}` of {} series; Nomo draws at most two tables on one plot",
                series.len()
            ));
        }
        let mut drawn = Vec::with_capacity(series.len());
        for e in &series {
            match self.expr(e) {
                Ok(text) => drawn.push(text),
                // One series that will not translate takes the whole plot with
                // it: half a chart is worse than a marker saying so.
                Err(why) => return self.unsupported(&format!("a `{tag}`: {why}")),
            }
        }
        self.push(&format!("plot({})", drawn.join(", ")));
        // The chart is drawn, but not the way SMath drew it: the viewport it
        // stored is dropped, and Nomo fits both axes to the data instead.
        // Counted rather than silent, for the reason `Carried` exists.
        self.note(
            NoteKind::Carried,
            format!(
                "a `{tag}` of {}, redrawn with both axes fitted to the data — \
                 SMath's zoom and position are not carried",
                drawn.join(", ")
            ),
        );
    }

    fn picture(&mut self, format: &str, data: &str, size: Option<(u32, u32)>) {
        // A `<picture>` with no `<raw>` under it. Nothing was stored, so there is
        // nothing to carry, and saying "carried" about an empty blob would be a
        // lie in the one direction that matters.
        if data.is_empty() {
            return self.unsupported(&format!("an embedded {format} image with no data"));
        }
        let name = format!("figure{}", self.resources.len() + 1);
        match size {
            Some((w, h)) => self.push(&format!("' image {name} {w}x{h}")),
            None => self.push(&format!("' image {name}")),
        }
        self.note(
            NoteKind::Carried,
            format!(
                "an embedded {format} image ({} bytes), carried as `{name}`",
                decoded_len(data)
            ),
        );
        self.resources.push(Resource {
            name,
            format: format.to_string(),
            data: data.to_string(),
        });
    }

    /// SMath's page header and footer, kept out of the body.
    ///
    /// It repeated on every printed page rather than forming part of the
    /// document, and Nomo has no page model, so there is nowhere for it to go.
    /// Dropping it is not the answer — that is what the reader was doing, for
    /// one file in 118 — but neither is emitting it as content. The interlock
    /// worksheet's header carries a date, which SMath stores as three
    /// operands and two subtractions; as a worksheet line that is a query
    /// evaluating to -2025. So the mathematics of a header is **shown and not
    /// run**, the treatment a plot's expression already gets, and its figures
    /// are carried like any other.
    fn write_furniture(&mut self, w: &Worksheet) {
        let regions = w.flat_furniture();
        if regions.is_empty() {
            return;
        }
        self.push("");
        self.push("' --- page header ---");
        self.push("' Repeated on every printed page in SMath rather than forming");
        self.push("' part of the document. Nomo has no page model, so this is");
        self.push("' kept out of the way: nothing is lost, and nothing is run.");
        for region in regions {
            match &region.payload {
                Payload::Text { variants } => {
                    let text = self.choose_language(variants);
                    for line in text.lines().filter(|l| !l.trim().is_empty()) {
                        self.push(&format!("' {}", line.trim_end()));
                    }
                }
                Payload::Picture { format, data, size } => self.picture(format, data, *size),
                Payload::Math(m) => {
                    let shown = match &m.statement {
                        Statement::Bare(e) => self.expr(e).ok(),
                        _ => None,
                    };
                    match shown {
                        Some(e) => self.unsupported(&format!("page header math: {e}")),
                        None => self.unsupported("page header math"),
                    }
                }
                _ => self.unsupported("a page header region"),
            }
        }
    }

    /// The images, appended in one block at the end of the file.
    ///
    /// # Why a trailer, and not at the point of use
    ///
    /// A `.nomo` file *is* its source text — there is no container to put a
    /// resource fork in — so an image can only live in it as base64. Put where
    /// the figure stood, the interlock worksheet's largest becomes a 116 KB
    /// line in the middle of the worksheet, and that destroys the property the
    /// text format
    /// was chosen for: that worksheets diff and review like code. Collected at
    /// the end, the body stays readable and the blobs are one contiguous,
    /// append-only region that `.gitattributes` can mark `-diff`.
    ///
    /// The alternative — external files beside the worksheet — is what the
    /// browser cannot do: `storage.js` opens a *file* handle, not a directory,
    /// and the Firefox and Safari fallback has no filesystem at all. Inline is
    /// the only form that survives every path the application already supports.
    ///
    /// # Why it is all comments
    ///
    /// Every line here is an ordinary `'` comment, so the output parses today
    /// and a worksheet keeps its figures through an import with no change to the
    /// engine. Nothing renders them yet, which is what the `Carried` note says.
    /// Whether this later becomes a real statement is a language decision, and
    /// making it one now would have been deciding it by writing an importer.
    fn write_trailer(&mut self) {
        if self.resources.is_empty() {
            return;
        }
        self.push("");
        self.push("' --- resources ---");
        self.push("' Images from the SMath worksheet, base64, in reading order.");
        self.push("' A block is `' image <name> <format> <bytes>` followed by the");
        self.push("' indented lines under it, up to the next block or end of file.");
        for r in core::mem::take(&mut self.resources) {
            self.push(&format!(
                "' image {} {} {}",
                r.name,
                r.format,
                decoded_len(&r.data)
            ));
            // 76 is the MIME line length, and with the marker and indent the
            // line lands on 80 columns.
            for chunk in wrapped(&r.data, 76) {
                self.push(&format!("'   {chunk}"));
            }
        }
    }

    fn unsupported(&mut self, what: &str) {
        self.push(&format!("' [import] unsupported: {what}"));
        // A region already marked keeps its marker lines — a second reason is
        // worth reading — but is counted once. The notes rank *regions a human
        // has to look at*, and a line that is untranslatable twice over is still
        // one line to look at.
        if !self.commenting {
            self.notes.push(Note {
                line: self.line,
                kind: NoteKind::Unsupported,
                detail: what.to_string(),
            });
        }
    }

    fn note(&mut self, kind: NoteKind, detail: String) {
        self.notes.push(Note {
            line: self.line,
            kind,
            detail,
        });
    }
}

/// `s` in pieces of at most `width` characters.
fn wrapped(s: &str, width: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        // Base64 is ASCII and the reader strips whitespace, so every piece is
        // `width` bytes in practice. Cutting on a character boundary anyway
        // because this is third-party input, and the failure mode of assuming
        // otherwise is a panic rather than a report of a bad file.
        let cut = rest
            .char_indices()
            .nth(width)
            .map_or(rest.len(), |(i, _)| i);
        let (head, tail) = rest.split_at(cut);
        out.push(head);
        rest = tail;
    }
    out
}

/// Binding powers, matching `docs/language.md`'s table so that what is emitted
/// parses back as what was meant.
const CONDITIONAL: u8 = 0;
const AND: u8 = 2;
const NOT: u8 = 3;
const COMPARE: u8 = 4;
const SUM: u8 = 5;
const PRODUCT: u8 = 6;
const UNARY: u8 = 7;
const POWER: u8 = 8;
const ATOM: u8 = 9;

/// What to call an SMath function in Nomo, if Nomo has it.
///
/// The engine's own `BUILTINS` is the authority for what exists, rather than a
/// second list here that could drift from it. Only the renames are spelled out.
fn nomo_function(name: &str) -> Option<&'static str> {
    // `norme` is the CustomFunctions plugin's Euclidean norm. Settled by the
    // corpus rather than by its name: `7.3.sm` divides a vector by it nine times
    // running and every quotient is a unit vector, which only the Euclidean norm
    // produces.
    const RENAMED: &[(&str, &str)] = &[("invert", "inv"), ("norme", "norm")];
    if let Some((_, to)) = RENAMED.iter().find(|(from, _)| *from == name) {
        return Some(to);
    }
    nomo_core::eval::BUILTINS
        .iter()
        .find(|b| **b == name)
        .copied()
}

/// The first numeric literal of each part of a stored answer that has several.
///
/// Empty for a plain scalar, which is most of them. Two kinds have parts:
///
/// - a **matrix**, `mat(e₁, …, eₙ, rows, cols)`, whose shape operands are
///   dropped and whose elements each contribute their own first number — `-61.1`
///   is a negation rather than a literal, and its precision is written on the
///   `61.1` inside it;
/// - a **complex** answer, `re ± im·i`, whose two parts are written to their own
///   significant places: `14.88 - 9.47·i` is one part to two decimals beside
///   another to two, and `196.18 - 20.29·i` would be five significant figures
///   beside four if a single mantissa had to speak for both.
fn element_numbers(e: &Expr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    e.walk(&mut |n| {
        if !out.is_empty() {
            return;
        }
        if let Expr::Call { name, args } = n {
            if name == "mat" && args.len() >= 3 {
                let (elements, _shape) = args.split_at(args.len() - 2);
                out = elements.iter().filter_map(first_number).collect();
            }
        }
        // `re ± im·i`: the addition whose right side is the imaginary term.
        // Found by looking for `i` rather than by assuming an operand order,
        // because the sign is on the operator and the unit may be outside the
        // whole thing — `(14.88 - 9.47·i)·A`.
        if let Expr::Op { glyph, args } = n {
            if (glyph == "+" || glyph == "-") && args.len() == 2 && mentions_i(&args[1]) {
                out = args.iter().filter_map(first_number).collect();
            }
        }
    });
    out
}

/// Whether this expression names SMath's imaginary unit.
fn mentions_i(e: &Expr) -> bool {
    let mut found = false;
    e.walk(&mut |n| {
        if let Expr::Name(name) = n {
            found |= name == "i";
        }
    });
    found
}

/// The first numeric literal in an expression, in source order.
fn first_number(e: &Expr) -> Option<String> {
    let mut found = None;
    e.walk(&mut |n| {
        if found.is_none() {
            if let Expr::Number(t) = n {
                found = Some(t.clone());
            }
        }
    });
    found
}

/// One `el(A, i)` or `el(A, i, c)` a fill loop writes.
#[derive(Debug)]
struct Write<'a> {
    target: &'a str,
    /// The column, for a loop that fills a table side by side. `None` when the
    /// loop writes a vector.
    column: Option<usize>,
    value: &'a Expr,
}

/// A `for` loop that builds vectors element by element — the shape `map` says.
///
/// Reading one is a series of refusals, and the message each returns is the
/// point: a loop that is not this shape keeps a marker naming what about it did
/// not fit, so a coverage report can rank the remaining kinds.
#[derive(Debug)]
struct Fill<'a> {
    /// The loop variable, which becomes the mapped function's parameter.
    var: &'a str,
    from: &'a Expr,
    to: &'a Expr,
    /// Whether the end is excluded — `i < 201` rather than `i ≤ 200`. Nomo's
    /// `range` includes both ends.
    exclusive: bool,
    writes: Vec<Write<'a>>,
}

impl<'a> Fill<'a> {
    fn read(args: &'a [Expr]) -> Result<Fill<'a>, String> {
        let (var, from, to, exclusive, body) = match args {
            // `for(i, range(a, b), body)`.
            [Expr::Name(var), Expr::Call { name, args: r }, body]
                if name == "range" && r.len() == 2 =>
            {
                (var.as_str(), &r[0], &r[1], false, body)
            }
            [_, Expr::Call { name, args: r }, _] if name == "range" && r.len() == 3 => {
                return Err(String::from(
                    "over a `range` with a step, whose meaning in SMath is unverified",
                ))
            }
            [_, _, _] => return Err(String::from("over something that is not a `range`")),
            // `for(i ← a, i < b, i ← i + 1, body)`, the counted form.
            [init, cond, step, body] => {
                let (target, from) = assigned(init).ok_or("whose first operand is not `i ← a`")?;
                let Expr::Name(var) = target else {
                    return Err(String::from("whose counter is not a plain name"));
                };
                let counter = Expr::Name(var.clone());
                let (stepped, next) = assigned(step).ok_or("whose third operand is not `i ← …`")?;
                if *stepped != counter {
                    return Err(String::from("that steps something other than its counter"));
                }
                // Only `i + 1`. A larger step is a different sequence, and
                // `range` with a step is refused for the reason §8 gives.
                let by_one = matches!(next, Expr::Op { glyph, args }
                    if glyph == "+"
                        && args.len() == 2
                        && args[0] == counter
                        && matches!(&args[1], Expr::Number(n) if n == "1"));
                if !by_one {
                    return Err(String::from("whose counter does not step by one"));
                }
                let Expr::Op { glyph, args: c } = cond else {
                    return Err(String::from("whose condition is not a comparison"));
                };
                if c.len() != 2 || c[0] != counter {
                    return Err(String::from("whose condition is not on its counter"));
                }
                match glyph.as_str() {
                    "<" => (var.as_str(), from, &c[1], true, body),
                    "≤" => (var.as_str(), from, &c[1], false, body),
                    // `>` counts down, and a countdown fills a vector back to
                    // front; `≠` says nothing about how far it runs.
                    _ => return Err(format!("whose condition is `{glyph}`, not `<` or `≤`")),
                }
            }
            _ => return Err(String::from("with an unrecognised header")),
        };

        // The body: one assignment, or a `line(s1, …, sn, n, 1)` block of them.
        let statements: Vec<&Expr> = match body {
            Expr::Call { name, args } if name == "line" && args.len() >= 3 => {
                args[..args.len() - 2].iter().collect()
            }
            other => vec![other],
        };
        let mut writes = Vec::with_capacity(statements.len());
        for s in statements {
            let Some((target, value)) = assigned(s) else {
                return Err(String::from(
                    "whose body does more than assign, so it is a program rather than a table",
                ));
            };
            let Expr::Call { name, args: idx } = target else {
                return Err(String::from(
                    "that binds a whole name rather than an element",
                ));
            };
            if name != "el" {
                return Err(format!("that assigns into `{name}(…)`"));
            }
            let Some(Expr::Name(base)) = idx.first() else {
                return Err(String::from("that writes into something unnamed"));
            };
            let column = match idx.len() {
                2 => None,
                3 => match &idx[2] {
                    Expr::Number(c) => Some(
                        c.parse::<usize>()
                            .map_err(|_| String::from("whose column is not a whole number"))?,
                    ),
                    _ => return Err(String::from("whose column is computed")),
                },
                _ => return Err(String::from("with more than two indices")),
            };
            // The map writes positions 1, 2, 3 … in order, so the index has to
            // be the loop variable and the loop has to start at the first
            // element. `el(x, i + 1)` over `range(0, n)` is the same sequence
            // and is accepted; `el(v, j + 1)` over `range(1, n)` leaves the
            // first element unwritten and is not.
            let offset =
                index_offset(&idx[1], var).ok_or("whose index is not its loop variable")?;
            let starts_at_one = match from {
                Expr::Number(n) => n.parse::<i64>().map(|a| a + offset == 1).unwrap_or(false),
                _ => false,
            };
            if !starts_at_one {
                return Err(String::from(
                    "that does not begin at the first element of what it fills",
                ));
            }
            writes.push(Write {
                target: base.as_str(),
                column,
                value,
            });
        }
        if writes.is_empty() {
            return Err(String::from("with an empty body"));
        }

        // Every element must be computable on its own. A body that reads a name
        // it also writes is a recurrence — `el(β, i) ← … el(β, i - 1) …` — and a
        // recurrence is a fold, not a map: it has an order, and `map` has none.
        let filled: BTreeSet<&str> = writes.iter().map(|w| w.target).collect();
        for w in &writes {
            if let Some(n) = reads_any(w.value, &filled) {
                return Err(format!(
                    "whose body reads `{n}` while filling it, so each element depends on the last"
                ));
            }
        }
        // The same, for how far the loop runs: `range(1, length(yy))` while
        // filling `yy` cannot be written as one definition of `yy`.
        for bound in [from, to] {
            if let Some(n) = reads_any(bound, &filled) {
                return Err(format!(
                    "that runs as far as `{n}`, which it is itself filling"
                ));
            }
        }
        Ok(Fill {
            var,
            from,
            to,
            exclusive,
            writes,
        })
    }

    /// The writes grouped by the name they fill, in the order the loop first
    /// mentions each — which is the order they are written out.
    fn by_target(&self) -> Vec<(&'a str, Vec<&Write<'a>>)> {
        let mut out: Vec<(&str, Vec<&Write>)> = Vec::new();
        for w in &self.writes {
            match out.iter_mut().find(|(t, _)| *t == w.target) {
                Some((_, group)) => group.push(w),
                None => out.push((w.target, vec![w])),
            }
        }
        out
    }
}

/// A span end, to six significant figures.
///
/// Not seventeen: the span is derived from a region box that is a few pixels
/// wider than the canvas SMath measured, so its last digits are noise. Six is
/// past where a chart could show a difference and short enough to read.
fn six_figures(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return format!("{x}");
    }
    let magnitude = x.abs().log10().floor() as i32;
    let places = (5 - magnitude).max(0) as usize;
    let text = format!("{x:.places$}");
    // Trim the zeros the fixed format leaves behind, and the point with them.
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    text
}

/// The two sides of `target : value` or `target ← value`.
fn assigned(e: &Expr) -> Option<(&Expr, &Expr)> {
    match e {
        Expr::Op { glyph, args } if (glyph == ":" || glyph == "←") && args.len() == 2 => {
            Some((&args[0], &args[1]))
        }
        _ => None,
    }
}

/// Whether this expression calls `name`.
fn calls(e: &Expr, name: &str) -> bool {
    let mut found = false;
    e.walk(&mut |e| {
        if let Expr::Call { name: called, .. } = e {
            found |= called == name;
        }
    });
    found
}

/// The first of `names` this expression reads, if it reads any.
fn reads_any(e: &Expr, names: &BTreeSet<&str>) -> Option<String> {
    let mut found = None;
    e.walk(&mut |e| {
        if let Expr::Name(n) = e {
            if found.is_none() && names.contains(n.as_str()) {
                found = Some(n.clone());
            }
        }
    });
    found
}

/// `i`, `i + 1`, `i - 1`: how far the index runs ahead of the loop variable.
fn index_offset(idx: &Expr, var: &str) -> Option<i64> {
    let counter = Expr::Name(var.to_string());
    if *idx == counter {
        return Some(0);
    }
    let Expr::Op { glyph, args } = idx else {
        return None;
    };
    if args.len() != 2 || args[0] != counter {
        return None;
    }
    let Expr::Number(n) = &args[1] else {
        return None;
    };
    let n: i64 = n.parse().ok()?;
    match glyph.as_str() {
        "+" => Some(n),
        "-" => Some(-n),
        _ => None,
    }
}

/// The plot a `line(…)` block configures, if that is all it does.
///
/// Plot properties are reached by a path in the operand name itself —
/// `XYPlot'Traces#0'Name`, `VPlot'Labels'XLabel` — 159 of them across 23 files.
/// A block is configuration when every statement in it assigns to such a path;
/// one that also computes something is left to the ordinary route, because
/// dropping arithmetic on the grounds that a plot was mentioned would lose work.
fn plot_configuration(e: &Expr) -> Option<String> {
    let Expr::Call { name, args } = e else {
        return None;
    };
    if name != "line" || args.len() < 3 {
        return None;
    }
    // `line(s1, …, sn, n, 1)`: the last two operands are the block's shape.
    let statements = &args[..args.len() - 2];
    let mut plot = None;
    for s in statements {
        let Expr::Op { glyph, args } = s else {
            return None;
        };
        if glyph != ":" && glyph != "←" {
            return None;
        }
        let Some(Expr::Name(target)) = args.first() else {
            return None;
        };
        let (owner, rest) = target.split_once('\'')?;
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        plot.get_or_insert_with(|| owner.to_string());
    }
    plot
}

fn shape(e: &Expr) -> &'static str {
    match e {
        Expr::Number(_) => "a number",
        Expr::Name(_) => "a name",
        Expr::Unit(_) => "a unit",
        Expr::Text(_) => "a string",
        Expr::Call { .. } => "a call",
        Expr::Op { .. } => "an expression",
        Expr::Unsupported { .. } => "something unreadable",
    }
}

/// The SMath-name-to-Nomo-name mapping for one worksheet.
///
/// Built once per document so that collisions can be detected: two SMath names
/// that respell to the same Nomo name would silently become one variable, which
/// is a wrong answer rather than a failed import.
#[derive(Debug, Clone, Default)]
struct Names {
    map: BTreeMap<String, String>,
    /// Variables the worksheet binds under a name it also uses as a unit, and
    /// the name each was moved to. See [`shadowed_units`].
    shadowed: BTreeMap<String, String>,
    collisions: Vec<String>,
}

impl Names {
    fn build(w: &Worksheet) -> Names {
        let mut names = Names::default();
        let mut taken: BTreeMap<String, String> = BTreeMap::new();
        let mut seen = Vec::new();
        // Plot expressions are the worksheet's content too, and a name that
        // appears only inside one still needs a Nomo spelling — otherwise the
        // plot reports as untranslatable for want of a variable it shares with
        // the lines above it.
        let collect = |e: &Expr, seen: &mut Vec<String>| match e {
            Expr::Name(n) | Expr::Unit(n) => seen.push(n.clone()),
            Expr::Call { name, .. } => seen.push(name.clone()),
            _ => {}
        };
        for r in w.flat() {
            if let Payload::Plot { expr, .. } = &r.payload {
                expr.walk(&mut |e| collect(e, &mut seen));
            }
        }
        for m in w.math() {
            m.statement.walk(&mut |e| collect(e, &mut seen));
            // A stored answer is a token stream like any other and carries the
            // unit SMath computed — which is routinely a unit the input side
            // never writes. The converter worksheet states its inputs in volts
            // and amps and SMath answers in ohms, farads and henries, so
            // leaving the results
            // out here left seventeen of its thirty-four stored answers with no
            // Nomo spelling for their unit, and therefore unusable as
            // assertions: the file's own numbers, unchecked, for want of a name.
            for e in [&m.result, &m.contract].into_iter().flatten() {
                e.walk(&mut |e| collect(e, &mut seen));
            }
        }
        seen.sort();
        seen.dedup();
        for original in seen {
            let Some(spelled) = respell(&original) else {
                continue;
            };
            if let Some(other) = taken.get(&spelled) {
                if *other != original {
                    names.collisions.push(format!(
                        "`{original}` and `{other}` both become `{spelled}`"
                    ));
                    continue;
                }
            }
            taken.insert(spelled.clone(), original.clone());
            names.map.insert(original, spelled);
        }
        for original in shadowed_units(w) {
            let Some(spelled) = names.map.get(&original) else {
                continue;
            };
            // Trailing underscores until the name is nobody else's. One is
            // almost always enough, and it keeps the author's word visible —
            // whoever reads the imported worksheet is looking for the `m` they
            // wrote, and `m_` is still that name.
            let mut moved = format!("{spelled}_");
            while taken.contains_key(&moved) {
                moved.push('_');
            }
            taken.insert(moved.clone(), original.clone());
            names.shadowed.insert(original, moved);
        }
        names
    }

    fn get(&self, original: &str) -> Option<String> {
        self.map.get(original).cloned()
    }

    /// The spelling to use where this name stands for a **value**.
    ///
    /// Differs from [`Names::get`] only for a name the worksheet uses as a unit
    /// as well, which Nomo cannot spell two ways at once.
    fn variable(&self, original: &str) -> Option<String> {
        self.shadowed
            .get(original)
            .or_else(|| self.map.get(original))
            .cloned()
    }
}

/// Names the worksheet binds as variables while also using them as units.
///
/// SMath tells the two apart by an attribute on the operand, so `m := 1 kg` and
/// `d := 1 N*(m/s)^-1` can stand four lines apart and mean a mass and a metre.
/// Nomo has one namespace, and a binding hides a unit of the same name for the
/// rest of the worksheet (`SH202`) — so emitting both as `m` turns every length
/// below into a mass.
///
/// **The wrongness is silent, which is why this exists.** `Auflage 1/10.1_ZI.sm`
/// computes `sqrt(2*g*r*sin(90°))` as `4.42945 kg/s` where SMath says `4.429
/// m/s`: the number is right, the dimension is nonsense, and nothing in the
/// output says so. Only the stored answer's dimension catches it.
///
/// The variable moves and the unit keeps the spelling, because the unit's is
/// fixed by the language and the variable's is not.
fn shadowed_units(w: &Worksheet) -> BTreeSet<String> {
    let mut bound = BTreeSet::new();
    let mut as_unit = BTreeSet::new();
    // A unit *declaration* — `VA : W` — is exempt. Its target is styled as a
    // unit and it becomes `unit VA = W`, so the name is a unit on both sides and
    // there is no collision to break. Only a target that is a plain name, or a
    // signature's parameters, binds a value.
    let record = |target: &Expr, out: &mut BTreeSet<String>| match target {
        Expr::Name(n) => {
            out.insert(n.clone());
        }
        // A signature binds its parameters, and a parameter named `m` hides the
        // metre through the whole body just as a document-level binding does.
        // `mat(a, b, 2, 1) : v` unpacks into names the same way; both put every
        // name they bind in argument position, and either may be unit-styled.
        Expr::Call { args, .. } => {
            for a in args {
                if let Expr::Name(n) | Expr::Unit(n) = a {
                    out.insert(n.clone());
                }
            }
        }
        _ => {}
    };
    for m in w.math() {
        if let Statement::Define { target, .. } = &m.statement {
            record(target, &mut bound);
        }
        m.statement.walk(&mut |e| match e {
            Expr::Op { glyph, args } if glyph == ":" || glyph == "←" => {
                if let Some(target) = args.first() {
                    record(target, &mut bound);
                }
            }
            Expr::Call { name, args } if introduces_a_variable(name) && args.len() >= 2 => {
                record(&args[1], &mut bound);
            }
            _ => {}
        });
        // `resolve::units` has already decided which styled operands are really
        // units, so an `Expr::Unit` surviving here is one the engine or the
        // document knows. Results and contracts count: SMath answers in units
        // the input side may never write.
        m.statement.walk(&mut |e| {
            if let Expr::Unit(n) = e {
                as_unit.insert(n.clone());
            }
        });
        for e in [&m.result, &m.contract].into_iter().flatten() {
            e.walk(&mut |e| {
                if let Expr::Unit(n) = e {
                    as_unit.insert(n.clone());
                }
            });
        }
    }
    for r in w.flat() {
        if let Payload::Plot { expr, .. } = &r.payload {
            expr.walk(&mut |e| {
                if let Expr::Unit(n) = e {
                    as_unit.insert(n.clone());
                }
            });
        }
    }
    bound.intersection(&as_unit).cloned().collect()
}

/// Every name the worksheet binds, by any means and anywhere on the page.
///
/// **Nomo has no free symbols.** SMath does: a region set to symbolic
/// optimization is evaluated by its CAS, so a name nothing defines stays a
/// symbol there instead of raising the error a numeric region would. A
/// worksheet can therefore carry a formula written for a *reader* — the generic
/// form of a result, with the values substituted a few lines further down — and
/// still save with no `error` attribute anywhere. The interlock worksheet does
/// exactly that:
///
/// ```text
/// Vout : (R851 + R850)*R840/((R840 + R841)*R850)*V2 - R851/R850*V1
/// ```
///
/// `V1` and `V2` occur once each in the whole file, here, bound nowhere, and
/// four lines later the same `Vout` is reassigned with every resistor and
/// voltage written out as a literal. Emitting the first line as Nomo source
/// produces a worksheet whose engine reports two undefined names — a *silent*
/// import failure, since the marker rule (design note §8.7 item 23) is what is
/// supposed to make a gap visible, and there was no marker.
///
/// # Collected across the whole document, not up to the point of use
///
/// Only a name with no definition *anywhere* counts as free. A name defined
/// below its use is a scope question — SMath's `≡` is visible above itself and
/// Nomo's `global` is narrower, which the emitter already reports as
/// [`NoteKind::ScopeFlattened`] — and conflating the two would file a working
/// worksheet under the wrong complaint.
#[derive(Debug, Default)]
struct Bound(BTreeSet<String>);

impl Bound {
    fn build(w: &Worksheet) -> Bound {
        let mut out = BTreeSet::new();
        for m in w.math() {
            if let Statement::Define { target, .. } = &m.statement {
                bind(target, &mut out);
            }
            m.statement.walk(&mut |e| match e {
                // A binding operator *inside* an expression rather than at a
                // region root, which is how SMath writes a block:
                // `line(x : 1, y : x + 1, y)`. `classify` lifts the one at the
                // root and these stay ordinary nodes, so a reader that only
                // looked at statements would call every block-local name free.
                Expr::Op { glyph, args } if glyph == ":" || glyph == "←" => {
                    if let Some(target) = args.first() {
                        bind(target, &mut out);
                    }
                }
                // `int(f(x), x, a, b)`, `sum(f(i), i, 1, n)`: the second operand
                // is not a value being passed but a variable being introduced
                // for the first one. The emitter already relies on this for
                // `int`, where the integrand is lifted into a named function.
                Expr::Call { name, args } if introduces_a_variable(name) && args.len() >= 2 => {
                    bind(&args[1], &mut out);
                }
                _ => {}
            });
        }
        Bound(out)
    }

    /// The names this statement uses that nothing defines, in sorted order.
    fn free_in(&self, s: &Statement) -> Vec<String> {
        let mut out = BTreeSet::new();
        s.walk(&mut |e| {
            if let Expr::Name(n) = e {
                // `∞` is spelled `inf` on the way out and is a constant there,
                // so it is bound even though nothing in the document binds it.
                if n != "∞" && !self.0.contains(n) && !nomo_core::eval::is_constant(n) {
                    out.insert(n.clone());
                }
            }
        });
        out.into_iter().collect()
    }
}

/// The names this region gives a *document-level* value to.
///
/// Narrower than [`bind`], which is deliberately conservative because deciding
/// "free symbol" errs towards calling a name bound. Here the question is which
/// names a later region can read, so the two kinds of name `bind` folds in are
/// left out: a function's parameters, which live only in its body, and the
/// index a `for` or a `sum` introduces, which lives only in the region.
///
/// `el(A, i, j) : x` writes into `A`, and `mat(a, b, 2, 1) : v` unpacks into
/// its argument names — both bind what they write to rather than the call.
fn values_bound_by(s: &Statement) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut record = |target: &Expr| match target {
        Expr::Name(n) | Expr::Unit(n) => {
            out.insert(n.clone());
        }
        Expr::Call { name, args } => match name.as_str() {
            "el" => {
                if let Some(Expr::Name(base)) = args.first() {
                    out.insert(base.clone());
                }
            }
            "mat" => {
                for a in args {
                    if let Expr::Name(n) = a {
                        out.insert(n.clone());
                    }
                }
            }
            // A function definition. Its parameters are not values anyone else
            // can read.
            _ => {
                out.insert(name.clone());
            }
        },
        _ => {}
    };
    if let Statement::Define { target, .. } = s {
        record(target);
    }
    s.walk(&mut |e| {
        if let Expr::Op { glyph, args } = e {
            if glyph == ":" || glyph == "←" {
                if let Some(target) = args.first() {
                    record(target);
                }
            }
        }
    });
    out
}

/// The functions the worksheet defines for itself — `f(x) : x^2` binds `f`.
///
/// [`Bound`] already collects these, but it cannot answer this question: it
/// records `f` and `x` together, because for deciding which symbols are free
/// a parameter and a function name are the same thing. Here they are not, and a
/// call to `x` would be nonsense.
///
/// `mat` and `el` are excluded because neither is a definition. `el(A, i) : x`
/// writes into a matrix and `mat(a, b, 2, 1) : v` unpacks a vector into names;
/// both are handled where they are emitted, and both would otherwise register a
/// function this worksheet never defined.
fn defined_functions(w: &Worksheet) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut record = |target: &Expr| {
        if let Expr::Call { name, .. } = target {
            if name != "mat" && name != "el" {
                out.insert(name.clone());
            }
        }
    };
    for m in w.math() {
        if let Statement::Define { target, .. } = &m.statement {
            record(target);
        }
        // A definition inside a block rather than at a region root, the same
        // case [`Bound::build`] takes care over: `line(f(x) : x^2, f(2))`.
        m.statement.walk(&mut |e| {
            if let Expr::Op { glyph, args } = e {
                if glyph == ":" || glyph == "←" {
                    if let Some(target) = args.first() {
                        record(target);
                    }
                }
            }
        });
    }
    out
}

/// The names a plot draws that are really functions of `x`.
///
/// # What the worksheet is doing
///
/// SMath has no function-valued definition, so a worksheet that wants to name
/// a curve before drawing it writes `plot_Z_LLC_eq : Z_LLC_eq_abs(x)` — an
/// ordinary definition whose right-hand side is free in the plot variable.
/// SMath's CAS keeps the region symbolic and the `<plot>` below it evaluates
/// the name at each sample. Nomo has no free symbols, so read literally the
/// definition binds nothing, and both regions used to end as markers: three of
/// them in the converter worksheet alone, which is every chart it has.
///
/// Read as what it is, the line is `fn plot_Z_LLC_eq(x) = Z_LLC_eq_abs(x)`, and
/// that is exactly the shape `plot` wants.
///
/// # Three conditions, all of them the worksheet's own evidence
///
/// - The definition's body **reads `x`**. That is not a guess either: `x` is
///   SMath's 2D plot variable, the one [`Emitter::plot`] already keys on, and
///   every function plot in both corpora is written in it.
/// - A **plot region reads the name**. Without a plot there is nothing saying
///   the definition was meant to be a function, and inventing a parameter for
///   an ordinary broken line would hide the breakage.
/// - **Nothing else reads it.** A name used as a value somewhere else is not a
///   function, and turning it into one would break the line that reads it.
///   `7.4.sm` in the mechanics corpus is why this is a condition rather than an
///   assumption: it defines `P` twice as a `sys(…)` of parametric curves, and
///   the second reads the first.
///
/// The fourth condition — that `x` is the only thing the body is waiting for —
/// is [`Emitter::curve_of_x`], because only emission knows it.
fn curves_of_x(w: &Worksheet) -> BTreeSet<String> {
    let mut plotted = BTreeSet::new();
    for r in w.flat() {
        if let Payload::Plot { expr, .. } = &r.payload {
            expr.walk(&mut |e| {
                if let Expr::Name(n) = e {
                    plotted.insert(n.clone());
                }
            });
        }
    }
    if plotted.is_empty() {
        return BTreeSet::new();
    }
    let mut candidates = BTreeSet::new();
    let mut read = BTreeSet::new();
    for m in w.math() {
        let defined = match &m.statement {
            Statement::Define {
                target: Expr::Name(name),
                value,
                ..
            } if plotted.contains(name) => {
                let mut reads_x = false;
                value.walk(&mut |e| reads_x |= matches!(e, Expr::Name(n) if n == "x"));
                reads_x.then(|| name.clone())
            }
            _ => None,
        };
        // Everything this region reads, which is the whole statement less the
        // name it is itself binding.
        m.statement.walk(&mut |e| {
            if let Expr::Name(n) = e {
                if Some(n) != defined.as_ref() {
                    read.insert(n.clone());
                }
            }
        });
        candidates.extend(defined);
    }
    candidates.retain(|n| !read.contains(n));
    candidates
}

/// The series of a `sys(s1, …, sn, n, 1)`: SMath's several-curve plot.
///
/// The last two operands are the shape, the same `n`-plus-two arity `mat` and
/// `line` are written with, and they are checked rather than assumed — a shape
/// that is not `n` by 1 is not a list of series, and reading it as one would
/// drop or invent a curve. Confirmed against `PlotRegion.dll` (design note
/// §8.21), which is where the operand order was read rather than guessed.
///
/// `None` for anything that is not a list of series.
fn series_of(e: &Expr) -> Option<Vec<&Expr>> {
    let Expr::Call { name, args } = e else {
        return None;
    };
    if name != "sys" || args.len() < 3 {
        return None;
    }
    let (listed, shape) = args.split_at(args.len() - 2);
    match (&shape[0], &shape[1]) {
        (Expr::Number(rows), Expr::Number(cols))
            if rows.parse() == Ok(listed.len()) && cols.parse() == Ok(1usize) =>
        {
            Some(listed.iter().collect())
        }
        _ => None,
    }
}

/// Apply every mention of a curve to `x`, so that a plot of `plot_Z/ohm` is a
/// plot of `plot_Z(x)/ohm`.
fn applied_to_x(e: &Expr, curves: &BTreeSet<String>) -> Expr {
    match e {
        Expr::Name(n) if curves.contains(n) => Expr::Call {
            name: n.clone(),
            args: vec![Expr::Name(String::from("x"))],
        },
        Expr::Call { name, args } => Expr::Call {
            name: name.clone(),
            args: args.iter().map(|a| applied_to_x(a, curves)).collect(),
        },
        Expr::Op { glyph, args } => Expr::Op {
            glyph: glyph.clone(),
            args: args.iter().map(|a| applied_to_x(a, curves)).collect(),
        },
        other => other.clone(),
    }
}

/// Record whatever names an assignment target binds.
fn bind(target: &Expr, out: &mut BTreeSet<String>) {
    match target {
        Expr::Name(n) | Expr::Unit(n) => {
            out.insert(n.clone());
        }
        // `mat(a, b, c, 3, 1) : v` unpacks a vector into names, and
        // `f(x, y) : …` is a signature whose parameters are bound in its body.
        // Both put every name they bind in argument position.
        Expr::Call { name, args } => {
            out.insert(name.clone());
            for a in args {
                if let Expr::Name(n) | Expr::Unit(n) = a {
                    out.insert(n.clone());
                }
            }
        }
        _ => {}
    }
}

/// Whether this SMath function introduces a variable in its second argument.
///
/// Conservative on purpose: a binder left out of this list makes a working
/// region report as free-symbolled, which is the expensive direction of the
/// mistake, so it errs towards saying a name is bound.
fn introduces_a_variable(name: &str) -> bool {
    matches!(
        name,
        "int" | "sum" | "product" | "diff" | "solve" | "roots" | "for" | "while" | "bisection"
    )
}

/// The refusal [`Emitter::captures`] produces, worded for the construct.
fn capture_refused(what: &str, captured: &str) -> String {
    format!(
        "`{what}` inside a definition, over a body that reads the definition's parameter \
         `{captured}`: Nomo has no closures, so the body would be lifted above the definition \
         and read the worksheet's global `{captured}` instead"
    )
}

/// Rewrite an SMath name into a legal Nomo one, or `None` if it cannot be.
///
/// Nomo names hold letters, digits, `_`, `°` and `%`, and may not start with a
/// digit. SMath is looser in three ways that matter, ranked by how often they
/// occur across the corpus: `.` separates a subscript (250 names), `#` marks a
/// parameter or temporary (37), and `'` is prime notation (10).
fn respell(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            c if c.is_alphanumeric() || c == '_' || c == '°' || c == '%' => out.push(c),
            // A prime is part of the identity of a name — `f` and `f'` are two
            // different functions — so it becomes a suffix rather than a `_`
            // that would collide with `f_`.
            '\'' => out.push_str("_prime"),
            '.' | '#' => out.push('_'),
            _ => return None,
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    // `unit` and `fn` are keywords; `global` is too.
    if matches!(out.as_str(), "unit" | "fn" | "global") {
        out.push('_');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emit_src(xml: &str) -> Emitted {
        emit(&crate::read(xml.as_bytes()).unwrap())
    }

    fn wrap(regions: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><?application progid="SMath Studio" version="0.85"?>
<regions><settings><calculation><precision>2</precision></calculation></settings>{regions}</regions>"#
        )
    }

    fn math(tokens: &str) -> String {
        format!(r#"<region id="0" left="0" top="0"><math>{tokens}</math></region>"#)
    }

    fn operand(t: &str) -> String {
        format!(r#"<e type="operand">{t}</e>"#)
    }
    fn unit(t: &str) -> String {
        format!(r#"<e type="operand" style="unit">{t}</e>"#)
    }
    fn op(g: &str, n: usize) -> String {
        format!(r#"<e type="operator" args="{n}">{g}</e>"#)
    }
    fn call(f: &str, n: usize) -> String {
        format!(r#"<e type="function" args="{n}">{f}</e>"#)
    }

    /// A whole region from a list of token fragments.
    fn region(tokens: &[String]) -> String {
        math(&tokens.concat())
    }

    /// A newer-era region: the input, and the answer SMath stored beside it.
    fn answered(tokens: &str, result: &str) -> String {
        format!(r#"<region id="0" left="0" top="0"><math><input>{tokens}</input>"#)
            + &format!(r#"<result action="numeric">{result}</result></math></region>"#)
    }

    /// `name : 1`, to give a fixture's operands somewhere to come from.
    ///
    /// A one-region worksheet has no definitions in it, so every name it
    /// mentions is free and the emitter is right to refuse it. Tests about
    /// anything *else* have to say where their values come from, exactly as a
    /// real worksheet does.
    fn given(names: &[&str]) -> String {
        names
            .iter()
            .map(|n| region(&[operand(n), operand("1"), op(":", 2)]))
            .collect()
    }

    fn picture(b64: &str) -> String {
        format!(
            r#"<region id="0" left="0" top="0"><picture><raw format="png" encoding="base64">{b64}</raw></picture></region>"#
        )
    }

    /// The base64 of one trailer block, reassembled from its indented lines.
    fn block(source: &str, name: &str) -> String {
        let mut lines = source
            .lines()
            .skip_while(|l| !l.starts_with(&format!("' image {name} ")));
        lines.next().expect("no block for that name");
        lines
            .take_while(|l| l.starts_with("'   "))
            .map(|l| &l[4..])
            .collect()
    }

    #[test]
    fn a_picture_is_referenced_in_the_body_and_carried_in_the_trailer() {
        let e = emit_src(&wrap(&picture("SGVsbG8h")));
        assert!(e.source.contains("\n' image figure1\n"), "{}", e.source);
        assert!(e.source.contains("' --- resources ---"), "{}", e.source);
        // The size in the header is the image's, not its base64's.
        assert!(e.source.contains("' image figure1 png 6"), "{}", e.source);
        assert_eq!(block(&e.source, "figure1"), "SGVsbG8h");
    }

    #[test]
    fn a_picture_keeps_the_size_it_was_drawn_at() {
        // SMath's figures are almost all scaled: one worksheet's first is a
        // 1161 px PNG placed at 749. Carrying the pixels alone would show every
        // figure at whatever size it happened to be photographed.
        let e = emit_src(&wrap(
            r#"<region id="0" left="0" top="0" width="749" height="483"><picture><raw format="png" encoding="base64">SGVsbG8h</raw></picture></region>"#,
        ));
        assert!(
            e.source.contains("\n' image figure1 749x483\n"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_picture_prefers_the_size_the_image_declares_over_its_region() {
        // A region carries a few pixels of frame around its content — the one
        // `<imagefile>` in the mechanics corpus is 117x100 inside a 127x108
        // region — so where the picture states its own box, that is the figure.
        let e = emit_src(&wrap(
            r#"<region id="0" left="0" top="0" width="127" height="108"><image><imagefile format="png" width="117" height="100">SGVsbG8h</imagefile></image></region>"#,
        ));
        assert!(
            e.source.contains("\n' image figure1 117x100\n"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_picture_in_a_region_with_no_box_is_referenced_without_a_size() {
        // The oldest era's regions declare no width or height. Inventing one
        // would draw the figure at a size no file states.
        let e = emit_src(&wrap(&picture("SGVsbG8h")));
        assert!(e.source.contains("\n' image figure1\n"), "{}", e.source);
    }

    #[test]
    fn a_carried_image_is_not_counted_as_unsupported() {
        // The data survives the import, so counting it as untranslated would
        // say a worksheet had lost something it still has. It is still counted,
        // because nothing displays it yet.
        let e = emit_src(&wrap(&picture("SGVsbG8h")));
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
        assert!(e.notes.iter().any(|n| n.kind == NoteKind::Carried));
    }

    #[test]
    fn the_body_stays_readable_however_large_the_image() {
        // The whole point of the trailer. 116 KB of base64 where the figure
        // stood is what would cost the text format the property it was chosen
        // for, so no line above the trailer may be longer than an ordinary one.
        let big = "A".repeat(40_000);
        let e = emit_src(&wrap(&format!("{}{}", picture(&big), math(&operand("x")))));
        let body = e.source.split("' --- resources ---").next().unwrap();
        assert!(
            body.lines().all(|l| l.len() <= 80),
            "longest body line was {}",
            body.lines().map(str::len).max().unwrap_or(0)
        );
        assert_eq!(block(&e.source, "figure1"), big);
    }

    #[test]
    fn a_picture_with_no_data_is_reported_rather_than_carried() {
        let e = emit_src(&wrap(
            r#"<region id="0" left="0" top="0"><picture></picture></region>"#,
        ));
        assert!(e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
        assert!(!e.source.contains("' --- resources ---"), "{}", e.source);
    }

    #[test]
    fn a_page_header_is_kept_but_its_mathematics_is_not_run() {
        // A page header holding the date 04-04-2025 stores it as three
        // operands and two subtractions. Emitted into the body it is a query
        // evaluating to -2025, so it is shown instead.
        let xml = format!(
            r#"<?xml version="1.0"?><?application progid="SMath Solver" version="1.4"?>
<worksheet xmlns="http://smath.info/schemas/worksheet/1.0">
<regions type="content">{}</regions>
<regions type="header">{}</regions></worksheet>"#,
            math(&format!("{}{}{}", operand("a"), operand("1"), op(":", 2))),
            math(&format!(
                "{}{}{}{}{}",
                operand("04"),
                operand("04"),
                op("-", 2),
                operand("2025"),
                op("-", 2)
            ))
        );
        let e = emit_src(&xml);
        let body = e.source.split("' --- page header ---").next().unwrap();
        assert!(body.contains("a = 1"), "{}", e.source);
        // Present, and inside a comment, so nothing evaluates it.
        assert!(
            e.source
                .contains("' [import] unsupported: page header math: 04 - 04 - 2025"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_unit_is_attached_by_juxtaposition() {
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}",
            operand("Van"),
            operand("230"),
            unit("V"),
            op("*", 2),
            op(":", 2)
        ))));
        assert!(e.source.contains("Van = 230 V"), "{}", e.source);
    }

    #[test]
    fn precedence_is_restored_without_redundant_brackets() {
        // (a + b) * c  must keep its brackets; a + b*c must not gain any.
        let needs = emit_src(&wrap(&region(&[
            operand("x"),
            operand("a"),
            operand("b"),
            op("+", 2),
            operand("c"),
            op("*", 2),
            op(":", 2),
        ])));
        assert!(needs.source.contains("x = (a + b)*c"), "{}", needs.source);

        let does_not = emit_src(&wrap(&region(&[
            operand("x"),
            operand("a"),
            operand("b"),
            operand("c"),
            op("*", 2),
            op("+", 2),
            op(":", 2),
        ])));
        assert!(
            does_not.source.contains("x = a + b*c"),
            "{}",
            does_not.source
        );
    }

    #[test]
    fn subtraction_keeps_its_right_operand_bracketed() {
        // a - (b - c) is not a - b - c.
        let e = emit_src(&wrap(&region(&[
            operand("x"),
            operand("a"),
            operand("b"),
            operand("c"),
            op("-", 2),
            op("-", 2),
            op(":", 2),
        ])));
        assert!(e.source.contains("x = a - (b - c)"), "{}", e.source);
    }

    #[test]
    fn powers_stay_right_associative() {
        let e = emit_src(&wrap(&region(&[
            operand("x"),
            operand("2"),
            operand("3"),
            operand("2"),
            op("^", 2),
            op("^", 2),
            op(":", 2),
        ])));
        assert!(e.source.contains("x = 2^3^2"), "{}", e.source);
    }

    #[test]
    fn element_access_becomes_indexing() {
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}",
            operand("x"),
            operand("v"),
            operand("2"),
            call("el", 2),
            op(":", 2)
        ))));
        assert!(e.source.contains("x = v[2]"), "{}", e.source);
    }

    #[test]
    fn a_matrix_literal_is_laid_out_row_by_row() {
        // mat(1, 2, 3, 4, 5, 6, rows=2, cols=3) -> [[1, 2, 3], [4, 5, 6]]
        let mut toks = String::from(&operand("m"));
        for v in ["1", "2", "3", "4", "5", "6", "2", "3"] {
            toks.push_str(&operand(v));
        }
        toks.push_str(&call("mat", 8));
        toks.push_str(&op(":", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(
            e.source.contains("m = [[1, 2, 3], [4, 5, 6]]"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_single_column_becomes_a_vector() {
        let mut toks = String::from(&operand("v"));
        for v in ["7", "8", "2", "1"] {
            toks.push_str(&operand(v));
        }
        toks.push_str(&call("mat", 4));
        toks.push_str(&op(":", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(e.source.contains("v = [7, 8]"), "{}", e.source);
    }

    #[test]
    fn a_global_definition_keeps_its_scope() {
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}",
            operand("g"),
            operand("9.81"),
            op("≡", 2)
        ))));
        assert!(e.source.contains("global g = 9.81"), "{}", e.source);
    }

    #[test]
    fn a_unit_alias_becomes_a_unit_declaration() {
        // Both sides carry `style="unit"` in the file, which is what makes the
        // left one a declaration rather than an ordinary binding.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}",
            unit("VA"),
            unit("W"),
            op(":", 2)
        ))));
        assert!(e.source.contains("unit VA = W"), "{}", e.source);
    }

    #[test]
    fn a_unit_declared_with_a_magnitude_becomes_one_too() {
        // `a.0 : 1 m` — a length scale the worksheet then works in.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}",
            unit("a.0"),
            operand("1"),
            unit("m"),
            op("*", 2),
            op(":", 2)
        ))));
        assert!(e.source.contains("unit a_0 = 1 m"), "{}", e.source);
    }

    #[test]
    fn a_styled_symbol_that_is_no_unit_is_bound_as_a_variable() {
        // `A.x` is a reaction component the worksheet solves for. Trusting the
        // style would emit a unit that does not exist and lose the unknown.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}",
            unit("A.x"),
            operand("2"),
            op("*", 2),
        ))));
        assert!(!e.source.contains("unit "), "{}", e.source);
        assert!(e.source.contains("A_x*2"), "{}", e.source);
    }

    #[test]
    fn a_subscripted_name_is_respelled_and_the_change_is_recorded() {
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}",
            operand("A.total"),
            operand("5"),
            op(":", 2)
        ))));
        assert!(e.source.contains("A_total = 5"), "{}", e.source);
    }

    #[test]
    fn two_names_that_would_merge_are_refused_not_merged() {
        let mut toks = String::new();
        toks.push_str(&operand("a.b"));
        toks.push_str(&operand("1"));
        toks.push_str(&op(":", 2));
        let regions = format!(
            r#"<region id="0" left="0" top="0"><math>{toks}</math></region>
               <region id="1" left="0" top="10"><math>{}{}{}</math></region>"#,
            operand("a_b"),
            operand("2"),
            op(":", 2)
        );
        let e = emit_src(&wrap(&regions));
        assert!(
            e.notes.iter().any(|n| n.kind == NoteKind::Collision),
            "{:?}",
            e.notes
        );
    }

    #[test]
    fn an_untranslatable_construct_becomes_a_visible_marker() {
        // `—` is the one glyph still unidentified: six uses, always at a region
        // root with a function call on its left. It is deliberately *not*
        // implemented on a guess, so it is what an unsupported operator looks
        // like.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}",
            operand("x"),
            operand("a"),
            operand("b"),
            op("—", 2),
            op(":", 2)
        ))));
        assert!(e.source.contains("' [import] unsupported"), "{}", e.source);
        assert!(e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
    }

    #[test]
    fn a_vector_target_unpacks_into_one_binding_per_name() {
        // `mat(a, b, 2, 1) : v` is how a solver result is taken apart. Nomo has
        // no destructuring statement, so it becomes a temporary and two indexes.
        let e = emit_src(&wrap(&format!(
            "{}{}",
            given(&["v"]),
            math(&format!(
                "{}{}{}{}{}{}{}",
                operand("a"),
                operand("b"),
                operand("2"),
                operand("1"),
                call("mat", 4),
                operand("v"),
                op(":", 2)
            ))
        )));
        assert!(e.source.contains("a_all = v"), "{}", e.source);
        assert!(e.source.contains("a = a_all[1]"), "{}", e.source);
        assert!(e.source.contains("b = a_all[2]"), "{}", e.source);
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
    }

    #[test]
    fn writing_into_an_index_is_refused_by_name() {
        // 103 uses across the corpora, and no faithful translation: Nomo is a
        // set of definitions, so there is no statement that mutates `A`.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}{}",
            operand("A"),
            operand("1"),
            operand("2"),
            call("el", 3),
            operand("x"),
            op(":", 2)
        ))));
        assert!(e.source.contains("an assignment into `A`"), "{}", e.source);
    }

    #[test]
    fn which_language_is_kept_is_a_choice_and_it_is_recorded() {
        let regions = r#"<region id="0" left="0" top="0">
            <text lang="ger"><p>Masse</p></text>
            <text lang="eng"><p>Mass</p></text></region>"#;
        let w = crate::read(wrap(regions).as_bytes()).unwrap();

        // Asked for English, and got it.
        let e = crate::emit_in(&w, Some("eng"));
        assert!(e.source.contains("' Mass"), "{}", e.source);
        // A region without the requested language keeps its first rather than
        // losing its prose.
        let e_none = crate::emit_in(&w, Some("rus"));
        assert!(e_none.source.contains("' Masse"), "{}", e_none.source);
        // And what was dropped is said once, not per region.
        let dropped: Vec<_> = e
            .notes
            .iter()
            .filter(|n| n.detail.contains("translation(s) of prose dropped"))
            .collect();
        assert_eq!(dropped.len(), 1, "{:?}", e.notes);
    }

    #[test]
    fn a_loop_that_fills_a_vector_becomes_a_map() {
        // `for(i, range(1, 3), el(A, i) ← i*2)`. The body is lifted into a named
        // function because `map` takes a name, which is the same move `int`
        // already makes for an integrand.
        let mut toks = String::from(&operand("i"));
        toks.push_str(&operand("1"));
        toks.push_str(&operand("3"));
        toks.push_str(&call("range", 2));
        toks.push_str(&operand("A"));
        toks.push_str(&operand("i"));
        toks.push_str(&call("el", 2));
        toks.push_str(&operand("i"));
        toks.push_str(&operand("2"));
        toks.push_str(&op("*", 2));
        toks.push_str(&op("←", 2));
        toks.push_str(&call("for", 3));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(e.source.contains("fn A_at_1(i) = i*2"), "{}", e.source);
        assert!(
            e.source.contains("A = map(A_at_1, range(1, 3))"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_counted_loop_filling_two_columns_becomes_an_augment() {
        // `for(i ← 1, i < 4, i ← i + 1, line(el(M, i, 1) ← i, el(M, i, 2) ← i*i, 2, 1))`
        // — the shape `NASA_atmosphere.sm` builds its tables with, and the
        // reason its four plots now draw. `i < 4` is `range(1, 3)`: Nomo's
        // range includes both ends.
        let mut toks = String::from(&operand("i"));
        toks.push_str(&operand("1"));
        toks.push_str(&op("←", 2));
        toks.push_str(&operand("i"));
        toks.push_str(&operand("4"));
        // Escaped, as SMath writes it in the file.
        toks.push_str(&op("&lt;", 2));
        toks.push_str(&operand("i"));
        toks.push_str(&operand("i"));
        toks.push_str(&operand("1"));
        toks.push_str(&op("+", 2));
        toks.push_str(&op("←", 2));
        for (col, rhs) in [("1", vec!["i"]), ("2", vec!["i", "i"])] {
            toks.push_str(&operand("M"));
            toks.push_str(&operand("i"));
            toks.push_str(&operand(col));
            toks.push_str(&call("el", 3));
            for r in &rhs {
                toks.push_str(&operand(r));
            }
            if rhs.len() == 2 {
                toks.push_str(&op("*", 2));
            }
            toks.push_str(&op("←", 2));
        }
        toks.push_str(&operand("2"));
        toks.push_str(&operand("1"));
        toks.push_str(&call("line", 4));
        toks.push_str(&call("for", 4));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(
            e.source
                .contains("M = augment(map(M_col1_1, range(1, 3)), map(M_col2_2, range(1, 3)))"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_recurrence_is_not_a_map_and_says_so() {
        // `for(i, range(1, 3), el(b, i) ← el(b, i - 1) + 1)`: each element needs
        // the last one, so there is an order and `map` has none.
        let mut toks = String::from(&operand("i"));
        toks.push_str(&operand("1"));
        toks.push_str(&operand("3"));
        toks.push_str(&call("range", 2));
        toks.push_str(&operand("b"));
        toks.push_str(&operand("i"));
        toks.push_str(&call("el", 2));
        toks.push_str(&operand("b"));
        toks.push_str(&operand("i"));
        toks.push_str(&operand("1"));
        toks.push_str(&op("-", 2));
        toks.push_str(&call("el", 2));
        toks.push_str(&operand("1"));
        toks.push_str(&op("+", 2));
        toks.push_str(&op("←", 2));
        toks.push_str(&call("for", 3));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(!e.source.contains("map("), "{}", e.source);
        assert!(e.source.contains("depends on the last"), "{}", e.source);
    }

    #[test]
    fn a_while_loop_says_what_it_would_have_to_invent() {
        // Every `while` in both corpora is an iterative solver stopping on a
        // tolerance. `iterate` takes a count, and the count decides the answer.
        let mut toks = String::from(&operand("k"));
        toks.push_str(&operand("1"));
        toks.push_str(&op("&lt;", 2));
        toks.push_str(&operand("k"));
        toks.push_str(&operand("k"));
        toks.push_str(&operand("1"));
        toks.push_str(&op("+", 2));
        toks.push_str(&op("←", 2));
        toks.push_str(&call("while", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(e.source.contains("would have to"), "{}", e.source);
    }

    #[test]
    fn a_definition_that_shadows_a_builtin_and_calls_it_is_refused() {
        // `ln(Nu) : ln(Re) + 1`. In SMath the inner call is the logarithm; here
        // the definition would shadow it for its own body, so the same text
        // means a function that calls itself. Refusing leaves every other line's
        // `ln` meaning the logarithm, which is what the worksheet meant.
        let mut toks = String::from(&operand("Nu"));
        toks.push_str(&call("ln", 1));
        toks.push_str(&operand("Re"));
        toks.push_str(&call("ln", 1));
        toks.push_str(&operand("1"));
        toks.push_str(&op("+", 2));
        toks.push_str(&op(":", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(!e.source.contains("fn ln"), "{}", e.source);
        assert!(e.source.contains("the built-in"), "{}", e.source);
    }

    #[test]
    fn a_plotted_table_is_drawn() {
        // `XY : augment(x1, y1)` and then a `<plot>` of `XY`: a table of points,
        // which needs no span. The corpus has no worksheet where this survives
        // — every table in it is filled by a loop that mutates — so the case is
        // pinned here instead.
        let mut toks = String::from(&operand("XY"));
        for v in ["1", "2", "2", "1"] {
            toks.push_str(&operand(v));
        }
        toks.push_str(&call("mat", 4));
        for v in ["3", "4", "2", "1"] {
            toks.push_str(&operand(v));
        }
        toks.push_str(&call("mat", 4));
        toks.push_str(&call("augment", 2));
        toks.push_str(&op(":", 2));
        let plot = r#"<region id="1" left="0" top="40"><plot>
            <e type="operand">XY</e></plot></region>"#;
        let e = emit_src(&wrap(&format!("{}{plot}", math(&toks))));
        assert!(e.source.contains("plot(XY)"), "{}", e.source);
        // Drawn, but not as SMath drew it: the viewport is dropped and both
        // axes are fitted, which is counted rather than silent.
        assert!(
            e.notes.iter().any(|n| n.kind == NoteKind::Carried),
            "{:?}",
            e.notes
        );
    }

    #[test]
    fn a_plotted_table_with_no_value_keeps_its_marker() {
        // The corpus's own case, in miniature: `M` is filled element by element
        // inside a block, so the name is bound and the numbers never arrive.
        // Emitting `plot(M)` would put a failing line in the output where a
        // marker belongs.
        let mut toks = String::from(&operand("M"));
        for v in ["i", "1"] {
            toks.push_str(&operand(v));
        }
        toks.push_str(&call("el", 3));
        toks.push_str(&operand("7"));
        toks.push_str(&op(":", 2));
        let plot = r#"<region id="1" left="0" top="40"><plot>
            <e type="operand">M</e></plot></region>"#;
        let e = emit_src(&wrap(&format!("{}{plot}", math(&toks))));
        assert!(!e.source.contains("plot(M)"), "{}", e.source);
        assert!(e.source.contains("`M` has no value here"), "{}", e.source);
    }

    #[test]
    fn a_plot_of_a_function_of_x_is_drawn_over_the_span_its_viewport_implies() {
        // A 350x233 region at the untouched view: the frame is
        // 10·(350/233)/1.66 = 9.049 pixels per unit, so the visible x is
        // ±175/9.049 = ±19.34. Derived in design note §8.21 by reading
        // `PlotRegion.dll`, and checked there against six worksheets.
        let plot = r#"<region id="0" left="0" top="0" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">
            <e type="operand">x</e><e type="function" args="1">sin</e></plot></region>"#;
        let e = emit_src(&wrap(plot));
        assert!(e.source.contains("fn curve_1(x) = sin(x)"), "{}", e.source);
        assert!(
            e.source.contains("plot(curve_1, -19.339, 19.339)"),
            "{}",
            e.source
        );
        // Drawn, but not the way SMath drew it — the viewport is read, not
        // carried — so it is counted rather than silent.
        assert!(
            e.notes.iter().any(|n| n.kind == NoteKind::Carried),
            "{:?}",
            e.notes
        );
    }

    #[test]
    fn a_panned_and_zoomed_view_moves_the_span() {
        // `transpose_x` shifts both ends and `scale_y` narrows them, which is
        // what makes six corpus worksheets come out over sensible domains.
        let plot = r#"<region id="0" left="0" top="0" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="2"
                  transpose_x="-100" transpose_y="0">
            <e type="operand">x</e><e type="function" args="1">sin</e></plot></region>"#;
        let e = emit_src(&wrap(plot));
        // frame = 9.0491·2 = 18.0981; centre = 100/18.0981 = 5.5254;
        // half = 175/18.0981 = 9.6695.
        assert!(
            e.source.contains("plot(curve_1, -4.14407, 15.1949)"),
            "{}",
            e.source
        );
    }

    #[test]
    fn an_xyplot_of_a_function_has_no_span_to_read() {
        // The third-party region keeps its own axes and is not this model, so
        // it still reports rather than guessing.
        let plot = r#"<region id="0" left="0" top="0" width="350" height="233"><xyplot>
            <input><e type="operand">x</e><e type="function" args="1">sin</e></input>
            </xyplot></region>"#;
        let e = emit_src(&wrap(plot));
        assert!(e.source.contains("records no span"), "{}", e.source);
    }

    #[test]
    fn several_series_are_the_operands_of_sys() {
        // `sys(s1, s2, 2, 1)` — the last two operands are the shape, the same
        // n-plus-two arity `mat` is written with. Reading them as series would
        // have reported two curves as four.
        let mut toks = String::new();
        for v in ["1", "2", "2", "1"] {
            toks.push_str(&operand(v));
        }
        toks.push_str(&call("mat", 4));
        let one = toks.clone();
        let mut plotted = format!("{one}{one}");
        plotted.push_str(&operand("2"));
        plotted.push_str(&operand("1"));
        plotted.push_str(&call("sys", 4));
        let plot = format!(r#"<region id="0" left="0" top="0"><plot>{plotted}</plot></region>"#);
        let e = emit_src(&wrap(&plot));
        assert!(
            e.source.contains("plot([[1, 2]], [[1, 2]])")
                || e.source.contains("plot([1, 2], [1, 2])"),
            "{}",
            e.source
        );
    }

    /// `p : sin(x)`, and a `<plot>` of `p` at the untouched 350×233 view.
    fn curve_and_plot(plotted: &str, extra: &str) -> Emitted {
        let mut toks = String::from(&operand("p"));
        toks.push_str(&operand("x"));
        toks.push_str(&call("sin", 1));
        toks.push_str(&op(":", 2));
        let plot = format!(
            r#"<region id="1" left="0" top="40" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">{plotted}</plot></region>"#
        );
        emit_src(&wrap(&format!("{}{extra}{plot}", math(&toks))))
    }

    #[test]
    fn a_definition_free_in_x_that_a_plot_draws_is_a_function_of_x() {
        // SMath has no function-valued definition, so `plot_Z : Z_abs(x)` is
        // how a worksheet names a curve. Read literally the line binds nothing
        // and the plot below it has nothing to draw; read as what it is, it is
        // `fn`. See [`curves_of_x`].
        let e = curve_and_plot(&operand("p"), "");
        assert!(e.source.contains("fn p(x) = sin(x)"), "{}", e.source);
        // `plot` takes the name of a function and this plot draws exactly one,
        // so the worksheet's own name reaches the chart rather than `curve_1`.
        assert!(
            e.source.contains("plot(p, -19.339, 19.339)"),
            "{}",
            e.source
        );
        assert!(!e.source.contains("defined nowhere"), "{}", e.source);
    }

    #[test]
    fn a_plot_of_an_expression_applies_the_curve_to_x() {
        // The converter worksheet draws `plot_Z_LLC_eq/ohm`. The name is a
        // function now, so the plotted expression is `p(x)/ohm` and takes the
        // ordinary path for
        // a function of `x`: lifted into a curve, over the span the viewport
        // implies.
        let e = curve_and_plot(
            &format!("{}{}{}", operand("p"), unit("ohm"), op("/", 2)),
            "",
        );
        assert!(e.source.contains("fn p(x) = sin(x)"), "{}", e.source);
        assert!(
            e.source.contains("fn curve_1(x) = p(x)/ohm"),
            "{}",
            e.source
        );
        assert!(
            e.source.contains("plot(curve_1, -19.339, 19.339)"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_definition_free_in_x_that_nothing_plots_is_still_a_free_symbol() {
        // The plot is the whole evidence that the definition was meant as a
        // function. Without one there is nothing to read it as, and inventing a
        // parameter would hide an ordinary broken line.
        let mut toks = String::from(&operand("p"));
        toks.push_str(&operand("x"));
        toks.push_str(&call("sin", 1));
        toks.push_str(&op(":", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(!e.source.contains("fn p("), "{}", e.source);
        assert!(
            e.source.contains("`x` is used here but defined nowhere"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_name_something_else_reads_is_a_value_and_not_a_curve() {
        // The mechanics corpus's `7.4.sm` defines `P` twice as a `sys(…)` of
        // parametric curves and the second reads the first. A name read as a
        // value somewhere else is not a function, and turning it into one would
        // break the line that reads it.
        let mut reader = String::from(&operand("q"));
        reader.push_str(&operand("p"));
        reader.push_str(&op(":", 2));
        let e = curve_and_plot(
            &operand("p"),
            &format!(r#"<region id="2" left="0" top="20"><math>{reader}</math></region>"#),
        );
        assert!(!e.source.contains("fn p("), "{}", e.source);
    }

    #[test]
    fn a_definition_waiting_on_more_than_x_is_not_a_curve() {
        // `x` has to be the *only* thing the body is waiting for. `M` here is
        // bound by an assignment into an element, which does not import, so the
        // definition is a broken line rather than a curve — and the marker it
        // already gets is the right one.
        let mut fill = String::from(&operand("M"));
        for v in ["i", "1"] {
            fill.push_str(&operand(v));
        }
        fill.push_str(&call("el", 3));
        fill.push_str(&operand("7"));
        fill.push_str(&op(":", 2));
        let mut toks = String::from(&operand("p"));
        toks.push_str(&operand("M"));
        toks.push_str(&operand("x"));
        toks.push_str(&op("*", 2));
        toks.push_str(&op(":", 2));
        let plot = r#"<region id="2" left="0" top="40" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">
            <e type="operand">p</e></plot></region>"#;
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math>{fill}</math></region>
               <region id="1" left="0" top="20"><math>{toks}</math></region>{plot}"#
        )));
        assert!(!e.source.contains("fn p("), "{}", e.source);
    }

    /// `f(x) : sin(x)` and `g(x) : cos(x)`, so that a plot has two functions of
    /// the worksheet's own to draw.
    fn two_functions() -> String {
        let mut out = String::new();
        for (name, builtin) in [("f", "sin"), ("g", "cos")] {
            // The target of a function definition is a call shape: `x`, then
            // the name, then the binding operator.
            let mut toks = String::from(&operand("x"));
            toks.push_str(&call(name, 1));
            toks.push_str(&operand("x"));
            toks.push_str(&call(builtin, 1));
            toks.push_str(&op(":", 2));
            out.push_str(&format!(
                r#"<region id="0" left="0" top="0"><math>{toks}</math></region>"#
            ));
        }
        out
    }

    /// `sys(f(x), g(x), 2, 1)` as a token stream.
    fn two_series() -> String {
        let mut out = String::new();
        for name in ["f", "g"] {
            out.push_str(&operand("x"));
            out.push_str(&call(name, 1));
        }
        out.push_str(&operand("2"));
        out.push_str(&operand("1"));
        out.push_str(&call("sys", 4));
        out
    }

    #[test]
    fn a_first_derivative_becomes_a_slope_at_a_point() {
        // `derivative_Mg(f) : diff(Mg(f), f)` — the converter worksheet's line,
        // and the whole reason the engine grew a derivative. The differentiand
        // is already the
        // worksheet's own function at the variable, so it is named rather than
        // lifted into `slope_1`.
        let mut toks = String::from(&operand("f"));
        toks.push_str(&call("g", 1));
        toks.push_str(&operand("f"));
        toks.push_str(&call("g", 1));
        toks.push_str(&operand("f"));
        toks.push_str(&call("diff", 2));
        toks.push_str(&op(":", 2));
        // `g(f) : sin(f)` so that `g` is a function this worksheet defines.
        let mut define_g = String::from(&operand("f"));
        define_g.push_str(&call("g", 1));
        define_g.push_str(&operand("f"));
        define_g.push_str(&call("sin", 1));
        define_g.push_str(&op(":", 2));
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math>{define_g}</math></region>
               <region id="1" left="0" top="20"><math>{toks}</math></region>"#
        )));
        assert!(
            e.source.contains("fn g_(f) = derivative(g, f)")
                || e.source.contains("derivative(g, f)"),
            "{}",
            e.source
        );
        assert!(!e.source.contains("slope_1"), "{}", e.source);
    }

    #[test]
    fn a_differentiand_that_does_not_read_the_variable_is_refused() {
        // `9.3.sm` writes `diff(y.B, t)` where `y.B` is a name the worksheet
        // already bound. SMath differentiates the formula that name stands for;
        // Nomo would differentiate the number it evaluated to and answer zero,
        // which is a wrong answer wearing the shape of a right one.
        let mut toks = String::from(&operand("v"));
        toks.push_str(&operand("y"));
        toks.push_str(&operand("t"));
        toks.push_str(&call("diff", 2));
        toks.push_str(&op(":", 2));
        let mut bind = String::from(&operand("y"));
        bind.push_str(&operand("2"));
        bind.push_str(&op(":", 2));
        let mut bind_t = String::from(&operand("t"));
        bind_t.push_str(&operand("1"));
        bind_t.push_str(&op(":", 2));
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math>{bind}</math></region>
               <region id="1" left="0" top="10"><math>{bind_t}</math></region>
               <region id="2" left="0" top="20"><math>{toks}</math></region>"#
        )));
        assert!(!e.source.contains("derivative("), "{}", e.source);
        assert!(e.source.contains("does not read `t`"), "{}", e.source);
    }

    #[test]
    fn a_second_derivative_carries_its_order() {
        // `diff(g(t), t, 2)` — the accelerations of the mechanics corpus, and
        // `normaldist.sm`'s inflection points.
        let mut define_g = String::from(&operand("t"));
        define_g.push_str(&call("g", 1));
        define_g.push_str(&operand("t"));
        define_g.push_str(&call("sin", 1));
        define_g.push_str(&op(":", 2));
        let mut toks = String::from(&operand("t"));
        toks.push_str(&call("g", 1));
        toks.push_str(&operand("t"));
        toks.push_str(&operand("2"));
        toks.push_str(&call("diff", 3));
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math>{define_g}</math></region>
               <region id="1" left="0" top="20"><math>{toks}</math></region>"#
        )));
        assert!(e.source.contains("derivative(g, t, 2)"), "{}", e.source);
    }

    #[test]
    fn an_order_this_engine_does_not_reach_is_refused() {
        // A third derivative, and an order the worksheet computes: both are
        // refused rather than rounded down to what is available.
        let mut toks = String::from(&operand("t"));
        toks.push_str(&call("g", 1));
        toks.push_str(&operand("t"));
        toks.push_str(&operand("3"));
        toks.push_str(&call("diff", 3));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(e.source.contains("of order 3"), "{}", e.source);
        let mut computed = String::from(&operand("t"));
        computed.push_str(&call("g", 1));
        computed.push_str(&operand("t"));
        computed.push_str(&operand("n"));
        computed.push_str(&call("diff", 3));
        let e = emit_src(&wrap(&math(&computed)));
        assert!(e.source.contains("computed order"), "{}", e.source);
    }

    #[test]
    fn a_range_solve_becomes_a_scan() {
        // `solve(f(x), x, 0, 2)`. §8.24 read the algorithm out of
        // `SpecialFunctions.dll`: 200 samples across the range and every sign
        // change refined, which is `roots` in Nomo. The expression is lifted
        // into a function the way an integrand is.
        let mut toks = String::from(&operand("x"));
        toks.push_str(&call("sin", 1));
        toks.push_str(&operand("x"));
        toks.push_str(&operand("0"));
        toks.push_str(&operand("2"));
        toks.push_str(&call("solve", 4));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(e.source.contains("fn zero_1(x) = sin(x)"), "{}", e.source);
        assert!(e.source.contains("roots(zero_1, 0, 2)"), "{}", e.source);
    }

    #[test]
    fn two_columns_multiplied_are_an_inner_product() {
        // `b : mat(2, 3, 2, 1)`, `h : mat(4, 5, 2, 1)`, then `b·h`.
        // `TMatrix::op_Multiply` in `SMath.Math.Numeric.dll` tests for two
        // one-column operands of equal height *before* anything else and
        // returns their inner product as a scalar. Nomo's `*` is element-wise
        // between vectors, on purpose, so the translation is `dot`.
        let define = |name: &str, a: &str, b: &str| {
            region(&[
                operand(name),
                operand(a),
                operand(b),
                operand("2"),
                operand("1"),
                call("mat", 4),
                op(":", 2),
            ])
        };
        let product = region(&[operand("b"), operand("h"), op("*", 2)]);
        let e = emit_src(&wrap(
            &(define("b", "2", "3") + &define("h", "4", "5") + &product),
        ));
        assert!(e.source.contains("dot(b, h)"), "{}", e.source);

        // A shape the file does not state leaves `*` alone: `q` is bound to a
        // call, so nothing here knows it is a column.
        let unknown = region(&[operand("q"), operand("h"), op("*", 2)]);
        let e2 = emit_src(&wrap(&(define("h", "4", "5") + &unknown)));
        assert!(e2.source.contains("q*h"), "{}", e2.source);
    }

    #[test]
    fn a_summation_becomes_a_map_over_a_range() {
        // `sum(el(v, k), k, 1, 3)`. SMath's summation carries its own index
        // variable, which is `int`'s shape and lifts the same way — and the
        // fold it needs already exists, so no builtin was added for it.
        let mut toks = String::from(&operand("v"));
        toks.push_str(&operand("k"));
        toks.push_str(&call("el", 2));
        toks.push_str(&operand("k"));
        toks.push_str(&operand("1"));
        toks.push_str(&operand("3"));
        toks.push_str(&call("sum", 4));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(e.source.contains("fn term_1(k) = v[k]"), "{}", e.source);
        assert!(
            e.source.contains("sum(map(term_1, range(1, 3)))"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_summation_that_would_capture_a_parameter_is_refused() {
        // `f(a) : sum(a*k, k, 1, 3)`. The summand has to be lifted *above* the
        // definition it came out of, where `a` is the worksheet's global rather
        // than the parameter — so the only faithful answer is to say so.
        // `simpsonrichardson.sm` is the real one: it is right there by
        // coincidence, because the call passes the globals of the same name.
        let mut toks = String::from(&operand("a"));
        toks.push_str(&call("f", 1));
        toks.push_str(&operand("a"));
        toks.push_str(&operand("k"));
        toks.push_str(&op("*", 2));
        toks.push_str(&operand("k"));
        toks.push_str(&operand("1"));
        toks.push_str(&operand("3"));
        toks.push_str(&call("sum", 4));
        toks.push_str(&op(":", 2));
        // A second region calling it, because a name only ever *defined* has
        // nowhere for the emitter to have learned its Nomo spelling.
        let mut use_it = String::from(&operand("2"));
        use_it.push_str(&call("f", 1));
        let e = emit_src(&wrap(&(math(&toks) + &math(&use_it))));
        assert!(
            e.source.contains("reads the definition's parameter `a`"),
            "{}",
            e.source
        );
        assert!(!e.source.contains("fn term_1"), "{}", e.source);
    }

    #[test]
    fn an_equation_solved_is_its_difference() {
        // `solve(f(x) ≡ 5, x, 0, 2)`. SMath rewrites the last term from the
        // equality to a minus before it searches; this is the same rewrite,
        // which is why it is a translation rather than a reading.
        let mut toks = String::from(&operand("x"));
        toks.push_str(&call("sin", 1));
        toks.push_str(&operand("5"));
        toks.push_str(&op("≡", 2));
        toks.push_str(&operand("x"));
        toks.push_str(&operand("0"));
        toks.push_str(&operand("2"));
        toks.push_str(&call("solve", 4));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(
            e.source.contains("fn zero_1(x) = sin(x) - 5"),
            "{}",
            e.source
        );
        assert!(e.source.contains("roots(zero_1, 0, 2)"), "{}", e.source);
    }

    #[test]
    fn a_solve_with_no_range_is_refused() {
        // The two ends are `SolveFromPoint` and `SolveToPoint`, program options
        // rather than anything the file records — and the range decides which
        // roots are found, so inventing one would invent the answer.
        let mut toks = String::from(&operand("x"));
        toks.push_str(&call("sin", 1));
        toks.push_str(&operand("x"));
        toks.push_str(&call("solve", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(!e.source.contains("roots("), "{}", e.source);
        assert!(
            e.source
                .contains("two program options the worksheet does not record"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_worksheets_own_definition_wins_over_the_refusals() {
        // SMath resolves a worksheet's own function before the built-in
        // registry, and these refusals sit in front of that lookup. `Finite
        // differences.sm` writes `diff(y, x) ≡ 0`, which registers `diff` as a
        // definition, and every `diff` in its documentation equations renders
        // because of it — an unguarded refusal took that away.
        let mut definition = String::from(&operand("y"));
        definition.push_str(&call("diff", 1));
        definition.push_str(&operand("0"));
        definition.push_str(&op(":", 2));
        let mut use_it = String::from(&operand("2"));
        use_it.push_str(&call("diff", 1));
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math>{definition}</math></region>
               <region id="1" left="0" top="20"><math>{use_it}</math></region>"#
        )));
        assert!(e.source.contains("diff(2)"), "{}", e.source);
        // Not turned into `derivative(…)`, which is what the arm in front of
        // the lookup would have done to it.
        assert!(!e.source.contains("derivative("), "{}", e.source);
    }

    #[test]
    fn a_solve_from_a_starting_guess_is_refused_with_its_reason() {
        // `FindRoot(Q(x), x ≡ L)`. Which root it lands on is the method's
        // choice: `5.1.sm` starts at `L` and gets 1.08 m, and at `L/2` — nearer
        // the other root — gets -3.08 m. Nomo has a bracket and a window scan
        // and is given neither here.
        let mut toks = String::from(&operand("x"));
        toks.push_str(&call("Q", 1));
        toks.push_str(&operand("x"));
        toks.push_str(&operand("2"));
        toks.push_str(&op("≡", 2));
        toks.push_str(&call("FindRoot", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(
            e.source
                .contains("the method's choice, not the worksheet's"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_system_solve_says_what_it_is_missing() {
        // `roots(GGB, R)` solves a system, which `solve_linear` could do — but
        // the unknowns arrive as a name holding free symbols, so nothing in the
        // call says what they are or what dimension they carry.
        let mut toks = String::from(&operand("GGB"));
        toks.push_str(&operand("R"));
        toks.push_str(&call("roots", 2));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(
            e.source
                .contains("free symbols the file does not give a dimension"),
            "{}",
            e.source
        );
    }

    #[test]
    fn smaths_roots_is_not_nomos_roots() {
        // `roots(Q(x), x, -1)` in SMath searches from a guess; `roots(f, a, b)`
        // in Nomo scans a window. The operands even line up, which is what
        // makes the collision dangerous: read as Nomo's, the guess becomes one
        // end of a range and the variable becomes a function name. Adding
        // `roots` to the language started translating 8 corpus regions this way
        // with nothing failing, which is why the refusal is by name.
        let mut toks = String::from(&operand("x"));
        toks.push_str(&call("Q", 1));
        toks.push_str(&operand("x"));
        toks.push_str(&operand("1"));
        toks.push_str(&op("-", 1));
        toks.push_str(&call("roots", 3));
        let e = emit_src(&wrap(&math(&toks)));
        assert!(!e.source.contains("roots(Q"), "{}", e.source);
        assert!(
            e.source.contains("searches from a starting guess"),
            "{}",
            e.source
        );
    }

    #[test]
    fn several_functions_of_x_share_one_span() {
        // `sys(…)` in the plot itself: two curves, one span, and neither is
        // lifted because each series is already a call to one of the
        // worksheet's own functions at `x`.
        let plot = format!(
            r#"<region id="1" left="0" top="40" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">{}</plot></region>"#,
            two_series()
        );
        let e = emit_src(&wrap(&format!("{}{plot}", two_functions())));
        assert!(
            e.source.contains("plot(f, g, -19.339, 19.339)"),
            "{}",
            e.source
        );
        assert!(!e.source.contains("fn curve_"), "{}", e.source);
    }

    #[test]
    fn a_definition_of_several_series_is_drawn_by_the_plot_that_reads_it() {
        // A converter worksheet's third chart: SMath has nowhere to write a
        // list of curves except a definition, so
        // `Multipleplots : sys(Mg(x), Mg2(x), 2, 1)` is
        // how it says "these two, together". Nomo has no value for a list of
        // series and needs none — the plot below is where the list means
        // something, and that is where it goes.
        let mut definition = String::from(&operand("M"));
        definition.push_str(&two_series());
        definition.push_str(&op(":", 2));
        let plot = r#"<region id="2" left="0" top="60" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">
            <e type="operand">M</e></plot></region>"#;
        let e = emit_src(&wrap(&format!(
            r#"{}<region id="1" left="0" top="40"><math>{definition}</math></region>{plot}"#,
            two_functions()
        )));
        assert!(
            e.source.contains("plot(f, g, -19.339, 19.339)"),
            "{}",
            e.source
        );
        // Nothing is written for the definition itself: a list of series is not
        // a value. Counted rather than silent.
        assert!(!e.source.contains("M ="), "{}", e.source);
        assert!(
            e.notes
                .iter()
                .any(|n| n.kind == NoteKind::Carried && n.detail.contains("names 2 series")),
            "{:?}",
            e.notes
        );
    }

    #[test]
    fn a_plot_of_a_curve_and_a_table_together_is_refused() {
        // SMath draws a sampled curve and a table of measured points on one
        // chart; Nomo's `plot` draws one kind at a time. Drawing the curve
        // alone would lose the table without saying so.
        let mut definition = String::from(&operand("XY"));
        for v in ["1", "2", "2", "1"] {
            definition.push_str(&operand(v));
        }
        definition.push_str(&call("mat", 4));
        definition.push_str(&op(":", 2));
        let mut plotted = String::from(&operand("x"));
        plotted.push_str(&call("f", 1));
        plotted.push_str(&operand("XY"));
        plotted.push_str(&operand("2"));
        plotted.push_str(&operand("1"));
        plotted.push_str(&call("sys", 4));
        let plot = format!(
            r#"<region id="2" left="0" top="60" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">{plotted}</plot></region>"#
        );
        let e = emit_src(&wrap(&format!(
            r#"{}<region id="1" left="0" top="40"><math>{definition}</math></region>{plot}"#,
            two_functions()
        )));
        assert!(
            e.source
                .contains("some of its series are functions of `x` and some are not"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_definition_is_not_a_curve_when_x_has_a_value() {
        // `x : 5` above it means `sin(5)`, a number. Turning that into a
        // function would invent a curve the worksheet never drew.
        let mut bind_x = String::from(&operand("x"));
        bind_x.push_str(&operand("5"));
        bind_x.push_str(&op(":", 2));
        let mut toks = String::from(&operand("p"));
        toks.push_str(&operand("x"));
        toks.push_str(&call("sin", 1));
        toks.push_str(&op(":", 2));
        // Above the definition, because `:` resolves in reading order: an `x`
        // bound below it would leave the line symbolic, which is a curve.
        let plot = r#"<region id="2" left="0" top="40" width="350" height="233">
            <plot type="2d" render="lines" scale_x="1" scale_y="1"
                  transpose_x="0" transpose_y="0">
            <e type="operand">p</e></plot></region>"#;
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math>{bind_x}</math></region>
               <region id="1" left="0" top="20"><math>{toks}</math></region>{plot}"#
        )));
        assert!(!e.source.contains("fn p("), "{}", e.source);
        assert!(e.source.contains("p = sin(x)"), "{}", e.source);
    }

    #[test]
    fn a_plot_keeps_what_it_plotted() {
        // Nomo has no plots, but the expression is the worksheet's content and
        // there is no reason to lose it along with the chart.
        let regions = r#"<region id="0" left="0" top="0"><plot>
            <e type="operand">t</e><e type="function" args="1">sin</e></plot></region>"#;
        let e = emit_src(&wrap(regions));
        assert!(e.source.contains("a `plot` of sin(t)"), "{}", e.source);
        // An expression that cannot be translated still reports the region
        // rather than pretending it was empty.
        let bad = r#"<region id="0" left="0" top="0"><plot>
            <e type="operand">t</e><e type="function" args="1">Maxima</e></plot></region>"#;
        let e = emit_src(&wrap(bad));
        assert!(e.source.contains("a `plot`"), "{}", e.source);
    }

    #[test]
    fn the_dagger_is_the_cross_product() {
        let e = emit_src(&wrap(&format!(
            "{}{}",
            given(&["r", "F"]),
            math(&format!(
                "{}{}{}{}{}",
                operand("M"),
                operand("r"),
                operand("F"),
                op("†", 2),
                op(":", 2)
            ))
        )));
        assert!(e.source.contains("M = cross(r, F)"), "{}", e.source);
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
    }

    #[test]
    fn a_free_symbol_is_marked_and_the_line_kept_as_a_comment() {
        // Seen in the wild as `Vout : k*V2 - V1`, with `V1` and `V2` bound
        // nowhere in the file. SMath evaluated it symbolically and saved
        // without an error;
        // Nomo has no free symbols, so emitting it produces a worksheet that
        // fails to evaluate with nothing to say why.
        let e = emit_src(&wrap(&format!(
            "{}{}",
            given(&["k"]),
            math(&format!(
                "{}{}{}{}{}{}{}",
                operand("Vout"),
                operand("k"),
                operand("V2"),
                op("*", 2),
                operand("V1"),
                op("-", 2),
                op(":", 2)
            ))
        )));
        assert!(
            e.source
                .contains("' [import] unsupported: `V1`, `V2` are used here"),
            "{}",
            e.source
        );
        // The formula is still the worksheet's content, so it survives — as a
        // comment, where it cannot be mistaken for something the engine ran.
        assert!(e.source.contains("' Vout = k*V2 - V1"), "{}", e.source);
        assert!(!e.source.contains("\nVout = "), "{}", e.source);
        // One region a human has to look at, counted once.
        assert_eq!(
            e.notes
                .iter()
                .filter(|n| n.kind == NoteKind::Unsupported)
                .count(),
            1
        );
    }

    #[test]
    fn a_symbolic_region_says_why_smath_accepted_it() {
        // `optimize="2"` is the region's own evidence: SMath's CAS kept the name
        // as a symbol rather than failing, which is why the file carries no
        // `error` attribute for a reviewer to find.
        let tokens = format!("{}{}{}", operand("y"), operand("q"), op(":", 2));
        let e = emit_src(&wrap(&format!(
            r#"<region id="0" left="0" top="0"><math optimize="2"><input>{tokens}</input></math></region>"#
        )));
        assert!(
            e.source.contains("and SMath kept the region symbolic"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_name_bound_anywhere_is_not_free() {
        // Defined *below* its use. That is a scope question, reported as a
        // flattened scope where it applies, and not the same complaint as a name
        // the document never defines at all.
        let e = emit_src(&wrap(&format!(
            "{}{}",
            math(&format!("{}{}{}", operand("b"), operand("a"), op(":", 2))),
            given(&["a"]),
        )));
        assert!(e.source.contains("b = a"), "{}", e.source);
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
    }

    #[test]
    fn a_block_local_name_is_bound_by_its_block() {
        // `line(t : 2, t + 1)`. The `:` is nested rather than at a region root,
        // so `classify` leaves it an ordinary node — and a reader that only
        // looked at statements would call `t` free and refuse a working block.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}{}{}{}{}",
            operand("u"),
            operand("t"),
            operand("2"),
            op(":", 2),
            operand("t"),
            operand("1"),
            op("+", 2),
            call("line", 2),
            op(":", 2),
        ))));
        assert!(
            !e.source.contains("defined nowhere in this worksheet"),
            "{}",
            e.source
        );
    }

    #[test]
    fn a_free_symbol_carries_no_assertion() {
        // A commented-out line computes nothing. Asserting a stored answer
        // against it would report the comment as a disagreement and blame the
        // engine for a gap the marker above it has already named.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}",
            operand("Ling"),
            operand("345.78"),
            op("=", 2)
        ))));
        assert!(e.assertions.is_empty(), "{:?}", e.assertions);
    }

    #[test]
    fn a_root_equivalence_over_an_expression_binds_nothing() {
        // `a + b ≡ c` is an equation written for the reader. Treating the glyph
        // as a definition wherever it sits at a root invents a variable whose
        // name is a whole expression.
        let e = emit_src(&wrap(&math(&format!(
            "{}{}{}{}{}",
            operand("a"),
            operand("b"),
            op("+", 2),
            operand("c"),
            op("≡", 2)
        ))));
        assert!(e.source.contains("' a + b = c"), "{}", e.source);
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
    }

    #[test]
    fn a_stored_answer_becomes_an_assertion_at_its_line() {
        let e = emit_src(&wrap(&format!(
            "{}{}",
            given(&["Ling"]),
            math(&format!(
                "{}{}{}",
                operand("Ling"),
                operand("345.78"),
                op("=", 2)
            ))
        )));
        assert_eq!(e.assertions.len(), 1);
        assert_eq!(e.assertions[0].expected, "345.78");
        assert_eq!(e.assertions[0].mantissa, "345.78");
        let line = e.source.lines().nth(e.assertions[0].line - 1).unwrap();
        assert_eq!(line, "Ling");
    }

    #[test]
    fn a_definition_carries_its_stored_answer_too() {
        // The newer era keeps the answer beside the definition rather than in a
        // display of its own, and it means the same thing: this is what `Ling`
        // is worth. Checking only the displays left most of a modern worksheet's
        // own arithmetic unguarded.
        let e = emit_src(&wrap(&answered(
            &[operand("Ling"), operand("2"), op(":", 2)].concat(),
            &operand("345.78"),
        )));
        assert_eq!(e.assertions.len(), 1);
        assert_eq!(e.assertions[0].expected, "345.78");
        let line = e.source.lines().nth(e.assertions[0].line - 1).unwrap();
        assert_eq!(line, "Ling = 2");
    }

    #[test]
    fn a_unit_seen_only_in_a_stored_answer_can_still_be_spelled() {
        // A worksheet states its inputs in one unit and SMath answers in
        // another. Collecting names from the input side alone left the answer
        // unspellable, and an assertion that cannot be written is an assertion
        // that is silently not made.
        let e = emit_src(&wrap(&answered(
            &[operand("Cr"), operand("2"), op(":", 2)].concat(),
            &[operand("4.5"), unit("F"), op("*", 2)].concat(),
        )));
        assert_eq!(e.assertions.len(), 1, "{:?}", e.notes);
        assert_eq!(e.assertions[0].expected, "4.5 F");
    }

    #[test]
    fn a_call_to_a_function_the_worksheet_defines_is_written_out() {
        let e = emit_src(&wrap(&format!(
            "{}{}",
            region(&[
                operand("x"),
                call("f", 1),
                operand("x"),
                operand("2"),
                op("^", 2),
                op(":", 2)
            ]),
            region(&[operand("3"), call("f", 1)])
        )));
        assert!(e.source.contains("fn f(x) = x^2"), "{}", e.source);
        assert!(e.source.contains("\nf(3)"), "{}", e.source);
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
    }

    #[test]
    fn a_variable_named_like_a_unit_the_worksheet_uses_is_moved_aside() {
        // `m := 1 kg` beside `d := 1 m` — a mass and a metre, told apart in
        // SMath by an attribute and in Nomo by nothing at all. Emitting both as
        // `m` makes the binding hide the unit and turns the length into a mass,
        // with the right number and the wrong dimension and no complaint.
        let e = emit_src(&wrap(&format!(
            "{}{}",
            region(&[
                operand("m"),
                operand("1"),
                unit("kg"),
                op("*", 2),
                op(":", 2)
            ]),
            region(&[
                operand("d"),
                operand("1"),
                unit("m"),
                op("*", 2),
                op(":", 2)
            ])
        )));
        assert!(e.source.contains("m_ = 1 kg"), "{}", e.source);
        assert!(e.source.contains("d = 1 m"), "{}", e.source);
        assert!(e.notes.iter().any(|n| n.kind == NoteKind::Renamed));
    }

    #[test]
    fn a_variable_named_like_a_unit_the_worksheet_never_uses_keeps_its_name() {
        // The rename is for a real collision, not for every name that happens to
        // spell a unit. A worksheet with a mass `m` and no length in it reads
        // better as the author wrote it.
        let e = emit_src(&wrap(&region(&[
            operand("m"),
            operand("1"),
            unit("kg"),
            op("*", 2),
            op(":", 2),
        ])));
        assert!(e.source.contains("m = 1 kg"), "{}", e.source);
        assert!(!e.notes.iter().any(|n| n.kind == NoteKind::Renamed));
    }

    #[test]
    fn degrees_mode_is_never_translated_in_silence() {
        let xml = wrap(&math(&operand("x"))).replace(
            "<precision>2</precision>",
            "<precision>2</precision><angle>degree</angle>",
        );
        let e = emit_src(&xml);
        assert!(e.notes.iter().any(|n| n.kind == NoteKind::Unsupported));
        assert!(e.source.contains("angle mode"), "{}", e.source);
    }

    #[test]
    fn a_respelt_name_stays_a_legal_nomo_name() {
        for name in ["A.total", "q'", "#out", "rate#", "l.d", "1st"] {
            let out = respell(name).expect("should respell");
            let (parsed, _) = nomo_core::run_source(&format!("{out} = 1"));
            assert!(
                matches!(parsed[0].kind, nomo_core::OutcomeKind::Assign { .. }),
                "{name} became {out}, which Nomo will not accept"
            );
        }
    }
}
