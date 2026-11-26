pub fn evaluate(text: &str, decimal_comma: bool) -> Option<String> {
    let tokens = tokens(text, decimal_comma)?;
    let mut parser = Parser { tokens: &tokens, at: 0, operations: 0, depth: 0 };
    let value = parser.sum()?;
    let shown = format(value);
    let comma = decimal_comma && text.contains(',');
    (parser.at == tokens.len() && parser.operations > 0 && value.is_finite())
        .then(|| if comma { shown.replace('.', ",") } else { shown })
}

const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Times,
    Divide,
    Remainder,
    Power,
    Open,
    Close,
}

fn tokens(text: &str, decimal_comma: bool) -> Option<Vec<Token>> {
    let point = |ch: char| ch == '.' || (decimal_comma && ch == ',');
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let ch = chars[at];
        let token = match ch {
            _ if ch.is_whitespace() => {
                at += 1;
                continue;
            }
            _ if ch.is_ascii_digit() || point(ch) => {
                let start = at;
                while at < chars.len() && (chars[at].is_ascii_digit() || point(chars[at])) {
                    at += 1;
                }
                let number: String = chars[start..at].iter().map(|&ch| if point(ch) { '.' } else { ch }).collect();
                if number == "." || number.matches('.').count() > 1 {
                    return None;
                }
                found.push(Token::Number(number.parse().ok()?));
                continue;
            }
            '+' => Token::Plus,
            '-' | '−' => Token::Minus,
            '*' if chars.get(at + 1) == Some(&'*') => {
                at += 1;
                Token::Power
            }
            '*' | '×' => Token::Times,
            '/' | '÷' => Token::Divide,
            '%' => Token::Remainder,
            '^' => Token::Power,
            '(' => Token::Open,
            ')' => Token::Close,
            _ => return None,
        };
        found.push(token);
        at += 1;
    }
    Some(found)
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    operations: usize,
    depth: usize,
}

impl Parser<'_> {
    fn next_if(&mut self, wanted: &[Token]) -> Option<Token> {
        let token = *self.tokens.get(self.at)?;
        wanted.contains(&token).then(|| {
            self.at += 1;
            token
        })
    }

    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        while let Some(token) = self.next_if(&[Token::Plus, Token::Minus]) {
            let right = self.product()?;
            self.operations += 1;
            value = if token == Token::Plus { value + right } else { value - right };
        }
        Some(value)
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.signed()?;
        while let Some(token) = self.next_if(&[Token::Times, Token::Divide, Token::Remainder]) {
            let right = self.signed()?;
            self.operations += 1;
            value = match token {
                Token::Times => value * right,
                Token::Divide => value / right,
                _ => value % right,
            };
        }
        Some(value)
    }

    fn signed(&mut self) -> Option<f64> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let value = match self.next_if(&[Token::Plus, Token::Minus]) {
            Some(Token::Minus) => self.signed().map(|value| -value),
            Some(_) => self.signed(),
            None => self.power(),
        };
        self.depth -= 1;
        value
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.next_if(&[Token::Power]).is_some() {
            let exponent = self.signed()?;
            self.operations += 1;
            return Some(base.powf(exponent));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        match *self.tokens.get(self.at)? {
            Token::Number(value) => {
                self.at += 1;
                Some(value)
            }
            Token::Open => {
                self.at += 1;
                let value = self.sum()?;
                self.next_if(&[Token::Close])?;
                Some(value)
            }
            _ => None,
        }
    }
}

const DIGITS: i32 = 12;

fn format(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs().log10().floor() as i32;
    if !(-6..15).contains(&magnitude) {
        return format!("{value:e}");
    }
    let decimals = usize::try_from((DIGITS - 1 - magnitude).max(0)).unwrap_or(0);
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
    if text == "-0" { "0".to_owned() } else { text.to_owned() }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn evaluate_point(text: &str) -> Option<String> {
        evaluate(text, false)
    }

    #[test]
    fn arithmetic_follows_the_usual_order() {
        assert_eq!(evaluate_point("2 + 3 * 4").as_deref(), Some("14"));
        assert_eq!(evaluate_point("(2+3)*4").as_deref(), Some("20"));
        assert_eq!(evaluate_point("2^3^2").as_deref(), Some("512"));
        assert_eq!(evaluate_point("2 ** 10").as_deref(), Some("1024"));
        assert_eq!(evaluate_point("-2^2").as_deref(), Some("-4"));
        assert_eq!(evaluate_point("17 % 5").as_deref(), Some("2"));
        assert_eq!(evaluate_point("10 / 4").as_deref(), Some("2.5"));
        assert_eq!(evaluate_point("1/3").as_deref(), Some("0.333333333333"));
        assert_eq!(evaluate_point("0.1 + 0.2").as_deref(), Some("0.3"));
        assert_eq!(evaluate_point("3 - -2").as_deref(), Some("5"));
        assert_eq!(evaluate_point("6 × 7 ÷ 2").as_deref(), Some("21"));
        assert_eq!(evaluate_point("1e3").as_deref(), None);
        assert_eq!(evaluate_point("2^60").as_deref(), Some("1.152921504606847e18"));
    }

    #[test]
    fn what_is_not_an_expression_has_no_value() {
        for text in ["", "42", "-5", "(7)", "main.go:42", "a+b", "1/0", "2 +", "(1+2", "1..2+3", "1 2"] {
            assert_eq!(evaluate_point(text), None, "{text}");
        }
    }

    #[test]
    fn huge_deep_and_undefined_expressions_have_no_value_and_do_not_panic() {
        let huge = format!("{} * 10", "9".repeat(400));
        let deep = format!("{}1+1{}", "(".repeat(100_000), ")".repeat(100_000));
        let signs = format!("{}1+1", "-".repeat(100_000));
        let powers = format!("2{}", "^2".repeat(100_000));
        for text in [huge.as_str(), &deep, &signs, &powers, "10^400", "0/0", "5 % 0", "(-1) ^ 0.5", "1e308*10"] {
            assert_eq!(evaluate_point(text), None, "{}", &text[..text.len().min(40)]);
        }
        let nested = format!("{}1+1{}", "(".repeat(30), ")".repeat(30));
        assert_eq!(evaluate_point(&nested).as_deref(), Some("2"), "reasonable nesting is fine");
    }

    #[test]
    fn a_decimal_comma_is_read_where_the_language_writes_one() {
        assert_eq!(evaluate("1,5 + 2", true).as_deref(), Some("3,5"));
        assert_eq!(evaluate("1,5 * 2", true).as_deref(), Some("3"));
        assert_eq!(evaluate("1.5 + 2", true).as_deref(), Some("3.5"));
        assert_eq!(evaluate("1,5 + 2", false), None, "a comma is no decimal point in English");
        assert_eq!(evaluate("1,5,5 + 2", true), None);
    }

    proptest! {
        #[test]
        fn any_text_evaluates_without_panicking(text in "[0-9.,+*/%^()× −-]{0,64}") {
            let _ = evaluate(&text, true);
            let _ = evaluate(&text, false);
        }

        #[test]
        fn sums_and_products_of_integers_are_exact(a in -9_999i64..9_999, b in -9_999i64..9_999, c in 1i64..999) {
            let text = format!("{a} + {b} * {c}");
            prop_assert_eq!(evaluate(&text, false), Some((a + b * c).to_string()));
            let text = format!("({a} + {b}) * {c}");
            prop_assert_eq!(evaluate(&text, false), Some(((a + b) * c).to_string()));
            let text = format!("{a} - {b} % {c}");
            prop_assert_eq!(evaluate(&text, false), Some((a - b % c).to_string()));
        }
    }
}
