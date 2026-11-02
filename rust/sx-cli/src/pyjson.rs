use sx_kernel::json::{self, Json, MAX_DEPTH};

struct Failure {
    message: String,
    at: usize,
}

type Scanned = Result<usize, Failure>;

fn fail<T>(message: &str, at: usize) -> Result<T, Failure> {
    Err(Failure { message: message.to_string(), at })
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

impl Scanner<'_> {
    fn skip_space(&self, mut at: usize) -> usize {
        while self.bytes.get(at).copied().is_some_and(is_space) {
            at += 1;
        }
        at
    }

    fn at(&self, at: usize) -> Option<u8> {
        self.bytes.get(at).copied()
    }

    fn value(&self, at: usize, depth: usize) -> Scanned {
        if depth > MAX_DEPTH {
            return fail(&format!("JSON nests deeper than {MAX_DEPTH} levels"), at);
        }
        match self.at(at) {
            Some(b'"') => self.string(at + 1),
            Some(b'{') => self.object(at + 1, depth),
            Some(b'[') => self.array(at + 1, depth),
            Some(b'n') if self.bytes[at..].starts_with(b"null") => Ok(at + 4),
            Some(b't') if self.bytes[at..].starts_with(b"true") => Ok(at + 4),
            Some(b'f') if self.bytes[at..].starts_with(b"false") => Ok(at + 5),
            Some(b'N') if self.bytes[at..].starts_with(b"NaN") => Ok(at + 3),
            Some(b'I') if self.bytes[at..].starts_with(b"Infinity") => Ok(at + 8),
            Some(b'-') if self.bytes[at..].starts_with(b"-Infinity") => Ok(at + 9),
            Some(b'-' | b'0'..=b'9') => self.number(at),
            _ => fail("Expecting value", at),
        }
    }

    fn digits(&self, mut at: usize) -> usize {
        while self.at(at).is_some_and(|b| b.is_ascii_digit()) {
            at += 1;
        }
        at
    }

    fn number(&self, start: usize) -> Scanned {
        let mut at = start;
        if self.at(at) == Some(b'-') {
            at += 1;
        }
        match self.at(at) {
            Some(b'0') => at += 1,
            Some(b'1'..=b'9') => at = self.digits(at),
            _ => return fail("Expecting value", start),
        }
        if self.at(at) == Some(b'.') && self.at(at + 1).is_some_and(|b| b.is_ascii_digit()) {
            at = self.digits(at + 1);
        }
        if matches!(self.at(at), Some(b'e' | b'E')) {
            let mut exponent = at + 1;
            if matches!(self.at(exponent), Some(b'+' | b'-')) {
                exponent += 1;
            }
            if self.at(exponent).is_some_and(|b| b.is_ascii_digit()) {
                at = self.digits(exponent);
            }
        }
        Ok(at)
    }

    fn string(&self, mut at: usize) -> Scanned {
        let begin = at - 1;
        loop {
            let Some(byte) = self.at(at) else {
                return fail("Unterminated string starting at", begin);
            };
            match byte {
                b'"' => return Ok(at + 1),
                0..=0x1f => return fail("Invalid control character at", at),
                b'\\' => {
                    let Some(escape) = self.at(at + 1) else {
                        return fail("Unterminated string starting at", begin);
                    };
                    match escape {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => at += 2,
                        b'u' => {
                            let digits = self.bytes.get(at + 2..at + 6);
                            match digits {
                                Some(hex) if hex.iter().all(u8::is_ascii_hexdigit) => at += 6,
                                _ => return fail("Invalid \\uXXXX escape", at + 1),
                            }
                        }
                        _ => return fail("Invalid \\escape", at),
                    }
                }
                _ => at += 1,
            }
        }
    }

    fn object(&self, start: usize, depth: usize) -> Scanned {
        let mut at = self.skip_space(start);
        if self.at(at) == Some(b'}') {
            return Ok(at + 1);
        }
        loop {
            if self.at(at) != Some(b'"') {
                return fail("Expecting property name enclosed in double quotes", at);
            }
            at = self.string(at + 1)?;
            at = self.skip_space(at);
            if self.at(at) != Some(b':') {
                return fail("Expecting ':' delimiter", at);
            }
            at = self.skip_space(at + 1);
            at = self.value(at, depth + 1)?;
            at = self.skip_space(at);
            match self.at(at) {
                Some(b'}') => return Ok(at + 1),
                Some(b',') => {}
                _ => return fail("Expecting ',' delimiter", at),
            }
            let comma = at;
            at = self.skip_space(at + 1);
            if self.at(at) == Some(b'}') {
                return fail("Illegal trailing comma before end of object", comma);
            }
        }
    }

    fn array(&self, start: usize, depth: usize) -> Scanned {
        let mut at = self.skip_space(start);
        if self.at(at) == Some(b']') {
            return Ok(at + 1);
        }
        loop {
            at = self.value(at, depth + 1)?;
            at = self.skip_space(at);
            match self.at(at) {
                Some(b']') => return Ok(at + 1),
                Some(b',') => {}
                _ => return fail("Expecting ',' delimiter", at),
            }
            let comma = at;
            at = self.skip_space(at + 1);
            if self.at(at) == Some(b']') {
                return fail("Illegal trailing comma before end of array", comma);
            }
        }
    }
}

fn located(text: &str, failure: &Failure) -> String {
    if failure.message.starts_with("JSON nests") {
        return failure.message.clone();
    }
    let prefix = text.get(..failure.at).unwrap_or(text);
    let line = prefix.matches('\n').count() + 1;
    let column = match prefix.rfind('\n') {
        Some(newline) => prefix[newline + 1..].chars().count() + 1,
        None => prefix.chars().count() + 1,
    };
    format!("{}: line {line} column {column} (char {})", failure.message, prefix.chars().count())
}

pub fn parse(text: &str) -> Result<Json, String> {
    let scanner = Scanner { text, bytes: text.as_bytes() };
    let checked = if text.starts_with('\u{feff}') {
        Err(Failure { message: "Unexpected UTF-8 BOM (decode using utf-8-sig)".to_string(), at: 0 })
    } else {
        let start = scanner.skip_space(0);
        scanner.value(start, 0).and_then(|end| {
            let end = scanner.skip_space(end);
            if end == scanner.bytes.len() { Ok(end) } else { fail("Extra data", end) }
        })
    };
    if let Err(failure) = checked {
        return Err(located(scanner.text, &failure));
    }
    json::parse(text).map_err(|error| error.0)
}
