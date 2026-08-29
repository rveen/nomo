//! What SMath provides, and what a worksheet brought itself.
//!
//! `.sm` serialises a user-defined function exactly like a built-in one: both are
//! `<e type="function">` with a name and an arity, and nothing in the XML says
//! which is which. Telling them apart needs outside knowledge, so this module
//! holds it.
//!
//! The registry is not the whole answer, and deliberately so. A function is
//! **defined by its own document** if some region binds a call of that name —
//! that is a fact about the file and needs no registry at all, so it is checked
//! first. The registry only has to catch what is left. A name that is neither
//! is *unknown*, and an unknown name is reported rather than guessed at
//! (design note §8.7 item 8).
//!
//! **Provenance, so this is not trusted further than it deserves:** the list
//! below is assembled from the names appearing across the 54-worksheet corpus
//! plus general knowledge of SMath's function set. Most of it has **not** been
//! checked against SMath's own documentation or a running copy. Its errors are
//! visible in the coverage report — a built-in missing from it shows up as an
//! unknown function — which is the reason the report exists.
//!
//! **One entry has been corrected by reading the installed copy** (design note
//! §8.42): `hlookup`, `vlookup`, `hmatch` and `vmatch` were here and are not
//! SMath functions at all. `SMath.Manager`'s function-name table holds
//! `Linterp`, `Cinterp` and `Ainterp` and no lookup of any kind, so those four
//! come from a plugin this installation does not carry. They are 18 calls in
//! exactly one worksheet — `SimplySupportedTimberBeam_Eurocode5_v1.0.sm`, which
//! reads a timber grade table — and the coverage report now calls them unknown,
//! which is what they are. "SMath provides this and Nomo does not" and "nobody
//! here knows what this does" are different answers, and only the second is
//! true of these.

/// Names SMath is believed to provide.
///
/// Grouped by area rather than sorted, because the groups are how the gaps
/// become obvious: an area with two entries is an area nobody has checked.
#[rustfmt::skip]
const BUILTINS: &[&str] = &[
    // Arithmetic and elementary
    "abs", "sign", "sqrt", "nthroot", "exp", "ln", "log", "log10", "mod", "gcd", "lcm", "fact",
    "Gamma", "random", "max", "min", "sum", "product",
    // Trigonometry. `angle` in the document settings decides degrees or radians
    // for every one of these, which is why the reader keeps that setting.
    "sin", "cos", "tan", "cot", "sec", "csc", "asin", "acos", "atan", "acot", "arcsin", "arccos",
    "arctan", "sinh", "cosh", "tanh", "coth", "asinh", "acosh", "atanh",
    // Rounding. SMath spells the direction into the name: `d` down, `u` up.
    "round", "trunc", "floor", "ceil", "dRound", "uRound", "dTrunc", "uTrunc", "dFloor", "uFloor",
    "dCeil", "uCeil", "OoM",
    // Complex
    "Re", "Im", "conj", "arg", "polar",
    // Vectors and matrices. `el` is the most-used function in the entire corpus.
    "el", "mat", "matrix", "identity", "augment", "stack", "submatrix", "transpose", "row", "col",
    "rows", "cols", "length", "reverse", "sort", "csort", "rsort", "chunk", "concat", "det",
    "invert", "rank", "trace", "diag", "norm", "cross", "dot",
    // Control flow, which is function application in this format rather than
    // syntax: `line` is a statement block, `for` and `while` take a body.
    "if", "for", "while", "line", "range", "break", "continue", "return", "eval", "error",
    // Numerics
    "solve", "roots", "diff", "int", "bisection", "Jacobi", "numbering",
    // Interpolation. Read out of the installed copy rather than assumed: these
    // three are `TMatrix::Linterp`, `Cinterp` and `Ainterp` in
    // `SMath.Math.Numeric.dll`, and `SMath.Manager`'s name table has entries for
    // exactly them. `minterp` stays on the strength of the corpus alone.
    "linterp", "cinterp", "ainterp", "minterp",
    // Strings
    "num2str", "str2num", "substr", "findstr", "strlen", "IsString", "IsDefined", "UoM",
    // Data
    "importData", "exportData",
    // Financial
    "pmt", "fv", "pv", "nper", "rate", "cnper", "crate", "fva", "fvc", "pva", "cumint", "cumprn",
    "prnpmt", "intpmt",
    // Drawing and 2D geometry, from SMath's plotting side. Present in the corpus
    // but of no interest to an engine: these produce pictures, not numbers.
    "pgon", "hPgon", "sPgon", "oPgon", "esPgon", "ssPgon", "ellipse", "circle", "semicircle",
    "arch", "ring", "rectangle", "roundrect", "roundedRect", "square", "triangle", "rTriangle",
    "iTriangle", "eTriangle", "rhombus", "parallelogram", "trapezoid", "rTrapezoid", "iTrapezoid",
    "goldenRect", "Rot", "Rotate", "Translate", "Scale", "Shear", "Mirror", "sys",
    // The overbar: forces an expression to evaluate element by element rather
    // than as matrix algebra.
    "vectorize",
];

/// Names a **plugin** provides, and which plugin provides each.
///
/// From 1.x a worksheet declares its plugins in `<dependencies>`, so this is not
/// guesswork about where a name comes from — the file says. It matters for a
/// migration report because "we have never heard of this name" and "this is
/// Maxima, which Nomo does not have and will not get" are different answers,
/// and only the second is a decision rather than a gap.
///
/// The CAS entries are out of scope by decision, not by omission: the target
/// user is the engineer doing dimensioned calculations, and 7.7% of the
/// mechanics corpus's regions reach for computer algebra.
#[rustfmt::skip]
const PLUGINS: &[(&str, &str)] = &[
    // MaximaPlugin — computer algebra. Out of scope.
    ("Maxima", "MaximaPlugin"), ("MaximaTakeover", "MaximaPlugin"),
    ("MaximaControl", "MaximaPlugin"), ("MaximaDefine", "MaximaPlugin"),
    ("assume", "MaximaPlugin"), ("ratexpand", "MaximaPlugin"), ("float", "MaximaPlugin"),
    ("simp", "MaximaPlugin"), ("simp.no_units", "MaximaPlugin"), ("Jacob", "MaximaPlugin"),
    // CustomFunctions — the symbolic-solve idiom, out of scope, plus a handful
    // of ordinary helpers that are not.
    ("Clear", "CustomFunctions"), ("Solve", "CustomFunctions"), ("Assign", "CustomFunctions"),
    ("Unknowns", "CustomFunctions"), ("at", "CustomFunctions"),
    ("description", "CustomFunctions"), ("cases", "CustomFunctions"),
    ("ltlt", "CustomFunctions"), ("ltle", "CustomFunctions"), ("lele", "CustomFunctions"),
    ("norme", "CustomFunctions"), ("strrep", "CustomFunctions"),
    // Numerics, all implementable.
    ("FindRoot", "Nonlinear Solvers"),
    ("rkfixed", "Mathcad Toolbox"), ("lspline", "Mathcad Toolbox"),
    ("sys2mat", "Mathcad Toolbox"), ("ODE.2", "Mathcad Toolbox"),
    ("dn_LinAlgEigenvalues", "DotNumerics"), ("dn_LinAlgEigenvectors", "DotNumerics"),
    ("eigens_by_jacobi", "DotNumerics"),
];

/// Which plugin provides `name`, if a plugin does.
pub fn plugin(name: &str) -> Option<&'static str> {
    PLUGINS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

/// Whether SMath is believed to provide this name.
pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// How many names the registry holds, for the coverage report's header.
pub fn count() -> usize {
    BUILTINS.len()
}

/// Operator glyphs whose meaning is settled.
///
/// Assignment glyphs are handled before this point, in
/// [`crate::expr::classify`], because their meaning depends on whether they sit
/// at the root of a region.
pub fn is_known_operator(glyph: &str) -> bool {
    matches!(
        glyph,
        "+" | "-"
            | "*"
            | "/"
            | "^"
            | "!"
            | "±"
            | "<"
            | ">"
            | "≤"
            | "≥"
            | "≠"
            | "&"
            | "¬"
            | "†"
            | "←"
            | ":"
            | "≡"
            | "="
    )
}

/// The glyphs the design note listed as unidentified (§11 question 9). Two are
/// now settled by a second corpus; the third is not, and is not implemented on a
/// guess.
///
/// * `†` — **the cross product**, and no longer here. The wiki corpus had two
///   uses in a matrix expression and told us nothing. The mechanics corpus has
///   48, every one of them either a moment `r † F` summed with other moments or
///   a unit normal `e.z † e.t(t)`. It is never at a region root: an ordinary
///   binary operator inside a sum.
/// * `|` — **logical or**. All three uses sit between two comparisons inside an
///   `if` condition, alongside `&` and `¬`. Kept here rather than implemented
///   because three uses in one corpus is thin evidence for a glyph that would
///   silently change a condition if it were something else.
/// * `—` U+2014 (6 uses) — narrowed but open. Every use has a **function call**
///   on its left — never a bare name — and that function's fully expanded form
///   on its right, which reads as a *symbolic evaluation display* rather than a
///   binding. If that is right, its right-hand sides are stored answers for the
///   oracle rather than definitions.
///
/// Left out of [`is_known_operator`] on purpose: the coverage report is more
/// useful naming nine uses it cannot translate than it would be quietly
/// translating them on a hunch.
pub const UNIDENTIFIED: &[&str] = &["|", "—"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_used_corpus_functions_are_all_known() {
        // Ranked by use across the corpus. If the registry cannot name these it
        // is not worth having.
        for name in [
            "el", "mat", "eval", "line", "if", "range", "for", "sin", "sqrt", "cos",
        ] {
            assert!(is_builtin(name), "{name} missing from the registry");
        }
    }

    #[test]
    fn a_worksheets_own_function_is_not_a_builtin() {
        for name in ["fSJ", "nasaT", "hPP", "A.P", "yL"] {
            assert!(!is_builtin(name), "{name} should not be in the registry");
        }
    }

    #[test]
    fn the_unidentified_glyphs_stay_unknown() {
        for glyph in UNIDENTIFIED {
            assert!(!is_known_operator(glyph));
        }
    }

    #[test]
    fn arity_bearing_arithmetic_is_known() {
        for glyph in ["+", "-", "*", "/", "^", "≤", "¬", "&"] {
            assert!(is_known_operator(glyph));
        }
    }
}
