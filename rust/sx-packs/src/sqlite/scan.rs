pub fn parameters(sql: &str) -> Result<Vec<String>, String> {
    let bytes = sql.as_bytes();
    let mut names: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\'' | b'"' | b'`' => at = skip_quoted(bytes, at, byte),
            b'[' => at = skip_quoted(bytes, at, b']'),
            b'-' if bytes.get(at + 1) == Some(&b'-') => {
                at = bytes.iter().skip(at).position(|b| *b == b'\n').map_or(bytes.len(), |n| at + n);
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at = sql.get(at + 2..).and_then(|rest| rest.find("*/")).map_or(bytes.len(), |n| at + 2 + n + 2);
            }
            b':' => {
                let start = at + 1;
                let end = bytes
                    .iter()
                    .enumerate()
                    .skip(start)
                    .find(|(_, b)| !(b.is_ascii_alphanumeric() || **b == b'_' || **b == b'$' || **b >= 0x80))
                    .map_or(bytes.len(), |(n, _)| n);
                let name = sql.get(start..end).unwrap_or_default();
                if name.is_empty() {
                    at = end;
                    continue;
                }
                if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || name.starts_with(|c: char| c.is_ascii_digit()) {
                    return Err(format!("parameter :{name} is not a name of letters, digits, and underscores that starts with a letter or underscore"));
                }
                if !names.iter().any(|known| known == name) {
                    names.push(name.to_string());
                }
                at = end;
            }
            b'?' | b'@' | b'$' => {
                return Err(format!(
                    "{} starts a parameter form this pack does not bind; write named parameters as :name",
                    char::from(byte)
                ));
            }
            _ => at += 1,
        }
    }
    Ok(names)
}

fn skip_quoted(bytes: &[u8], start: usize, close: u8) -> usize {
    bytes
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, b)| **b == close)
        .map_or(bytes.len(), |(n, _)| n + 1)
}

#[cfg(test)]
mod tests {
    use super::parameters;

    #[test]
    fn named_parameters_are_found_outside_quotes_and_comments() {
        let sql = "INSERT INTO t(a, b) VALUES (:a, ':not'); -- :nope\n/* :nor */ SELECT \"x:y\", [z:w], :b, :a, 'it''s :no'";
        assert_eq!(parameters(sql), Ok(vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn other_parameter_forms_and_odd_names_are_refused() {
        for sql in ["SELECT ?", "SELECT ?1", "SELECT @a", "SELECT $a", "SELECT :1a", "SELECT :\u{e9}", "SELECT :a$b"] {
            assert!(parameters(sql).is_err(), "{sql}");
        }
    }
}
