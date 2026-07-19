//! Expression AST and constant evaluation for assembler directives and operands.

use std::collections::BTreeMap;
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
    Defined { section: usize, value: i64 },
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

    let mut defined_groups: BTreeMap<usize, Vec<(String, i64, i32)>> = BTreeMap::new();
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

    for section in defined_order {
        let group = defined_groups
            .remove(&section)
            .expect("section group should exist");
        let section_sum: i32 = group.iter().map(|(_, _, coeff)| *coeff).sum();
        match section_sum {
            0 => {
                for (_, value, coeff) in group {
                    constant = checked_add(constant, checked_mul(value, coeff as i64)?)?;
                }
            }
            1 | -1 => {
                let (anchor_symbol, anchor_value, _) = &group[0];
                for (_, value, coeff) in &group {
                    constant = checked_add(
                        constant,
                        checked_mul(checked_sub(*value, *anchor_value)?, *coeff as i64)?,
                    )?;
                }
                push_plain_term(&mut remaining, anchor_symbol.clone(), section_sum);
            }
            other => {
                return Err(ClassifyError::Illegal(format!(
                    "expression has unsupported section-relative coefficient {}",
                    other
                )));
            }
        }
    }

    remaining.retain(|(_, coeff)| *coeff != 0);
    got_terms.retain(|(_, coeff)| *coeff != 0);

    if !got_terms.is_empty() || current_location_coeff != 0 {
        if !remaining.is_empty() {
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
        return Ok(ClassifiedExpr::PointerToGot {
            symbol: got_terms[0].0.clone(),
            addend: constant,
            pcrel: current_location_coeff == -1,
        });
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

pub(crate) fn resolve_absolute_assignments(
    assignments: &[(String, Expr)],
    base_symbols: &BTreeMap<String, SymbolValue>,
) -> Vec<Result<i64, String>> {
    AbsoluteAssignmentResolver::new(assignments, base_symbols).resolve_all()
}

struct AbsoluteAssignmentResolver<'a> {
    assignments: &'a [(String, Expr)],
    base_symbols: &'a BTreeMap<String, SymbolValue>,
    positions: BTreeMap<String, Vec<usize>>,
    resolved: Vec<Option<Result<i64, String>>>,
    visiting: Vec<usize>,
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
        Self {
            assignments,
            base_symbols,
            positions,
            resolved: vec![None; assignments.len()],
            visiting: Vec::new(),
        }
    }

    fn resolve_all(mut self) -> Vec<Result<i64, String>> {
        for index in 0..self.assignments.len() {
            let _ = self.resolve_assignment(index);
        }
        self.resolved
            .into_iter()
            .map(|value| value.expect("every absolute assignment was visited"))
            .collect()
    }

    fn resolve_assignment(&mut self, index: usize) -> Result<i64, String> {
        if let Some(value) = &self.resolved[index] {
            return value.clone();
        }

        let name = self.assignments[index].0.clone();
        if self.visiting.contains(&index) {
            return Err(format!(
                "absolute symbol '{}' has a cyclic definition",
                name
            ));
        }

        self.visiting.push(index);
        let result = self.resolve_assignment_expr(index, &name);
        self.visiting.pop();
        self.resolved[index] = Some(result.clone());
        result
    }

    fn resolve_assignment_expr(&mut self, index: usize, name: &str) -> Result<i64, String> {
        let references = referenced_symbols(&self.assignments[index].1);
        let mut symbols = BTreeMap::new();
        for referenced in references {
            if symbols.contains_key(&referenced) {
                continue;
            }
            let value = self.resolve_symbol_before_assignment(&referenced, index, name)?;
            symbols.insert(referenced, value);
        }

        match classify(&self.assignments[index].1, &symbols) {
            Ok(ClassifiedExpr::Absolute(value)) => Ok(value),
            Ok(_) => Err(format!(
                "absolute symbol '{}' must resolve to an absolute value",
                name
            )),
            Err(error) => Err(format!("absolute symbol '{}': {}", name, error)),
        }
    }

    fn resolve_symbol_before_assignment(
        &mut self,
        symbol: &str,
        assignment_index: usize,
        owner: &str,
    ) -> Result<SymbolValue, String> {
        if let Some(indices) = self.positions.get(symbol) {
            let prior_count = indices.partition_point(|index| *index < assignment_index);
            let target = if prior_count > 0 {
                Some(indices[prior_count - 1])
            } else {
                let future = indices.partition_point(|index| *index <= assignment_index);
                indices.get(future).copied()
            };
            if let Some(target) = target {
                return self.resolve_assignment(target).map(SymbolValue::Absolute);
            }
        }

        self.base_symbols.get(symbol).copied().ok_or_else(|| {
            format!(
                "absolute symbol '{}' references undefined symbol '{}'",
                owner, symbol
            )
        })
    }
}

fn checked_add(lhs: i64, rhs: i64) -> Result<i64, ClassifyError> {
    lhs.checked_add(rhs).ok_or(ClassifyError::Overflow)
}

fn checked_sub(lhs: i64, rhs: i64) -> Result<i64, ClassifyError> {
    lhs.checked_sub(rhs).ok_or(ClassifyError::Overflow)
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
    fn absolute_assignment_resolution_handles_large_linear_timelines() {
        let assignments: Vec<_> = (0..10_000)
            .map(|index| (format!("X{index}"), Expr::Int(index)))
            .collect();
        let resolved = resolve_absolute_assignments(&assignments, &BTreeMap::new());
        assert_eq!(resolved.len(), assignments.len());
        assert_eq!(resolved.last(), Some(&Ok(9_999)));
    }
}
