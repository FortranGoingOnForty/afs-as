//! Expression AST and constant evaluation for assembler directives and operands.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    /// Positive integer literal above `i64::MAX` whose value is preserved as
    /// a 64-bit bit pattern. The parser permits these only as standalone data
    /// or directive-pattern values so relocation addends and expression math
    /// remain `i64`.
    Unsigned(u64),
    Symbol(String),
    ModifiedSymbol {
        symbol: String,
        modifier: SymbolModifier,
    },
    CurrentLocation,
    UnaryMinus(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolModifier {
    Got,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    UndefinedSymbol(String),
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolValue {
    Absolute(i64),
    Defined { section: usize, value: u64 },
    Undefined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifiedExpr {
    Absolute(i64),
    UnsignedAbsolute(u64),
    Relocatable {
        symbol: String,
        addend: i64,
    },
    Difference {
        minuend: String,
        subtrahend: String,
        addend: i64,
    },
    PointerToGot {
        symbol: String,
        addend: i64,
        pcrel: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyError {
    Overflow,
    Illegal(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedSymbol(symbol) => write!(f, "undefined absolute symbol '{}'", symbol),
            Self::Overflow => write!(f, "expression overflows i64"),
        }
    }
}

impl std::error::Error for EvalError {}

impl fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => write!(f, "expression overflows i64"),
            Self::Illegal(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ClassifyError {}

pub fn eval_pure(expr: &Expr) -> Result<i64, EvalError> {
    eval_with_symbols(expr, &BTreeMap::new())
}

pub fn eval_with_symbols(expr: &Expr, symbols: &BTreeMap<String, i64>) -> Result<i64, EvalError> {
    match expr {
        Expr::Int(value) => Ok(*value),
        Expr::Unsigned(value) => i64::try_from(*value).map_err(|_| EvalError::Overflow),
        Expr::Symbol(symbol) => symbols
            .get(symbol)
            .copied()
            .ok_or_else(|| EvalError::UndefinedSymbol(symbol.clone())),
        Expr::ModifiedSymbol { symbol, .. } => {
            Err(EvalError::UndefinedSymbol(format!("{}@GOT", symbol)))
        }
        Expr::CurrentLocation => Err(EvalError::UndefinedSymbol(".".into())),
        Expr::UnaryMinus(inner) => eval_with_symbols(inner, symbols)?
            .checked_neg()
            .ok_or(EvalError::Overflow),
        Expr::Add(lhs, rhs) => eval_with_symbols(lhs, symbols)?
            .checked_add(eval_with_symbols(rhs, symbols)?)
            .ok_or(EvalError::Overflow),
        Expr::Sub(lhs, rhs) => eval_with_symbols(lhs, symbols)?
            .checked_sub(eval_with_symbols(rhs, symbols)?)
            .ok_or(EvalError::Overflow),
    }
}

pub fn referenced_symbols(expr: &Expr) -> Vec<String> {
    let mut symbols = Vec::new();
    collect_symbols(expr, &mut symbols);
    symbols
}

pub fn classify(
    expr: &Expr,
    symbols: &BTreeMap<String, SymbolValue>,
) -> Result<ClassifiedExpr, ClassifyError> {
    if let Expr::Unsigned(value) = expr {
        return Ok(match i64::try_from(*value) {
            Ok(value) => ClassifiedExpr::Absolute(value),
            Err(_) => ClassifiedExpr::UnsignedAbsolute(*value),
        });
    }

    let mut constant = 0i64;
    let mut terms = Vec::new();
    linearize(expr, 1, &mut constant, &mut terms)?;
    terms.retain(|(_, coeff)| *coeff != 0);

    let mut defined_groups: BTreeMap<usize, Vec<(String, u64, i32)>> = BTreeMap::new();
    let mut defined_order = Vec::new();
    let mut remaining = Vec::new();
    let mut got_terms = Vec::new();
    let mut current_location_coeff = 0;

    for (term, coeff) in terms {
        match term {
            Term::Plain(symbol) => match symbols
                .get(&symbol)
                .copied()
                .unwrap_or(SymbolValue::Undefined)
            {
                SymbolValue::Absolute(value) => {
                    constant = checked_add(constant, checked_mul(value, coeff as i64)?)?;
                }
                SymbolValue::Defined { section, value } => {
                    if !defined_groups.contains_key(&section) {
                        defined_order.push(section);
                    }
                    defined_groups
                        .entry(section)
                        .or_default()
                        .push((symbol, value, coeff));
                }
                SymbolValue::Undefined => push_plain_term(&mut remaining, symbol, coeff),
            },
            Term::Modified {
                symbol,
                modifier: SymbolModifier::Got,
            } => {
                push_plain_term(&mut got_terms, symbol, coeff);
            }
            Term::CurrentLocation => {
                current_location_coeff += coeff;
            }
        }
    }

    let mut defined_total = 0i128;
    let mut anchor_groups = Vec::new();
    for section in defined_order {
        let group = defined_groups
            .remove(&section)
            .expect("section group should exist");
        let section_sum: i32 = group.iter().map(|(_, _, coeff)| *coeff).sum();
        defined_total = defined_total
            .checked_add(defined_group_total(&group)?)
            .ok_or(ClassifyError::Overflow)?;
        match section_sum {
            0 => {}
            -1 | 1 => anchor_groups.push(DefinedAnchorGroup {
                coefficient: section_sum,
                candidates: group
                    .into_iter()
                    .map(|(symbol, value, _)| (symbol, value))
                    .collect(),
            }),
            other => {
                return Err(ClassifyError::Illegal(format!(
                    "expression has unsupported section-relative coefficient {}",
                    other
                )));
            }
        }
    }
    let defined_constant = i128::from(constant)
        .checked_add(defined_total)
        .ok_or(ClassifyError::Overflow)?;

    remaining.retain(|(_, coeff)| *coeff != 0);
    got_terms.retain(|(_, coeff)| *coeff != 0);

    if !got_terms.is_empty() || current_location_coeff != 0 {
        if !remaining.is_empty() || !anchor_groups.is_empty() {
            return Err(ClassifyError::Illegal(
                "pointer-to-GOT expression cannot be combined with plain relocatable symbols"
                    .into(),
            ));
        }
        if got_terms.len() != 1 || got_terms[0].1 != 1 {
            return Err(ClassifyError::Illegal(
                "expression is not representable as a pointer-to-GOT relocation".into(),
            ));
        }
        if current_location_coeff != 0 && current_location_coeff != -1 {
            return Err(ClassifyError::Illegal(
                "pointer-to-GOT expression may subtract current location only once".into(),
            ));
        }
        let constant = i64::try_from(defined_constant).map_err(|_| ClassifyError::Overflow)?;
        return Ok(ClassifiedExpr::PointerToGot {
            symbol: got_terms[0].0.clone(),
            addend: constant,
            pcrel: current_location_coeff == -1,
        });
    }

    if !relocation_coefficients_are_supported(&remaining, &anchor_groups) {
        return Err(ClassifyError::Illegal(
            "expression is not representable as an absolute value or relocation".into(),
        ));
    }

    let (constant, anchors) = select_defined_anchors(defined_constant, &anchor_groups)?;
    for (symbol, coefficient) in anchors {
        push_plain_term(&mut remaining, symbol, coefficient);
    }

    match remaining.as_slice() {
        [] => Ok(ClassifiedExpr::Absolute(constant)),
        [(symbol, 1)] => Ok(ClassifiedExpr::Relocatable {
            symbol: symbol.clone(),
            addend: constant,
        }),
        [(minuend, 1), (subtrahend, -1)] => Ok(ClassifiedExpr::Difference {
            minuend: minuend.clone(),
            subtrahend: subtrahend.clone(),
            addend: constant,
        }),
        [(subtrahend, -1), (minuend, 1)] => Ok(ClassifiedExpr::Difference {
            minuend: minuend.clone(),
            subtrahend: subtrahend.clone(),
            addend: constant,
        }),
        _ => Err(ClassifyError::Illegal(
            "expression is not representable as an absolute value or relocation".into(),
        )),
    }
}

fn collect_symbols(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Int(_) | Expr::Unsigned(_) => {}
        Expr::Symbol(symbol) => out.push(symbol.clone()),
        Expr::ModifiedSymbol { symbol, .. } => out.push(symbol.clone()),
        Expr::CurrentLocation => {}
        Expr::UnaryMinus(inner) => collect_symbols(inner, out),
        Expr::Add(lhs, rhs) | Expr::Sub(lhs, rhs) => {
            collect_symbols(lhs, out);
            collect_symbols(rhs, out);
        }
    }
}

fn linearize(
    expr: &Expr,
    sign: i32,
    constant: &mut i64,
    terms: &mut Vec<(Term, i32)>,
) -> Result<(), ClassifyError> {
    match expr {
        Expr::Int(value) => {
            let signed = if sign == 1 {
                *value
            } else {
                value.checked_neg().ok_or(ClassifyError::Overflow)?
            };
            *constant = checked_add(*constant, signed)?;
            Ok(())
        }
        Expr::Unsigned(value) => {
            let value = i64::try_from(*value).map_err(|_| ClassifyError::Overflow)?;
            let signed = if sign == 1 {
                value
            } else {
                value.checked_neg().ok_or(ClassifyError::Overflow)?
            };
            *constant = checked_add(*constant, signed)?;
            Ok(())
        }
        Expr::Symbol(symbol) => {
            push_term(terms, Term::Plain(symbol.clone()), sign);
            Ok(())
        }
        Expr::ModifiedSymbol { symbol, modifier } => {
            push_term(
                terms,
                Term::Modified {
                    symbol: symbol.clone(),
                    modifier: *modifier,
                },
                sign,
            );
            Ok(())
        }
        Expr::CurrentLocation => {
            push_term(terms, Term::CurrentLocation, sign);
            Ok(())
        }
        Expr::UnaryMinus(inner) => linearize(inner, -sign, constant, terms),
        Expr::Add(lhs, rhs) => {
            linearize(lhs, sign, constant, terms)?;
            linearize(rhs, sign, constant, terms)
        }
        Expr::Sub(lhs, rhs) => {
            linearize(lhs, sign, constant, terms)?;
            linearize(rhs, -sign, constant, terms)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    Plain(String),
    Modified {
        symbol: String,
        modifier: SymbolModifier,
    },
    CurrentLocation,
}

fn push_term(terms: &mut Vec<(Term, i32)>, term: Term, delta: i32) {
    if let Some((_, coeff)) = terms.iter_mut().find(|(existing, _)| *existing == term) {
        *coeff += delta;
    } else {
        terms.push((term, delta));
    }
}

fn push_plain_term(terms: &mut Vec<(String, i32)>, symbol: String, delta: i32) {
    if let Some((_, coeff)) = terms.iter_mut().find(|(name, _)| *name == symbol) {
        *coeff += delta;
    } else {
        terms.push((symbol, delta));
    }
}

fn defined_group_total(group: &[(String, u64, i32)]) -> Result<i128, ClassifyError> {
    group.iter().try_fold(0i128, |total, (_, value, coeff)| {
        let term = i128::from(*value)
            .checked_mul(i128::from(*coeff))
            .ok_or(ClassifyError::Overflow)?;
        total.checked_add(term).ok_or(ClassifyError::Overflow)
    })
}

struct DefinedAnchorGroup {
    coefficient: i32,
    candidates: Vec<(String, u64)>,
}

fn relocation_coefficients_are_supported(
    remaining: &[(String, i32)],
    anchor_groups: &[DefinedAnchorGroup],
) -> bool {
    let coefficients: Vec<_> = remaining
        .iter()
        .map(|(_, coefficient)| *coefficient)
        .chain(anchor_groups.iter().map(|group| group.coefficient))
        .collect();
    matches!(coefficients.as_slice(), [] | [1] | [1, -1] | [-1, 1])
}

fn select_defined_anchors(
    defined_constant: i128,
    anchor_groups: &[DefinedAnchorGroup],
) -> Result<(i64, Vec<(String, i32)>), ClassifyError> {
    // Keep the source's first viable anchor, then try equivalent anchors when
    // high section offsets would make that choice overflow the signed addend.
    let convert = |value: i128| i64::try_from(value).map_err(|_| ClassifyError::Overflow);
    match anchor_groups {
        [] => Ok((convert(defined_constant)?, Vec::new())),
        [group] => {
            for (symbol, value) in &group.candidates {
                let adjusted = defined_constant
                    .checked_sub(i128::from(group.coefficient) * i128::from(*value))
                    .ok_or(ClassifyError::Overflow)?;
                if let Ok(constant) = convert(adjusted) {
                    return Ok((constant, vec![(symbol.clone(), group.coefficient)]));
                }
            }
            Err(ClassifyError::Overflow)
        }
        [first, second] => {
            for (first_symbol, first_value) in &first.candidates {
                let first_adjusted = defined_constant
                    .checked_sub(i128::from(first.coefficient) * i128::from(*first_value))
                    .ok_or(ClassifyError::Overflow)?;
                for (second_symbol, second_value) in &second.candidates {
                    let adjusted = first_adjusted
                        .checked_sub(i128::from(second.coefficient) * i128::from(*second_value))
                        .ok_or(ClassifyError::Overflow)?;
                    if let Ok(constant) = convert(adjusted) {
                        return Ok((
                            constant,
                            vec![
                                (first_symbol.clone(), first.coefficient),
                                (second_symbol.clone(), second.coefficient),
                            ],
                        ));
                    }
                }
            }
            Err(ClassifyError::Overflow)
        }
        _ => unreachable!("supported relocations use at most two defined anchors"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AbsoluteAssignmentError {
    UndefinedSymbol {
        assignment: usize,
        owner: String,
        symbol: String,
    },
    CyclicDefinition {
        assignment: usize,
        symbol: String,
    },
    NonAbsolute {
        assignment: usize,
        symbol: String,
    },
    InvalidExpression {
        assignment: usize,
        owner: String,
        error: ClassifyError,
    },
}

impl AbsoluteAssignmentError {
    /// Whether the parser should DEFER this failure rather than report it.
    ///
    /// The parse-time preview evaluates every `.set` before any label exists,
    /// so it cannot tell an absolute symbol from a label. Two failures are
    /// therefore provisional: a symbol it has never seen, and one that
    /// resolves to an address rather than a value -- `.set alias, real` is the
    /// second, and is a symbol ALIAS once labels are known. Both are decided
    /// again in the assembler, where `self.labels` is final; a failure that is
    /// still real there is reported with the same text and its own location.
    pub(crate) fn may_resolve_with_labels(&self) -> bool {
        matches!(
            self,
            Self::UndefinedSymbol { .. } | Self::NonAbsolute { .. }
        )
    }

    pub(crate) fn assignment_index(&self) -> usize {
        match self {
            Self::UndefinedSymbol { assignment, .. }
            | Self::CyclicDefinition { assignment, .. }
            | Self::NonAbsolute { assignment, .. }
            | Self::InvalidExpression { assignment, .. } => *assignment,
        }
    }
}

impl fmt::Display for AbsoluteAssignmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedSymbol { owner, symbol, .. } => write!(
                f,
                "absolute symbol '{}' references undefined symbol '{}'",
                owner, symbol
            ),
            Self::CyclicDefinition { symbol, .. } => {
                write!(f, "absolute symbol '{}' has a cyclic definition", symbol)
            }
            Self::NonAbsolute { symbol, .. } => {
                write!(
                    f,
                    "absolute symbol '{}' must resolve to an absolute value",
                    symbol
                )
            }
            Self::InvalidExpression { owner, error, .. } => {
                write!(f, "absolute symbol '{}': {}", owner, error)
            }
        }
    }
}

impl std::error::Error for AbsoluteAssignmentError {}

pub(crate) fn resolve_absolute_assignments(
    assignments: &[(String, Expr)],
    base_symbols: &BTreeMap<String, SymbolValue>,
) -> Vec<Result<i64, AbsoluteAssignmentError>> {
    AbsoluteAssignmentResolver::new(assignments, base_symbols).resolve_all()
}

struct AbsoluteAssignmentResolver<'a> {
    assignments: &'a [(String, Expr)],
    dependencies: Vec<Vec<ReferencedAssignmentSymbol>>,
    states: Vec<AssignmentResolutionState>,
}

#[derive(Debug, Clone)]
struct ReferencedAssignmentSymbol {
    name: String,
    dependency: AssignmentDependency,
}

#[derive(Debug, Clone, Copy)]
enum AssignmentDependency {
    Assignment(usize),
    Base(SymbolValue),
    Undefined,
}

#[derive(Debug, Clone)]
enum AssignmentResolutionState {
    Unvisited,
    Visiting,
    Resolved(Result<i64, AbsoluteAssignmentError>),
}

#[derive(Debug, Clone, Copy)]
struct AssignmentResolutionFrame {
    index: usize,
    next_dependency: usize,
}

impl<'a> AbsoluteAssignmentResolver<'a> {
    fn new(
        assignments: &'a [(String, Expr)],
        base_symbols: &'a BTreeMap<String, SymbolValue>,
    ) -> Self {
        let mut positions: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, (name, _)) in assignments.iter().enumerate() {
            positions.entry(name.clone()).or_default().push(index);
        }
        let dependencies = assignments
            .iter()
            .enumerate()
            .map(|(index, (_, expression))| {
                let mut seen = BTreeSet::new();
                referenced_symbols(expression)
                    .into_iter()
                    .filter(|symbol| seen.insert(symbol.clone()))
                    .map(|name| ReferencedAssignmentSymbol {
                        dependency: Self::dependency_for_symbol(
                            &name,
                            index,
                            &positions,
                            base_symbols,
                        ),
                        name,
                    })
                    .collect()
            })
            .collect();
        Self {
            assignments,
            dependencies,
            states: vec![AssignmentResolutionState::Unvisited; assignments.len()],
        }
    }

    fn dependency_for_symbol(
        symbol: &str,
        assignment_index: usize,
        positions: &BTreeMap<String, Vec<usize>>,
        base_symbols: &BTreeMap<String, SymbolValue>,
    ) -> AssignmentDependency {
        if let Some(indices) = positions.get(symbol) {
            let prior_count = indices.partition_point(|index| *index < assignment_index);
            let target = if prior_count > 0 {
                Some(indices[prior_count - 1])
            } else {
                indices.get(prior_count).copied()
            };
            if let Some(target) = target {
                return AssignmentDependency::Assignment(target);
            }
        }

        base_symbols
            .get(symbol)
            .copied()
            .map(AssignmentDependency::Base)
            .unwrap_or(AssignmentDependency::Undefined)
    }

    fn resolve_all(mut self) -> Vec<Result<i64, AbsoluteAssignmentError>> {
        for index in 0..self.assignments.len() {
            self.resolve_assignment(index);
        }
        self.states
            .into_iter()
            .map(|state| match state {
                AssignmentResolutionState::Resolved(value) => value,
                AssignmentResolutionState::Unvisited | AssignmentResolutionState::Visiting => {
                    unreachable!("every absolute assignment was visited")
                }
            })
            .collect()
    }

    fn resolve_assignment(&mut self, root: usize) {
        if !matches!(self.states[root], AssignmentResolutionState::Unvisited) {
            return;
        }

        self.states[root] = AssignmentResolutionState::Visiting;
        let mut stack = vec![AssignmentResolutionFrame {
            index: root,
            next_dependency: 0,
        }];

        while let Some(frame) = stack.last().copied() {
            let Some(reference) = self.dependencies[frame.index]
                .get(frame.next_dependency)
                .cloned()
            else {
                let result = self.evaluate_assignment(frame.index);
                self.states[frame.index] = AssignmentResolutionState::Resolved(result);
                stack.pop();
                continue;
            };

            match reference.dependency {
                AssignmentDependency::Base(_) => {
                    stack.last_mut().expect("resolution frame").next_dependency += 1;
                }
                AssignmentDependency::Undefined => {
                    stack.last_mut().expect("resolution frame").next_dependency += 1;
                }
                AssignmentDependency::Assignment(target) => match self.states[target].clone() {
                    AssignmentResolutionState::Unvisited => {
                        self.states[target] = AssignmentResolutionState::Visiting;
                        stack.push(AssignmentResolutionFrame {
                            index: target,
                            next_dependency: 0,
                        });
                    }
                    AssignmentResolutionState::Visiting => {
                        let error = AbsoluteAssignmentError::CyclicDefinition {
                            assignment: target,
                            symbol: self.assignments[target].0.clone(),
                        };
                        for frame in stack.drain(..) {
                            self.states[frame.index] =
                                AssignmentResolutionState::Resolved(Err(error.clone()));
                        }
                    }
                    AssignmentResolutionState::Resolved(Ok(_)) => {
                        stack.last_mut().expect("resolution frame").next_dependency += 1;
                    }
                    AssignmentResolutionState::Resolved(Err(error)) => {
                        self.states[frame.index] = AssignmentResolutionState::Resolved(Err(error));
                        stack.pop();
                    }
                },
            }
        }
    }

    fn evaluate_assignment(&self, index: usize) -> Result<i64, AbsoluteAssignmentError> {
        let mut symbols = BTreeMap::new();
        let mut unresolved = Vec::new();
        for reference in &self.dependencies[index] {
            let value = match reference.dependency {
                AssignmentDependency::Assignment(target) => match &self.states[target] {
                    AssignmentResolutionState::Resolved(Ok(value)) => SymbolValue::Absolute(*value),
                    AssignmentResolutionState::Resolved(Err(error)) => return Err(error.clone()),
                    AssignmentResolutionState::Unvisited | AssignmentResolutionState::Visiting => {
                        unreachable!("assignment dependencies resolve before evaluation")
                    }
                },
                AssignmentDependency::Base(value) => value,
                AssignmentDependency::Undefined => {
                    unresolved.push(reference.name.clone());
                    SymbolValue::Undefined
                }
            };
            symbols.insert(reference.name.clone(), value);
        }

        let name = &self.assignments[index].0;
        match classify(&self.assignments[index].1, &symbols) {
            Ok(ClassifiedExpr::Absolute(value)) => Ok(value),
            Ok(ClassifiedExpr::UnsignedAbsolute(value)) => {
                Ok(i64::from_le_bytes(value.to_le_bytes()))
            }
            Err(ClassifyError::Overflow) => Err(AbsoluteAssignmentError::InvalidExpression {
                assignment: index,
                owner: name.clone(),
                error: ClassifyError::Overflow,
            }),
            _ if !unresolved.is_empty()
                && self.can_resolve_with_labels(index, &symbols, &unresolved) =>
            {
                Err(AbsoluteAssignmentError::UndefinedSymbol {
                    assignment: index,
                    owner: name.clone(),
                    symbol: unresolved[0].clone(),
                })
            }
            Ok(_) => Err(AbsoluteAssignmentError::NonAbsolute {
                assignment: index,
                symbol: name.clone(),
            }),
            Err(error) => Err(AbsoluteAssignmentError::InvalidExpression {
                assignment: index,
                owner: name.clone(),
                error,
            }),
        }
    }

    fn can_resolve_with_labels(
        &self,
        index: usize,
        symbols: &BTreeMap<String, SymbolValue>,
        unresolved: &[String],
    ) -> bool {
        let mut defined = symbols.clone();
        for symbol in unresolved {
            defined.insert(
                symbol.clone(),
                SymbolValue::Defined {
                    section: usize::MAX,
                    value: 0,
                },
            );
        }
        matches!(
            classify(&self.assignments[index].1, &defined),
            Ok(ClassifiedExpr::Absolute(_))
        )
    }
}

fn checked_add(lhs: i64, rhs: i64) -> Result<i64, ClassifyError> {
    lhs.checked_add(rhs).ok_or(ClassifyError::Overflow)
}

fn checked_mul(lhs: i64, rhs: i64) -> Result<i64, ClassifyError> {
    lhs.checked_mul(rhs).ok_or(ClassifyError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_pure_constant_expression() {
        let expr = Expr::Sub(
            Box::new(Expr::Add(Box::new(Expr::Int(1)), Box::new(Expr::Int(2)))),
            Box::new(Expr::Int(3)),
        );
        assert_eq!(eval_pure(&expr).unwrap(), 0);
    }

    #[test]
    fn eval_symbolic_expression() {
        let expr = Expr::Add(Box::new(Expr::Symbol("foo".into())), Box::new(Expr::Int(4)));
        let mut symbols = BTreeMap::new();
        symbols.insert("foo".into(), 8);
        assert_eq!(eval_with_symbols(&expr, &symbols).unwrap(), 12);
    }

    #[test]
    fn eval_undefined_symbol_errors() {
        let err = eval_pure(&Expr::Symbol("foo".into())).unwrap_err();
        assert_eq!(err, EvalError::UndefinedSymbol("foo".into()));
    }

    #[test]
    fn wide_unsigned_literal_is_preserved_only_as_an_absolute() {
        let value = u64::MAX;
        assert_eq!(
            classify(&Expr::Unsigned(value), &BTreeMap::new()).unwrap(),
            ClassifiedExpr::UnsignedAbsolute(value)
        );
        assert_eq!(eval_pure(&Expr::Unsigned(value)), Err(EvalError::Overflow));

        let expression = Expr::Add(Box::new(Expr::Unsigned(value)), Box::new(Expr::Int(1)));
        assert_eq!(
            classify(&expression, &BTreeMap::new()),
            Err(ClassifyError::Overflow)
        );
    }

    #[test]
    fn classify_relocatable_symbol_plus_constant() {
        let expr = Expr::Add(Box::new(Expr::Symbol("foo".into())), Box::new(Expr::Int(4)));
        let mut symbols = BTreeMap::new();
        symbols.insert(
            "foo".into(),
            SymbolValue::Defined {
                section: 1,
                value: 12,
            },
        );
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::Relocatable {
                symbol: "foo".into(),
                addend: 4
            }
        );
    }

    #[test]
    fn classify_same_section_difference_as_absolute() {
        let expr = Expr::Sub(
            Box::new(Expr::Symbol("foo".into())),
            Box::new(Expr::Symbol("bar".into())),
        );
        let mut symbols = BTreeMap::new();
        symbols.insert(
            "foo".into(),
            SymbolValue::Defined {
                section: 1,
                value: 16,
            },
        );
        symbols.insert(
            "bar".into(),
            SymbolValue::Defined {
                section: 1,
                value: 24,
            },
        );
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::Absolute(-8)
        );
    }

    #[test]
    fn classify_same_section_difference_across_signed_address_boundary() {
        let difference = Expr::Sub(
            Box::new(Expr::Symbol("end".into())),
            Box::new(Expr::Symbol("start".into())),
        );
        let mut symbols = BTreeMap::from([
            (
                "start".into(),
                SymbolValue::Defined {
                    section: 1,
                    value: 1,
                },
            ),
            (
                "end".into(),
                SymbolValue::Defined {
                    section: 1,
                    value: 1_u64 << 63,
                },
            ),
        ]);
        assert_eq!(
            classify(&difference, &symbols).unwrap(),
            ClassifiedExpr::Absolute(i64::MAX)
        );

        symbols.insert(
            "start".into(),
            SymbolValue::Defined {
                section: 1,
                value: 0,
            },
        );
        assert_eq!(
            classify(&difference, &symbols),
            Err(ClassifyError::Overflow)
        );

        let adjusted = Expr::Sub(Box::new(difference), Box::new(Expr::Int(1)));
        assert_eq!(
            classify(&adjusted, &symbols).unwrap(),
            ClassifiedExpr::Absolute(i64::MAX)
        );
    }

    #[test]
    fn high_address_symbols_keep_their_relocation_class() {
        let symbols = BTreeMap::from([
            (
                "high".into(),
                SymbolValue::Defined {
                    section: 1,
                    value: u64::MAX,
                },
            ),
            (
                "other".into(),
                SymbolValue::Defined {
                    section: 2,
                    value: u64::MAX,
                },
            ),
        ]);
        assert_eq!(
            classify(&Expr::Symbol("high".into()), &symbols).unwrap(),
            ClassifiedExpr::Relocatable {
                symbol: "high".into(),
                addend: 0,
            }
        );
        assert_eq!(
            classify(
                &Expr::Sub(
                    Box::new(Expr::Symbol("high".into())),
                    Box::new(Expr::Symbol("other".into())),
                ),
                &symbols,
            )
            .unwrap(),
            ClassifiedExpr::Difference {
                minuend: "high".into(),
                subtrahend: "other".into(),
                addend: 0,
            }
        );
    }

    #[test]
    fn high_address_anchor_selection_accepts_equivalent_term_orders() {
        let symbols = BTreeMap::from([
            (
                "low1".into(),
                SymbolValue::Defined {
                    section: 1,
                    value: 0,
                },
            ),
            (
                "low2".into(),
                SymbolValue::Defined {
                    section: 1,
                    value: 0,
                },
            ),
            (
                "high".into(),
                SymbolValue::Defined {
                    section: 1,
                    value: 1_u64 << 63,
                },
            ),
        ]);
        let low_first = Expr::Sub(
            Box::new(Expr::Add(
                Box::new(Expr::Symbol("low1".into())),
                Box::new(Expr::Symbol("high".into())),
            )),
            Box::new(Expr::Symbol("low2".into())),
        );
        let high_first = Expr::Sub(
            Box::new(Expr::Add(
                Box::new(Expr::Symbol("high".into())),
                Box::new(Expr::Symbol("low1".into())),
            )),
            Box::new(Expr::Symbol("low2".into())),
        );
        let expected = ClassifiedExpr::Relocatable {
            symbol: "high".into(),
            addend: 0,
        };
        assert_eq!(classify(&low_first, &symbols).unwrap(), expected);
        assert_eq!(classify(&high_first, &symbols).unwrap(), expected);
    }

    #[test]
    fn classify_external_difference() {
        let expr = Expr::Add(
            Box::new(Expr::Sub(
                Box::new(Expr::Symbol("_foo".into())),
                Box::new(Expr::Symbol("_bar".into())),
            )),
            Box::new(Expr::Int(4)),
        );
        let symbols = BTreeMap::new();
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::Difference {
                minuend: "_foo".into(),
                subtrahend: "_bar".into(),
                addend: 4,
            }
        );
    }

    #[test]
    fn classify_section_sum_one_folds_difference_into_addend() {
        let expr = Expr::Add(
            Box::new(Expr::Symbol("foo".into())),
            Box::new(Expr::Sub(
                Box::new(Expr::Symbol("bar".into())),
                Box::new(Expr::Symbol("baz".into())),
            )),
        );
        let mut symbols = BTreeMap::new();
        symbols.insert(
            "foo".into(),
            SymbolValue::Defined {
                section: 1,
                value: 16,
            },
        );
        symbols.insert(
            "bar".into(),
            SymbolValue::Defined {
                section: 1,
                value: 24,
            },
        );
        symbols.insert(
            "baz".into(),
            SymbolValue::Defined {
                section: 1,
                value: 20,
            },
        );
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::Relocatable {
                symbol: "foo".into(),
                addend: 4
            }
        );
    }

    #[test]
    fn classify_pointer_to_got() {
        let expr = Expr::ModifiedSymbol {
            symbol: "_puts".into(),
            modifier: SymbolModifier::Got,
        };
        let symbols = BTreeMap::new();
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::PointerToGot {
                symbol: "_puts".into(),
                addend: 0,
                pcrel: false,
            }
        );
    }

    #[test]
    fn classify_pointer_to_got_pcrel() {
        let expr = Expr::Sub(
            Box::new(Expr::ModifiedSymbol {
                symbol: "_puts".into(),
                modifier: SymbolModifier::Got,
            }),
            Box::new(Expr::CurrentLocation),
        );
        let symbols = BTreeMap::new();
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::PointerToGot {
                symbol: "_puts".into(),
                addend: 0,
                pcrel: true,
            }
        );
    }

    #[test]
    fn absolute_assignments_resolve_chronologically_and_freeze_aliases() {
        let assignments = vec![
            ("A".into(), Expr::Symbol("B".into())),
            ("B".into(), Expr::Int(2)),
            ("B".into(), Expr::Int(3)),
        ];
        assert_eq!(
            resolve_absolute_assignments(&assignments, &BTreeMap::new()),
            [Ok(2), Ok(2), Ok(3)]
        );
    }

    #[test]
    fn absolute_assignment_resolution_handles_deep_alias_chains() {
        const COUNT: usize = 20_000;
        let assignments: Vec<_> = (0..COUNT)
            .map(|index| {
                let expression = if index + 1 == COUNT {
                    Expr::Int(7)
                } else {
                    Expr::Symbol(format!("X{}", index + 1))
                };
                (format!("X{index}"), expression)
            })
            .collect();
        let resolved = resolve_absolute_assignments(&assignments, &BTreeMap::new());
        assert_eq!(resolved.len(), assignments.len());
        assert!(resolved.iter().all(|value| value == &Ok(7)));
    }

    #[test]
    fn absolute_assignment_resolution_handles_deep_cycles() {
        const COUNT: usize = 20_000;
        let assignments: Vec<_> = (0..COUNT)
            .map(|index| {
                (
                    format!("X{index}"),
                    Expr::Symbol(format!("X{}", (index + 1) % COUNT)),
                )
            })
            .collect();
        let resolved = resolve_absolute_assignments(&assignments, &BTreeMap::new());
        assert_eq!(resolved.len(), assignments.len());
        assert!(resolved.iter().all(|value| matches!(
            value,
            Err(AbsoluteAssignmentError::CyclicDefinition { symbol, .. }) if symbol == "X0"
        )));
    }
}
