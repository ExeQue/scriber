//! Parameter expressions: numbers, parameter names, `+ - * /` and parentheses, e.g. `d1 / 2 + 3`.

use std::collections::HashMap;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Op(char),
}

/// Whether `s` can be a parameter name: a letter or `_`, then letters, digits or `_`.
pub fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_alphabetic() || f == '_') && c.all(|ch| ch.is_alphanumeric() || ch == '_')
}

/// The tokens with their byte spans, so names can be renamed in place.
fn tokens(s: &str) -> Result<Vec<(Tok, Range<usize>)>, String> {
    let mut out = Vec::new();
    let mut rest = s.char_indices().peekable();
    while let Some((i, c)) = rest.next() {
        let mut end = i + c.len_utf8();
        let mut take = |ok: fn(char) -> bool| {
            while let Some(&(j, ch)) = rest.peek().filter(|(_, ch)| ok(*ch)) {
                end = j + ch.len_utf8();
                rest.next();
            }
            end
        };
        if c.is_whitespace() {
            continue;
        } else if c.is_ascii_digit() || c == '.' {
            let end = take(|ch| ch.is_ascii_digit() || ch == '.');
            let text = &s[i..end];
            out.push((Tok::Num(text.parse().map_err(|_| format!("\"{text}\" is not a number"))?), i..end));
        } else if c.is_alphabetic() || c == '_' {
            let end = take(|ch| ch.is_alphanumeric() || ch == '_');
            out.push((Tok::Name(s[i..end].to_string()), i..end));
        } else if "+-*/()".contains(c) {
            out.push((Tok::Op(c), i..end));
        } else {
            return Err(format!("unexpected \"{c}\""));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    t: Vec<Tok>,
    pos: usize,
    lookup: &'a mut dyn FnMut(&str) -> Result<f64, String>,
}

impl Parser<'_> {
    fn op(&mut self, ops: &str) -> Option<char> {
        match self.t.get(self.pos) {
            Some(Tok::Op(c)) if ops.contains(*c) => {
                self.pos += 1;
                Some(*c)
            }
            _ => None,
        }
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(o) = self.op("+-") {
            let r = self.term()?;
            v = if o == '+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.factor()?;
        while let Some(o) = self.op("*/") {
            let r = self.factor()?;
            if o == '/' && r == 0.0 {
                return Err("division by zero".into());
            }
            v = if o == '*' { v * r } else { v / r };
        }
        Ok(v)
    }

    fn factor(&mut self) -> Result<f64, String> {
        if self.op("-").is_some() {
            return Ok(-self.factor()?);
        }
        if self.op("+").is_some() {
            return self.factor();
        }
        if self.op("(").is_some() {
            let v = self.sum()?;
            return self.op(")").map(|_| v).ok_or_else(|| "missing \")\"".into());
        }
        let tok = self.t.get(self.pos).cloned();
        self.pos += 1;
        match tok {
            Some(Tok::Num(n)) => Ok(n),
            Some(Tok::Name(n)) => (self.lookup)(&n),
            Some(Tok::Op(c)) => Err(format!("unexpected \"{c}\"")),
            None => Err("the expression ends too early".into()),
        }
    }
}

/// Evaluate `expr`, asking `lookup` for the value of each parameter name.
pub fn eval(expr: &str, lookup: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    let t: Vec<Tok> = tokens(expr)?.into_iter().map(|t| t.0).collect();
    if t.is_empty() {
        return Err("the expression is empty".into());
    }
    let mut p = Parser { t, pos: 0, lookup };
    let v = p.sum()?;
    if p.pos < p.t.len() {
        return Err("unexpected text after the expression".into());
    }
    if !v.is_finite() {
        return Err("the result is not a number".into());
    }
    Ok(v)
}

/// The value of every parameter, given as (name, expression) pairs.
pub fn evaluate_all(params: &[(&str, &str)]) -> Result<HashMap<String, f64>, String> {
    let mut done = HashMap::new();
    for (name, _) in params {
        value(name, params, &mut done, &mut Vec::new())?;
    }
    Ok(done)
}

fn value(name: &str, params: &[(&str, &str)], done: &mut HashMap<String, f64>, stack: &mut Vec<String>) -> Result<f64, String> {
    if let Some(v) = done.get(name) {
        return Ok(*v);
    }
    if stack.iter().any(|s| s == name) {
        return Err(format!("{name} depends on itself ({} → {name})", stack.join(" → ")));
    }
    let Some(&(_, expr)) = params.iter().find(|p| p.0 == name) else { return Err(format!("there is no parameter \"{name}\"")) };
    stack.push(name.to_string());
    // Errors from deeper parameters already name them; only plain errors get this one's name.
    let v = eval(expr, &mut |n| value(n, params, done, stack)).map_err(|e| if stack.len() > 1 || e.contains("parameter") || e.contains("itself") { e } else { format!("{name} = {expr}: {e}") })?;
    stack.pop();
    done.insert(name.to_string(), v);
    Ok(v)
}

/// `expr` with every use of the parameter `old` renamed to `new`.
pub fn rename(expr: &str, old: &str, new: &str) -> String {
    let Ok(t) = tokens(expr) else { return expr.to_string() };
    let mut out = expr.to_string();
    for (tok, span) in t.into_iter().rev() {
        if tok == Tok::Name(old.to_string()) {
            out.replace_range(span, new);
        }
    }
    out
}

/// A number as short text: 40, 12.5, 0.333333.
pub fn fmt_num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions() {
        let p = [("d1", "40"), ("d2", "d1 / 2 + 3"), ("ang1", "-(d2 - 3) * 2"), ("w", "2*(d1+d2)")];
        let v = evaluate_all(&p).unwrap();
        assert_eq!((v["d2"], v["ang1"], v["w"]), (23.0, -40.0, 126.0));
        assert!(evaluate_all(&[("a", "b + 1"), ("b", "a")]).unwrap_err().contains("depends on itself"));
        assert!(evaluate_all(&[("a", "nope")]).unwrap_err().contains("no parameter"));
        assert!(eval("1/0", &mut |_| Ok(0.0)).is_err());
        assert!(eval("(1 + 2", &mut |_| Ok(0.0)).is_err());
        assert!(eval("1 2", &mut |_| Ok(0.0)).is_err());
        assert!(eval("2 $ 3", &mut |_| Ok(0.0)).is_err());
        assert_eq!(rename("d1*2 + d10 - d1", "d1", "width"), "width*2 + d10 - width");
        assert!(is_name("rad_2") && !is_name("2d") && !is_name(""));
        assert_eq!(fmt_num(40.0), "40");
        assert_eq!(fmt_num(12.50), "12.5");
    }
}
