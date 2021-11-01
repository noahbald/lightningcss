use super::length::*;
use cssparser::*;
use crate::traits::{Parse, ToCss, PropertyHandler};
use super::color::CssColor;
use crate::properties::Property;
use super::rect::Rect;
use crate::macros::*;
use crate::printer::Printer;
use std::fmt::Write;
use super::number::serialize_number;

#[derive(Debug, Clone, PartialEq)]
pub enum MathFunction<V> {
  Calc(Calc<V>),
  Min,
  Max,
  Clamp
}

impl<V: ToCss + std::cmp::PartialOrd<f32> + std::ops::Mul<f32, Output = V> + Clone + std::fmt::Debug> ToCss for MathFunction<V> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> std::fmt::Result where W: std::fmt::Write {
    match self {
      MathFunction::Calc(calc) => {
        dest.write_str("calc(")?;
        calc.to_css(dest)?;
        dest.write_char(')')
      }
      _ => todo!()
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Calc<V> {
  Value(Box<V>),
  Number(f32),
  Sum(Box<Calc<V>>, Box<Calc<V>>),
  Function(Box<MathFunction<V>>)
}

impl<V: Parse + std::ops::Mul<f32, Output = V> + std::ops::Add<V, Output = V> + std::convert::Into<Calc<V>> + std::convert::From<Calc<V>> + std::fmt::Debug> Parse for Calc<V> {
  fn parse<'i, 't>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ()>> {
    let f = input.expect_function()?;
    match_ignore_ascii_case! { &f,
      "calc" => {
        let calc = input.parse_nested_block(Calc::parse_sum)?;
        if let Calc::Value(_) = calc {
          return Ok(calc)
        }
        Ok(Calc::Function(Box::new(MathFunction::Calc(calc))))
      },
      _ => Err(input.new_error(BasicParseErrorKind::QualifiedRuleInvalid))
    }
  }
}

impl<V: Parse + std::ops::Mul<f32, Output = V> + std::ops::Add<V, Output = V> + std::convert::Into<Calc<V>> + std::convert::From<Calc<V>> + std::fmt::Debug> Calc<V> {
  fn parse_sum<'i, 't>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ()>> {
    let mut cur: Calc<V> = Calc::parse_product(input)?;
    loop {
      let start = input.state();
      match input.next_including_whitespace() {
        Ok(&Token::WhiteSpace(_)) => {
          if input.is_exhausted() {
            break; // allow trailing whitespace
          }
          match *input.next()? {
            Token::Delim('+') => {
              let next = Calc::parse_product(input)?;
              cur = cur + next;
            },
            Token::Delim('-') => {
              let mut rhs = Calc::parse_product(input)?;
              rhs = rhs * -1.0;
              cur = cur + rhs;
            },
            ref t => {
              let t = t.clone();
              return Err(input.new_unexpected_token_error(t));
            },
          }
        }
        _ => {
          input.reset(&start);
          break;
        }
      }
    }
    Ok(cur)
  }

  fn parse_product<'i, 't>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ()>> {
    let mut node = Calc::parse_value(input)?;
    loop {
      let start = input.state();
      match input.next() {
        Ok(&Token::Delim('*')) => {
          // At least one of the operands must be a number.
          let rhs = Self::parse_value(input)?;
          if let Calc::Number(val) = rhs {
            node = node * val;
          } else if let Calc::Number(val) = node {
            node = rhs;
            node = node * val;
          } else {
            return Err(input.new_unexpected_token_error(Token::Delim('*')))
          }
        }
        Ok(&Token::Delim('/')) => {
          let rhs = Self::parse_value(input)?;
          if let Calc::Number(val) = rhs {
            if val != 0.0 {
              node = node * (1.0 / val);
              continue
            }
          }
          return Err(input.new_error(BasicParseErrorKind::QualifiedRuleInvalid))
        }
        _ => {
          input.reset(&start);
          break;
        },
      }
    }
    Ok(node)
  }

  fn parse_value<'i, 't>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ()>> {
    // Parse nested calc() and other math functions.
    if let Ok(calc) = input.try_parse(Self::parse) {
      match calc {
        Calc::Function(f) => {
          return Ok(match *f {
            MathFunction::Calc(c) => c,
            _ => Calc::Function(f)
          })
        }
        c => return Ok(c)
      }
    }

    if input.try_parse(|input| input.expect_parenthesis_block()).is_ok() {
      return input.parse_nested_block(Calc::parse_sum)
    }

    if let Ok(num) = input.try_parse(|input| input.expect_number()) {
      return Ok(Calc::Number(num))
    }

    if let Ok(value) = input.try_parse(V::parse) {
      return Ok(Calc::Value(Box::new(value)))
    }

    Err(input.new_error(BasicParseErrorKind::QualifiedRuleInvalid))
  }
}

impl<V: std::ops::Mul<f32, Output = V>> std::ops::Mul<f32> for Calc<V> {
  type Output = Self;

  fn mul(self, other: f32) -> Self {
    match self {
      Calc::Value(v) => Calc::Value(Box::new(*v * other)),
      Calc::Number(n) => Calc::Number(n * other),
      Calc::Sum(a, b) => Calc::Sum(Box::new(*a * other), Box::new(*b * other)),
      Calc::Function(f) => {
        match *f {
          MathFunction::Calc(c) => Calc::Function(Box::new(MathFunction::Calc(c * other))),
          _ => todo!()
        }
      }
    }
  }
}

impl<V: std::ops::Add<V, Output = V> + std::convert::Into<Calc<V>> + std::convert::From<Calc<V>> + std::fmt::Debug> std::ops::Add<Calc<V>> for Calc<V> {
  type Output = Self;

  fn add(self, other: Calc<V>) -> Calc<V> {
    match (self, other) {
      (Calc::Value(a), Calc::Value(b)) => (*a + *b).into(),
      (Calc::Number(a), Calc::Number(b)) => Calc::Number(a + b),
      (Calc::Value(a), b) => (*a + V::from(b)).into(),
      (a, Calc::Value(b)) => (V::from(a) + *b).into(),
      (a, b) => (V::from(a) + V::from(b)).into()
    }
  }
}

impl<V: std::cmp::PartialEq<f32>> std::cmp::PartialEq<f32> for Calc<V> {
  fn eq(&self, other: &f32) -> bool {
    match self {
      Calc::Value(a) => **a == *other,
      Calc::Number(a) => *a == *other,
      _ => false
    }
  }
}

impl<V: std::cmp::PartialOrd<f32>> std::cmp::PartialOrd<f32> for Calc<V> {
  fn partial_cmp(&self, other: &f32) -> Option<std::cmp::Ordering> {
    match self {
      Calc::Value(a) => a.partial_cmp(other),
      Calc::Number(a) => a.partial_cmp(other),
      _ => None
    }
  }
}

impl<V: ToCss + std::cmp::PartialOrd<f32> + std::ops::Mul<f32, Output = V> + Clone + std::fmt::Debug> ToCss for Calc<V> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> std::fmt::Result where W: std::fmt::Write {
    match self {
      Calc::Value(v) => v.to_css(dest),
      Calc::Number(n) => serialize_number(*n, dest),
      Calc::Sum(a, b) => {
        a.to_css(dest)?;
        // Whitespace is always required.
        let b = &**b;
        if *b < 0.0 {
          dest.write_str(" - ")?;
          let b = b.clone() * -1.0;
          b.to_css(dest)?;
        } else {
          dest.write_str(" + ")?;
          b.to_css(dest)?;
        }
        Ok(())
      },
      Calc::Function(f) => f.to_css(dest)
    }
  }
}