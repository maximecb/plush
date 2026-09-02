/// Produce a string with comma separator for thousands for an integer
pub fn thousands_sep<T: ToString>(n: T) -> String
{
    let num_str = n.to_string();

    // The sign is set aside, so that it can't be mistaken for a digit and
    // get a separator of its own
    let (sign, digits) = match num_str.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("", num_str.as_str()),
    };

    let digit_chars: Vec<char> = digits.chars().rev().collect();

    let mut chars_sep = Vec::new();

    for idx in 0..digit_chars.len() {
        if idx > 0 && (idx % 3) == 0 {
            chars_sep.push(',');
        }
        chars_sep.push(digit_chars[idx]);
    }

    let sep_str: String = chars_sep.into_iter().rev().collect();

    sign.to_string() + &sep_str
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn thousands()
    {
        assert_eq!(thousands_sep(0), "0");
        assert_eq!(thousands_sep(7), "7");
        assert_eq!(thousands_sep(999), "999");
        assert_eq!(thousands_sep(1000), "1,000");
        assert_eq!(thousands_sep(1000000), "1,000,000");
        assert_eq!(thousands_sep(-999), "-999");
        assert_eq!(thousands_sep(-1000), "-1,000");
        assert_eq!(thousands_sep(-123456), "-123,456");
        assert_eq!(thousands_sep(i64::MIN), "-9,223,372,036,854,775,808");
    }
}
