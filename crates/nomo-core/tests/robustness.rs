//! What the engine does with input nobody wrote.
//!
//! Every other test here states a case and its answer. This one states no
//! answers at all — it generates worksheets that are mostly nonsense and asserts
//! the *properties* that must hold whatever the input is: the engine returns
//! rather than aborting, every diagnostic points somewhere real, and the same
//! source gives the same output twice.
//!
//! It exists because the parser had no nesting limit for the whole of this
//! project's life and nothing noticed. The unit tests could not: each of them
//! states an input, and nobody states 20 000 brackets. A generator does, in
//! about a second.
//!
//! **Deterministic, and no dependency.** A fixed seed, a fixed case count and
//! twenty lines of xorshift rather than `proptest` or `cargo-fuzz`: this is a
//! gate, so it has to fail for the same reason on every machine and be reported
//! with the case that broke it. `nomo-core` carries exactly one dependency and
//! it is a math library; a test harness is not a good reason for the second.
//! A shrinking property-test framework would be worth it the day this finds
//! something it cannot report legibly, and not before.

use nomo_core::diag::Severity;

/// The seed. Any value works; this one is fixed so that a failure here
/// reproduces exactly, on any machine, forever.
const SEED: u64 = 0x_9E37_79B9_7F4A_7C15;

/// The seed, and how many cases each test runs.
///
/// Both are fixed by default, because this is a gate: it has to fail for the
/// same reason on every machine, and a gate that generates different work each
/// run reports a failure nobody else can reproduce. `NOMO_FUZZ_SEED` and
/// `NOMO_FUZZ_CASES` override them for a soak — a longer run with a different
/// seed is how you go looking for the next bug rather than guarding against the
/// last one, and anything it finds gets a case of its own here.
fn seed(salt: u64) -> u64 {
    match std::env::var("NOMO_FUZZ_SEED") {
        Ok(v) => v.parse::<u64>().unwrap_or(SEED) ^ salt,
        Err(_) => SEED ^ salt,
    }
}

fn cases(default: usize) -> usize {
    match std::env::var("NOMO_FUZZ_CASES") {
        Ok(v) => v.parse().unwrap_or(default),
        Err(_) => default,
    }
}

/// xorshift64*, which is ten lines and good enough to generate rubbish with.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// The pieces a worksheet is made of, including the ones that hurt.
///
/// Multi-byte characters are in here deliberately. `π`, `°` and `—` are ordinary
/// in this language — every unit-bearing worksheet has some — and an offset bug
/// around them is exactly the class of failure that shipped once already.
const PIECES: &[&str] = &[
    "1",
    "2.5",
    "0",
    "-1",
    "1e3",
    "1.2.3",
    "1e", // numbers, valid and not
    "x",
    "y",
    "V_drop",
    "σ",
    "Δt",
    "f1", // names
    "m",
    "cm",
    "kg",
    "s",
    "°C",
    "MPa",
    "in", // units
    "+",
    "-",
    "*",
    "/",
    "^",
    "->",
    "==",
    "<=",
    "<",
    ">", // operators
    "(",
    ")",
    "[",
    "]",
    ",",
    "=", // structure
    "unit",
    "fn",
    "global",
    "if",
    "then",
    "else",
    "and",
    "or",
    "not", // keywords
    "pi",
    "e",
    "inf", // constants
    "sin",
    "sqrt",
    "map",
    "range",
    "iterate",
    "root",
    "derivative", // calls
    "\"grade\"",
    "\"", // strings, closed and not
    "' prose",
    "' # heading", // comments
    "π",
    "°",
    "—",
    "≤",
    "€",
    "\u{200b}", // characters that are not ASCII
    " ",
    "  ",
    "\n",
    "\n\n",
    "\t", // whitespace, newlines included
];

/// Worksheets that mean something, used as the starting point for mutation.
///
/// Nonsense finds one class of bug and *nearly* correct input finds another:
/// most of what an editor sees is a valid worksheet with one character wrong,
/// because that is what half-typed input is.
const SEEDS: &[&str] = &[
    "r = 5 cm\nh = 12 cm\nV = pi*r^2*h\nV -> dm^3\n",
    "unit kip = 1000 lbf\nw = 2.5 kip/ft\nM = w*(30 ft)^2/8\n",
    "T = 20 °C + 5 K\ndT = 20 °C - 15 °C\n",
    "K = [[2, -1], [-1, 2]]\nd = det(K)\nI = K*inv(K)\n",
    "fn f(x) = x^2 - 2\nz = root(f, 0, 2)\ns = sum(map(f, range(0, 4)))\n",
    "a = 3 m\nverdict = if a <= 4 m then \"ok\" else \"no\"\n",
    "' # Title\n' Some prose about the calculation.\nq = 2 m + 3 m\n",
];

/// Run a case list on a thread with room to spare.
///
/// The engine's bound has to fit the WebAssembly build's 1 MB stack, and it
/// does. A native *debug* test thread is a different question: it gets 2 MiB and
/// frames several times larger than release, so it can overflow on a worksheet
/// the shipped build evaluates without trouble — which is a fact about this
/// harness, not about the engine. A soak run found exactly that
/// (`NOMO_FUZZ_SEED=2`), and diagnosing it twice would be a waste. 64 MiB takes
/// the harness out of the picture, leaving the engine's own refusals as the only
/// thing under test.
fn on_a_deep_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("the generated worksheets should not bring the engine down");
}

/// Everything that must be true of the engine's answer, whatever went in.
fn exercise(name: &str, source: &str) {
    let sheet = nomo_core::Sheet::new(source);

    for d in sheet.diagnostics() {
        let (start, end) = (d.span.start as usize, d.span.end as usize);
        assert!(
            start <= end && end <= source.len(),
            "{name}: diagnostic span {start}..{end} is outside a {} byte source\n\
             message: {}\nsource: {source:?}",
            source.len(),
            d.message,
        );
        // The editor slices the source with these. A span landing inside a
        // multi-byte character would panic there — or, worse, in `str::text`
        // here — so it is checked rather than assumed.
        assert!(
            source.is_char_boundary(start) && source.is_char_boundary(end),
            "{name}: diagnostic span {start}..{end} splits a character\nsource: {source:?}",
        );
        assert!(
            !d.message.is_empty(),
            "{name}: a diagnostic with no message\nsource: {source:?}",
        );
        assert!(
            matches!(d.severity, Severity::Error | Severity::Warning),
            "{name}: unknown severity",
        );
    }

    // Rendering is not optional on bad input: the editor renders what it has
    // after every keystroke, and most keystrokes land on a worksheet that does
    // not parse. `snapshot` is parse, evaluate, render text, render HTML and
    // report diagnostics in one call — the same function the WebAssembly build
    // exports — so this covers the whole pipeline rather than the parser alone.
    let first = nomo_core::golden::snapshot(name, source);

    // Determinism is the product. Two runs over one source must agree exactly:
    // an iteration order that leaked into the output would show up here, on a
    // thousand inputs, rather than on the one worksheet that happened to hash
    // differently on somebody else's machine.
    let second = nomo_core::golden::snapshot(name, source);
    assert!(
        first == second,
        "{name}: two runs over the same source disagreed\nsource: {source:?}",
    );

    // The browser parses this string. A raw control character in it is invalid
    // JSON, which would break the editor rather than the worksheet.
    let json = nomo_core::api::analysis_json(&sheet);
    assert!(
        !json.chars().any(|c| (c as u32) < 0x20),
        "{name}: the analysis payload carries a raw control character\nsource: {source:?}",
    );
}

#[test]
fn token_soup_is_survivable() {
    on_a_deep_stack(|| {
        let mut rng = Rng(seed(0));
        for case in 0..cases(2000) {
            let mut source = String::new();
            for _ in 0..rng.below(40) + 1 {
                source.push_str(rng.pick(PIECES));
            }
            exercise(&format!("soup-{case}"), &source);
        }
    });
}

#[test]
fn one_character_wrong_is_survivable() {
    on_a_deep_stack(|| {
        let mut rng = Rng(seed(0xABCD));
        for case in 0..cases(2000) {
            let seed = *rng.pick(SEEDS);
            // Work in characters rather than bytes: cutting a `°` in half would test
            // the test harness rather than the engine, and `String` would panic
            // before the engine saw anything.
            let mut chars: Vec<char> = seed.chars().collect();
            for _ in 0..rng.below(3) + 1 {
                if chars.is_empty() {
                    break;
                }
                let at = rng.below(chars.len());
                match rng.below(5) {
                    0 => {
                        chars.remove(at);
                    }
                    1 => chars.insert(at, rng.pick(PIECES).chars().next().unwrap_or('?')),
                    2 => chars[at] = rng.pick(PIECES).chars().next().unwrap_or('?'),
                    3 => {
                        let c = chars[at];
                        chars.insert(at, c);
                    }
                    _ => chars.truncate(at),
                }
            }
            exercise(
                &format!("mutation-{case}"),
                &chars.into_iter().collect::<String>(),
            );
        }
    });
}

#[test]
fn nesting_around_the_limit_is_survivable() {
    on_a_deep_stack(|| {
        // The bug this file exists for. Straddling `MAX_NEST` covers both sides of
        // the guard, and mixing the ways of nesting covers the paths into it: a
        // bracket, a call argument, a vector literal and an index all recurse.
        let mut rng = Rng(SEED ^ 0x1234);
        let opens = ["(", "sin(", "[", "-(", "abs("];
        for case in 0..200 {
            let depth = nomo_core::parse::MAX_NEST - 8 + rng.below(16);
            let mut open = String::new();
            let mut close = String::new();
            for _ in 0..depth {
                let piece = *rng.pick(&opens);
                open.push_str(piece);
                close.push(if piece == "[" { ']' } else { ')' });
            }
            let source = format!(
                "x = {open}1{}\ny = 2 m + 3 m\n",
                close.chars().rev().collect::<String>()
            );
            exercise(&format!("nesting-{case}"), &source);
        }
    });
}

#[test]
fn nesting_times_recursion_is_survivable() {
    // The two limits multiply, which is how a worksheet respecting both of them
    // still ran the stack out: brackets up to `MAX_NEST` inside calls up to
    // `MAX_DEPTH` is some 7 700 nested evaluations, and the shipped WebAssembly
    // build trapped on it. `MAX_EVAL_NEST` is what bounds the product; these
    // straddle it from both directions.
    on_a_deep_stack(|| {
        let mut rng = Rng(seed(0x9ABC));
        for case in 0..cases(100) {
            let brackets = rng.below(nomo_core::parse::MAX_NEST - 4) + 1;
            let calls = rng.below(80) + 1;
            let body = format!("{}f(n - 1){}", "(".repeat(brackets), ")".repeat(brackets));
            let source = format!("fn f(n) = if n <= 0 then 1 else {body}\ny = f({calls})\n");
            exercise(&format!("product-{case}"), &source);
        }
    });
}

#[test]
fn a_long_worksheet_is_survivable() {
    on_a_deep_stack(|| {
        // Length rather than depth: a thousand statements, each of which may or may
        // not parse, with the dependency graph having to sort out what refers to
        // what. Statements are generated in pairs so that roughly half of them
        // reference a name defined above and the rest reference one that does not
        // exist.
        let mut rng = Rng(SEED ^ 0x5678);
        let mut source = String::new();
        for i in 0..1000 {
            match rng.below(4) {
                0 => source.push_str(&format!("v{i} = {} m\n", rng.below(100))),
                1 => source.push_str(&format!("v{i} = v{} + 1 m\n", rng.below(i + 1))),
                2 => source.push_str(&format!("v{i} = v{} * {}\n", i + 1, rng.below(10))),
                _ => {
                    for _ in 0..rng.below(10) + 1 {
                        source.push_str(rng.pick(PIECES));
                    }
                    source.push('\n');
                }
            }
        }
        exercise("long", &source);
    });
}
