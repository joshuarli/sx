mod tables;

pub use tables::UNICODE_VERSION;

fn within(table: &[(u32, u32)], c: char) -> bool {
    let point = u32::from(c);
    table
        .binary_search_by(|&(low, high)| {
            if high < point {
                std::cmp::Ordering::Less
            } else if low > point {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub fn is_printable(c: char) -> bool {
    !within(tables::NOT_PRINTABLE, c)
}

pub fn is_digit(c: char) -> bool {
    within(tables::DIGIT, c)
}

pub fn is_space(c: char) -> bool {
    within(tables::SPACE, c)
}

pub fn strip_space(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub fn lstrip_chars<'a>(text: &'a str, chars: &str) -> &'a str {
    text.trim_start_matches(|c| chars.contains(c))
}

pub fn strip_chars<'a>(text: &'a str, chars: &str) -> &'a str {
    text.trim_matches(|c| chars.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_follow_python() {
        assert!(is_space('\u{1c}'));
        assert!(!is_space('\u{200b}'));
        assert!(!is_printable('\u{7f}'));
        assert!(is_printable(' '));
        assert!(!is_printable('\u{a0}'));
        assert!(is_printable('\u{1F600}'));
        assert!(is_digit('\u{b2}'));
        assert!(is_digit('\u{663}'));
        assert!(!is_digit('\u{bd}'));
    }
}
