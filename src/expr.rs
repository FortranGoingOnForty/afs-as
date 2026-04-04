//! Expression AST and constant evaluation for assembler directives and operands.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    Symbol(String),
    UnaryMinus(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
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
    Relocatable { symbol: String, addend: i64 },
    Difference { minuend: String, subtrahend: String, addend: i64 },
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
        Expr::Symbol(symbol) => symbols
            .get(symbol)
            .copied()
            .ok_or_else(|| EvalError::UndefinedSymbol(symbol.clone())),
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
    let mut constant = 0i64;
    let mut terms = Vec::new();
    linearize(expr, 1, &mut constant, &mut terms)?;
    terms.retain(|(_, coeff)| *coeff != 0);

    let mut defined_groups: BTreeMap<usize, Vec<(String, i64, i32)>> = BTreeMap::new();
    let mut defined_order = Vec::new();
    let mut remaining = Vec::new();

    for (symbol, coeff) in terms {
        match symbols.get(&symbol).copied().unwrap_or(SymbolValue::Undefined) {
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
            SymbolValue::Undefined => push_term(&mut remaining, symbol, coeff),
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
                push_term(&mut remaining, anchor_symbol.clone(), section_sum);
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
        Expr::Int(_) => {}
        Expr::Symbol(symbol) => out.push(symbol.clone()),
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
    terms: &mut Vec<(String, i32)>,
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
        Expr::Symbol(symbol) => {
            push_term(terms, symbol.clone(), sign);
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

fn push_term(terms: &mut Vec<(String, i32)>, symbol: String, delta: i32) {
    if let Some((_, coeff)) = terms.iter_mut().find(|(name, _)| *name == symbol) {
        *coeff += delta;
    } else {
        terms.push((symbol, delta));
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
    fn classify_relocatable_symbol_plus_constant() {
        let expr = Expr::Add(Box::new(Expr::Symbol("foo".into())), Box::new(Expr::Int(4)));
        let mut symbols = BTreeMap::new();
        symbols.insert("foo".into(), SymbolValue::Defined { section: 1, value: 12 });
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::Relocatable { symbol: "foo".into(), addend: 4 }
        );
    }

    #[test]
    fn classify_same_section_difference_as_absolute() {
        let expr = Expr::Sub(Box::new(Expr::Symbol("foo".into())), Box::new(Expr::Symbol("bar".into())));
        let mut symbols = BTreeMap::new();
        symbols.insert("foo".into(), SymbolValue::Defined { section: 1, value: 16 });
        symbols.insert("bar".into(), SymbolValue::Defined { section: 1, value: 24 });
        assert_eq!(classify(&expr, &symbols).unwrap(), ClassifiedExpr::Absolute(-8));
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
        symbols.insert("foo".into(), SymbolValue::Defined { section: 1, value: 16 });
        symbols.insert("bar".into(), SymbolValue::Defined { section: 1, value: 24 });
        symbols.insert("baz".into(), SymbolValue::Defined { section: 1, value: 20 });
        assert_eq!(
            classify(&expr, &symbols).unwrap(),
            ClassifiedExpr::Relocatable { symbol: "foo".into(), addend: 4 }
        );
    }
}
