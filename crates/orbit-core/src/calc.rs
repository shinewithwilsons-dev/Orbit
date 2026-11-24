//! A small calculator for the command bar: + - * / % ^, brackets, and
//! sqrt, sin, cos, tan, ln, log, abs, round, pi, e.

pub fn looks_like_math(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars()
            .any(|c| "+-*/^%()".contains(c) || s.contains("sqrt") || s.contains("pi"))
        && s.chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_alphabetic() || " .+-*/^%(),x×÷".contains(c))
}

pub fn eval(input: &str) -> Option<f64> {
    let s: String = input
        .replace('×', "*")
        .replace('÷', "/")
        .replace(',', "")
        .to_lowercase();
    let mut p = Parser {
        s: s.as_bytes(),
        i: 0,
    };
    let v = p.expr()?;
    p.ws();
    if p.i == p.s.len() && v.is_finite() {
        Some(v)
    } else {
        None
    }
}

/// Format a result without trailing zeros.
pub fn format(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i] == b' ' {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') | Some(b'x') => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    v /= self.power()?;
                }
                Some(b'%') => {
                    self.i += 1;
                    v %= self.power()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let exp = self.power()?; // right-associative
            return Some(base.powf(exp));
        }
        Some(base)
    }
    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Some(-self.unary()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            b'(' => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(b')') {
                    return None;
                }
                self.i += 1;
                Some(v)
            }
            c if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.')
                {
                    self.i += 1;
                }
                // Scientific notation: 3e8
                if self.i < self.s.len()
                    && self.s[self.i] == b'e'
                    && self
                        .s
                        .get(self.i + 1)
                        .map(|c| c.is_ascii_digit() || *c == b'-')
                        .unwrap_or(false)
                {
                    self.i += 2;
                    while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                        self.i += 1;
                    }
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()?
                    .parse()
                    .ok()
            }
            c if c.is_ascii_alphabetic() => {
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[start..self.i]).ok()?;
                match name {
                    "pi" => Some(std::f64::consts::PI),
                    "e" => Some(std::f64::consts::E),
                    _ => {
                        let arg = self.atom()?;
                        Some(match name {
                            "sqrt" => arg.sqrt(),
                            "sin" => arg.sin(),
                            "cos" => arg.cos(),
                            "tan" => arg.tan(),
                            "ln" => arg.ln(),
                            "log" => arg.log10(),
                            "abs" => arg.abs(),
                            "round" => arg.round(),
                            _ => return None,
                        })
                    }
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maths() {
        assert_eq!(eval("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-3 + 5"), Some(2.0));
        assert_eq!(eval("sqrt(16) + 1"), Some(5.0));
        assert_eq!(eval("3e8 / 1000"), Some(300_000.0));
        assert_eq!(eval("10 % 4"), Some(2.0));
        assert!(eval("2 +").is_none());
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(42.0), "42");
    }

    #[test]
    fn detection() {
        assert!(looks_like_math("12*7"));
        assert!(looks_like_math("sqrt(2)"));
        assert!(!looks_like_math("firefox"));
        assert!(!looks_like_math("report 2026.docx"));
    }
}
