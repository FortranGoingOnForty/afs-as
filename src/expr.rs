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

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedSymbol(symbol) => write!(f, "undefined absolute symbol '{}'", symbol),
            Self::Overflow => write!(f, "expression overflows i64"),
        }
    }
}

impl std::error::Error for EvalError {}

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
}
