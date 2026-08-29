//! The worksheet as a dependency graph.
//!
//! A worksheet is not a script to run top to bottom; it is a set of definitions
//! with dependencies between them. Modelling that explicitly is what buys, from
//! one structure: deterministic evaluation order, incremental recalculation that
//! touches only what actually changed, cycle detection that reports an error
//! instead of hanging, and caching that needs no separate bookkeeping.
//!
//! EngineeringPaper.xyz is the counter-example. It hands the entire sheet to
//! Python as one JSON blob, gets every result back at once, and caches on that
//! whole blob with a hundred-entry LRU — all of which is a workaround for not
//! having this graph.
//!
//! # Two kinds of binding
//!
//! A *positional* binding (`x = 1`) is visible to statements below it. A *global*
//! binding (`global g = 9.81 m/s^2`) is visible everywhere, including above its
//! own definition. Globals exist because SMath's `≡` behaves that way, verified
//! by 39 forward references in the surveyed corpus, so imported worksheets will
//! rely on it.
//!
//! When both exist for one name, the nearest preceding positional binding wins,
//! falling back to the global. That is the rule Mathcad-family tools use: globals
//! are established first, then reading-order assignments override them.

use crate::ast::{Ast, Expr, Stmt};
use crate::span::Span;
use std::collections::{BTreeMap, BTreeSet};

/// What a statement binds, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    None,
    /// Visible below this statement only.
    Positional(String),
    /// Visible throughout the worksheet.
    Global(String),
}

impl Binding {
    pub fn name(&self) -> Option<&str> {
        match self {
            Binding::None => None,
            Binding::Positional(n) | Binding::Global(n) => Some(n),
        }
    }
}

/// One statement's place in the graph.
#[derive(Debug, Clone)]
pub struct Node {
    pub binds: Binding,
    /// Names this statement reads.
    pub uses: BTreeSet<String>,
    /// Statement indices this one must be evaluated after.
    pub depends_on: BTreeSet<usize>,
    /// Statement indices that must be evaluated after this one.
    pub dependents: BTreeSet<usize>,
    pub span: Span,
}

/// A cycle among statements, named by the binding that closes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    /// Statement indices taking part, in the order they were discovered.
    pub members: Vec<usize>,
    pub names: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DepGraph {
    pub nodes: Vec<Node>,
    /// A valid evaluation order. Statements in a cycle are omitted.
    pub order: Vec<usize>,
    pub cycles: Vec<Cycle>,
}

impl DepGraph {
    pub fn build(ast: &Ast) -> DepGraph {
        let mut nodes: Vec<Node> = ast
            .stmts
            .iter()
            .map(|s| Node {
                binds: binding_of(s),
                uses: uses_of(s),
                depends_on: BTreeSet::new(),
                dependents: BTreeSet::new(),
                span: s.span(),
            })
            .collect();

        // Globals first: they are visible everywhere, so they are resolved
        // without regard to position. A later global of the same name wins, which
        // matches the last-definition-wins rule used for everything else.
        let mut globals: BTreeMap<String, usize> = BTreeMap::new();
        for (i, n) in nodes.iter().enumerate() {
            if let Binding::Global(name) = &n.binds {
                globals.insert(name.clone(), i);
            }
        }

        // Then walk in reading order, tracking the most recent positional
        // binding of each name.
        let mut positional: BTreeMap<String, usize> = BTreeMap::new();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            for name in &node.uses {
                // Nearest preceding positional binding, else the global.
                let source = positional
                    .get(name)
                    .copied()
                    .or_else(|| globals.get(name).copied().filter(|&g| g != i));
                if let Some(src) = source {
                    if src != i {
                        edges.push((src, i));
                    }
                }
            }
            // A global's own dependencies resolve like anyone else's, but its
            // definition becomes visible to earlier statements too.
            if let Binding::Positional(name) = &node.binds {
                positional.insert(name.clone(), i);
            }
        }

        // A global that mentions its own name is genuinely circular: the binding
        // it would read is the one being defined. A *positional* `a = a + 1` is
        // not — that reads whatever `a` meant on the line above — which is why
        // this case is specific to globals.
        for (name, &def) in &globals {
            if nodes[def].uses.contains(name) {
                edges.push((def, def));
            }
        }

        // Globals are visible above their definition, which the reading-order
        // pass above cannot see. Add those edges now.
        for (name, &def) in &globals {
            for (i, n) in nodes.iter().enumerate() {
                if i == def || !n.uses.contains(name) {
                    continue;
                }
                // Unless a positional binding shadows it at that point.
                let shadowed = nodes[..i]
                    .iter()
                    .any(|m| matches!(&m.binds, Binding::Positional(p) if p == name));
                if !shadowed {
                    edges.push((def, i));
                }
            }
        }

        for (from, to) in edges {
            nodes[to].depends_on.insert(from);
            nodes[from].dependents.insert(to);
        }

        let (order, cycles) = topological_order(&nodes);
        DepGraph {
            nodes,
            order,
            cycles,
        }
    }

    /// Everything that must be recomputed when `changed` changes, including
    /// `changed` itself, in evaluation order.
    pub fn affected(&self, changed: &BTreeSet<usize>) -> Vec<usize> {
        let mut dirty = changed.clone();
        let mut stack: Vec<usize> = changed.iter().copied().collect();
        while let Some(i) = stack.pop() {
            for &d in &self.nodes[i].dependents {
                if dirty.insert(d) {
                    stack.push(d);
                }
            }
        }
        // Evaluation order, restricted to the dirty set.
        self.order
            .iter()
            .copied()
            .filter(|i| dirty.contains(i))
            .collect()
    }
}

/// Kahn's algorithm, breaking ties by statement index so the order is stable.
///
/// Stability matters: two runs over the same worksheet must evaluate in the same
/// sequence, or floating-point results could differ between them.
fn topological_order(nodes: &[Node]) -> (Vec<usize>, Vec<Cycle>) {
    let mut indegree: Vec<usize> = nodes.iter().map(|n| n.depends_on.len()).collect();
    let mut ready: BTreeSet<usize> = (0..nodes.len()).filter(|&i| indegree[i] == 0).collect();
    let mut order = Vec::with_capacity(nodes.len());

    while let Some(&i) = ready.iter().next() {
        ready.remove(&i);
        order.push(i);
        for &d in &nodes[i].dependents {
            indegree[d] -= 1;
            if indegree[d] == 0 {
                ready.insert(d);
            }
        }
    }

    if order.len() == nodes.len() {
        return (order, vec![]);
    }

    // Whatever is left has a cycle running through it.
    let stuck: Vec<usize> = (0..nodes.len()).filter(|i| !order.contains(i)).collect();
    let cycles = strongly_connected(nodes, &stuck);
    (order, cycles)
}

/// Find the cyclic groups among `candidates`, so the error can name them.
fn strongly_connected(nodes: &[Node], candidates: &[usize]) -> Vec<Cycle> {
    let mut cycles = Vec::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();

    for &start in candidates {
        if seen.contains(&start) {
            continue;
        }
        // Everything reachable from `start` that can also reach `start` forms
        // one cyclic group.
        let mut group: Vec<usize> = Vec::new();
        for &other in candidates {
            if reaches(nodes, start, other, candidates) && reaches(nodes, other, start, candidates)
            {
                group.push(other);
            }
        }
        if group.len() < 2 && !reaches_itself(nodes, start) {
            continue;
        }
        if group.is_empty() {
            group.push(start);
        }
        for &g in &group {
            seen.insert(g);
        }
        let names: Vec<String> = group
            .iter()
            .filter_map(|&i| nodes[i].binds.name().map(str::to_string))
            .collect();
        let span = group
            .iter()
            .map(|&i| nodes[i].span)
            .reduce(Span::to)
            .unwrap_or(Span::new(0, 0));
        cycles.push(Cycle {
            members: group,
            names,
            span,
        });
    }
    cycles
}

fn reaches(nodes: &[Node], from: usize, to: usize, within: &[usize]) -> bool {
    if from == to {
        return true;
    }
    let mut seen = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(i) = stack.pop() {
        for &d in &nodes[i].dependents {
            if d == to {
                return true;
            }
            if within.contains(&d) && seen.insert(d) {
                stack.push(d);
            }
        }
    }
    false
}

fn reaches_itself(nodes: &[Node], i: usize) -> bool {
    nodes[i].dependents.contains(&i)
}

fn binding_of(stmt: &Stmt) -> Binding {
    match stmt {
        Stmt::Assign { name, .. } | Stmt::UnitDecl { name, .. } | Stmt::FnDef { name, .. } => {
            Binding::Positional(name.text.clone())
        }
        Stmt::GlobalDef { name, .. } => Binding::Global(name.text.clone()),
        // A check binds nothing: it is read by a reader, not by a statement.
        Stmt::Comment { .. }
        | Stmt::Query { .. }
        | Stmt::Check { .. }
        | Stmt::Use { .. }
        | Stmt::Digits { .. }
        | Stmt::Error { .. } => Binding::None,
    }
}

fn uses_of(stmt: &Stmt) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    match stmt {
        Stmt::Comment { .. } | Stmt::Use { .. } | Stmt::Digits { .. } | Stmt::Error { .. } => {}
        Stmt::Assign { value, .. }
        | Stmt::GlobalDef { value, .. }
        | Stmt::UnitDecl { value, .. } => collect_names(value, &mut out),
        Stmt::Query { expr, .. } | Stmt::Check { expr, .. } => collect_names(expr, &mut out),
        Stmt::FnDef { params, body, .. } => {
            collect_names(body, &mut out);
            // Parameters are bound by the definition, not read from the sheet.
            for p in params {
                out.remove(&p.text);
            }
        }
    }
    out
}

/// Every name an expression reads, including the names of functions it calls.
fn collect_names(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Number { .. } | Expr::Text { .. } | Expr::Error { .. } => {}
        // Both arms are dependencies even though only one will run: which arm
        // that is depends on values, and the graph is built before any value
        // exists. Over-approximating here is right — it can only add an edge,
        // never lose one, and a lost edge is a stale result.
        Expr::If {
            cond,
            then,
            otherwise,
            ..
        } => {
            collect_names(cond, out);
            collect_names(then, out);
            collect_names(otherwise, out);
        }
        Expr::Ident(n) => {
            out.insert(n.text.clone());
        }
        Expr::Unary { operand, .. } => collect_names(operand, out),
        Expr::Paren { inner, .. } => collect_names(inner, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect_names(lhs, out);
            collect_names(rhs, out);
        }
        Expr::Convert { value, unit, .. } => {
            collect_names(value, out);
            collect_names(unit, out);
        }
        Expr::Call { callee, args, .. } => {
            out.insert(callee.text.clone());
            for a in args {
                collect_names(a, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_names(base, out);
            for i in indices {
                collect_names(i, out);
            }
        }
        Expr::Vector { elements, .. } => {
            for e in elements {
                collect_names(e, out);
            }
        }
        Expr::Matrix { rows, .. } => {
            for row in rows {
                for e in row {
                    collect_names(e, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(src: &str) -> DepGraph {
        DepGraph::build(&crate::parse(src).ast)
    }

    #[test]
    fn a_chain_orders_itself() {
        let g = graph("a = 1\nb = a*2\nc = b+1");
        assert_eq!(g.order, vec![0, 1, 2]);
        assert!(g.cycles.is_empty());
        assert_eq!(g.nodes[2].depends_on, [1].into_iter().collect());
    }

    #[test]
    fn independent_statements_keep_source_order() {
        // Nothing forces an order, so the stable tie-break keeps the file's.
        let g = graph("a = 1\nb = 2\nc = 3");
        assert_eq!(g.order, vec![0, 1, 2]);
    }

    #[test]
    fn a_definition_below_a_use_is_not_a_dependency() {
        // Positional bindings only reach downward, so this `x` is undefined
        // rather than forward-referencing line 2.
        let g = graph("y = x\nx = 1");
        assert!(g.nodes[0].depends_on.is_empty());
    }

    #[test]
    fn globals_are_visible_above_their_definition() {
        let g = graph("y = g*2\nglobal g = 10");
        assert_eq!(g.nodes[0].depends_on, [1].into_iter().collect());
        // And therefore the global evaluates first.
        assert_eq!(g.order, vec![1, 0]);
    }

    #[test]
    fn a_positional_binding_shadows_a_global_below_it() {
        let g = graph("global g = 1\ng = 99\ny = g");
        // `y` takes the positional binding on line 2, not the global.
        assert_eq!(g.nodes[2].depends_on, [1].into_iter().collect());
    }

    #[test]
    fn rebinding_takes_the_nearest_definition_above() {
        let g = graph("x = 1\ny = x\nx = 2\nz = x");
        assert_eq!(g.nodes[1].depends_on, [0].into_iter().collect());
        assert_eq!(g.nodes[3].depends_on, [2].into_iter().collect());
    }

    #[test]
    fn function_parameters_are_not_sheet_dependencies() {
        let g = graph("fn f(x) = x*2\ny = f(3)");
        assert!(g.nodes[0].uses.is_empty(), "{:?}", g.nodes[0].uses);
        assert_eq!(g.nodes[1].depends_on, [0].into_iter().collect());
    }

    #[test]
    fn a_function_body_depends_on_what_it_closes_over() {
        let g = graph("k = 2\nfn f(x) = x*k\ny = f(3)");
        assert_eq!(g.nodes[1].depends_on, [0].into_iter().collect());
    }

    #[test]
    fn cycles_are_detected_rather_than_hung_on() {
        let g = graph("global a = b\nglobal b = a");
        assert_eq!(g.cycles.len(), 1);
        let mut names = g.cycles[0].names.clone();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn a_self_referencing_global_is_a_cycle() {
        let g = graph("global a = a + 1");
        assert_eq!(g.cycles.len(), 1, "{:?}", g.cycles);
    }

    #[test]
    fn statements_outside_a_cycle_still_evaluate() {
        let g = graph("x = 5\nglobal a = b\nglobal b = a\ny = x*2");
        assert!(g.order.contains(&0));
        assert!(g.order.contains(&3));
        assert!(!g.order.contains(&1));
    }

    #[test]
    fn affected_walks_downstream_only() {
        let g = graph("a = 1\nb = a*2\nc = b+1\nd = 99");
        let changed = [0].into_iter().collect();
        assert_eq!(g.affected(&changed), vec![0, 1, 2]);

        // Changing a leaf touches nothing else.
        let changed = [3].into_iter().collect();
        assert_eq!(g.affected(&changed), vec![3]);
    }

    #[test]
    fn affected_returns_evaluation_order() {
        let g = graph("global g = 1\ny = g*2\nz = y+1");
        let changed = [0].into_iter().collect();
        let affected = g.affected(&changed);
        // A dependency must never appear after its dependent.
        let pos = |i: usize| affected.iter().position(|&x| x == i).unwrap();
        assert!(pos(0) < pos(1));
        assert!(pos(1) < pos(2));
    }

    #[test]
    fn units_that_are_declared_create_dependencies() {
        let g = graph("unit klf = 1000 lbf/ft\nw = 2.5 klf");
        assert_eq!(g.nodes[1].depends_on, [0].into_iter().collect());
    }

    #[test]
    fn built_in_units_do_not_create_dependencies() {
        let g = graph("w = 2.5 kN");
        assert!(g.nodes[0].depends_on.is_empty());
    }
}
