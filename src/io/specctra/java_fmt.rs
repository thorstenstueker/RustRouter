//! Java compatible number formatting (`Double.toString`, `BigDecimal.toPlainString`).

/// Sign, shortest decimal digits and decimal exponent of the first digit of `d`
/// (`d = 0.d1d2... * 10^(exp + 1)`). `d` must be finite and non zero.
fn shortest_digits(d: f64) -> (bool, String, i32) {
    let s = format!("{:e}", d);
    let (mantissa, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    (negative, digits, exp)
}

/// `String.valueOf(double)` / `Double.toString(double)` of Java (JDK 19+, shortest repr).
pub fn java_double_to_string(d: f64) -> String {
    if d.is_nan() {
        return "NaN".to_string();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let (negative, digits, exp) = shortest_digits(d);
    let mut r = String::new();
    if negative {
        r.push('-');
    }
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                r.push_str(&digits);
                for _ in digits.len()..int_len {
                    r.push('0');
                }
                r.push_str(".0");
            } else {
                r.push_str(&digits[..int_len]);
                r.push('.');
                r.push_str(&digits[int_len..]);
            }
        } else {
            r.push_str("0.");
            for _ in 0..(-exp - 1) {
                r.push('0');
            }
            r.push_str(&digits);
        }
    } else {
        r.push_str(&digits[..1]);
        r.push('.');
        if digits.len() > 1 {
            r.push_str(&digits[1..]);
        } else {
            r.push('0');
        }
        r.push('E');
        r.push_str(&exp.to_string());
    }
    r
}

/// `BigDecimal.valueOf(d).stripTrailingZeros().toPlainString()`.
pub fn java_plain_string(d: f64) -> String {
    if d == 0.0 || !d.is_finite() {
        // BigDecimal.valueOf throws for non finite values; callers check before.
        return "0".to_string();
    }
    let (negative, digits, exp) = shortest_digits(d);
    let digits = digits.trim_end_matches('0');
    let mut r = String::new();
    if negative {
        r.push('-');
    }
    let len = digits.len() as i32;
    if exp >= len - 1 {
        r.push_str(digits);
        for _ in 0..(exp - (len - 1)) {
            r.push('0');
        }
    } else if exp >= 0 {
        let int_len = exp as usize + 1;
        r.push_str(&digits[..int_len]);
        r.push('.');
        r.push_str(&digits[int_len..]);
    } else {
        r.push_str("0.");
        for _ in 0..(-exp - 1) {
            r.push('0');
        }
        r.push_str(digits);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_to_string() {
        assert_eq!(java_double_to_string(1.0), "1.0");
        assert_eq!(java_double_to_string(100.0), "100.0");
        assert_eq!(java_double_to_string(-2.25), "-2.25");
        assert_eq!(java_double_to_string(1.0e7), "1.0E7");
        assert_eq!(java_double_to_string(1.234e-5), "1.234E-5");
        assert_eq!(java_double_to_string(0.001), "0.001");
        assert_eq!(java_double_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(java_double_to_string(123456.75), "123456.75");
    }

    #[test]
    fn plain_string() {
        assert_eq!(java_plain_string(90.0), "90");
        assert_eq!(java_plain_string(338.5), "338.5");
        assert_eq!(java_plain_string(0.0), "0");
        assert_eq!(java_plain_string(1.5e-5), "0.000015");
        assert_eq!(java_plain_string(1.0e7), "10000000");
        assert_eq!(java_plain_string(-12.25), "-12.25");
    }
}
