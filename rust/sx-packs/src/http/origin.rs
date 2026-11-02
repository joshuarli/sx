use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }

    fn of(text: &str) -> Result<Scheme, String> {
        if text.eq_ignore_ascii_case("http") {
            Ok(Scheme::Http)
        } else if text.eq_ignore_ascii_case("https") {
            Ok(Scheme::Https)
        } else {
            Err(format!("the scheme must be http or https, not {text:?}"))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}:{}", self.scheme.name(), self.host, self.port)
    }
}

impl Origin {
    pub fn parse(text: &str) -> Result<Origin, String> {
        let (scheme, rest) = split_scheme(text)?;
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, remainder) = rest.split_at(end);
        if !remainder.is_empty() {
            return Err(format!("{text:?} has a path, query, or fragment, and an origin has none"));
        }
        parse_authority(scheme, authority)
    }

    pub fn authority(&self) -> String {
        if self.port == self.scheme.default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub origin: Origin,
    pub target: String,
}

impl Url {
    pub fn parse(text: &str) -> Result<Url, String> {
        if let Some(bad) = text.chars().find(|c| c.is_control() || c.is_whitespace() || *c == '\\') {
            return Err(format!("the URL contains {bad:?}, which a URL never holds unescaped"));
        }
        let without_fragment = text.split_once('#').map_or(text, |(before, _)| before);
        let (scheme, rest) = split_scheme(without_fragment)?;
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, remainder) = rest.split_at(end);
        let origin = parse_authority(scheme, authority)?;
        let target = if remainder.is_empty() {
            "/".to_string()
        } else if remainder.starts_with('?') {
            format!("/{remainder}")
        } else {
            remainder.to_string()
        };
        check_target(&target)?;
        Ok(Url { origin, target })
    }

    pub fn path(&self) -> &str {
        self.target.split_once('?').map_or(&self.target, |(path, _)| path)
    }

    pub fn uri(&self) -> String {
        format!("{}://{}{}", self.origin.scheme.name(), self.origin.authority(), self.target)
    }

    pub fn shown(&self) -> String {
        format!("{}://{}{}", self.origin.scheme.name(), self.origin.authority(), self.path())
    }

    pub fn resolve(&self, location: &str) -> Result<Url, String> {
        if location.contains("://") {
            return Url::parse(location);
        }
        let origin = format!("{}://{}", self.origin.scheme.name(), self.origin.authority());
        let joined = if let Some(authority) = location.strip_prefix("//") {
            format!("{}://{authority}", self.origin.scheme.name())
        } else if location.starts_with('/') {
            format!("{origin}{}", dot_free(location))
        } else if location.starts_with('?') {
            format!("{origin}{}{location}", self.path())
        } else if location.starts_with('#') || location.is_empty() {
            self.uri()
        } else {
            let directory = self.path().rsplit_once('/').map_or("", |(directory, _)| directory);
            format!("{origin}{}", dot_free(&format!("{directory}/{location}")))
        };
        Url::parse(&joined)
    }
}

fn dot_free(reference: &str) -> String {
    let (path, query) = match reference.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (reference, None),
    };
    let mut kept: Vec<&str> = Vec::new();
    let mut segments = path.split('/').peekable();
    while let Some(segment) = segments.next() {
        let last = segments.peek().is_none();
        match segment {
            "." => {
                if last {
                    kept.push("");
                }
            }
            ".." => {
                if kept.len() > 1 {
                    kept.pop();
                }
                if last {
                    kept.push("");
                }
            }
            other => kept.push(other),
        }
    }
    let mut out = kept.join("/");
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    match query {
        Some(query) => format!("{out}?{query}"),
        None => out,
    }
}

fn split_scheme(text: &str) -> Result<(Scheme, &str), String> {
    let (scheme, rest) = text.split_once("://").ok_or_else(|| format!("{text:?} is not an absolute http or https URL"))?;
    Ok((Scheme::of(scheme)?, rest))
}

fn check_target(target: &str) -> Result<(), String> {
    let bytes = target.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if !(0x21..=0x7e).contains(byte) {
            return Err("the path and query must be ASCII, with other characters percent-encoded".to_string());
        }
        if *byte == b'%' {
            let escape = bytes.get(at + 1..at + 3);
            if !escape.is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit)) {
                return Err("the path or query holds a % that is not followed by two hexadecimal digits".to_string());
            }
        }
    }
    Ok(())
}

fn parse_authority(scheme: Scheme, authority: &str) -> Result<Origin, String> {
    if authority.contains('@') {
        return Err("the URL carries userinfo, which is never allowed".to_string());
    }
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (inside, after) = bracketed.split_once(']').ok_or("the IPv6 address has no closing bracket")?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or("the IPv6 address is followed by something other than a port")?),
        };
        (ipv6_host(inside)?, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (domain_host(host)?, Some(port)),
            None => (domain_host(authority)?, None),
        }
    };
    let port = match port {
        None => scheme.default_port(),
        Some(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => match text.parse::<u16>() {
            Ok(0) | Err(_) => return Err(format!("port {text} is not between 1 and 65535")),
            Ok(port) => port,
        },
        Some(text) => return Err(format!("port {text:?} is not a number")),
    };
    Ok(Origin { scheme, host, port })
}

fn ipv6_host(inside: &str) -> Result<String, String> {
    let address: Ipv6Addr = inside.parse().map_err(|_| format!("[{inside}] is not an IPv6 address"))?;
    Ok(format!("[{address}]"))
}

fn domain_host(host: &str) -> Result<String, String> {
    if host.is_empty() {
        return Err("the URL has no host".to_string());
    }
    if host.ends_with('.') {
        return Err(format!("the host {host:?} ends in a dot"));
    }
    if host.contains('%') {
        return Err(format!("the host {host:?} contains a percent escape"));
    }
    let mut labels = Vec::new();
    for label in host.split('.') {
        labels.push(ascii_label(label)?);
    }
    let joined = labels.join(".");
    if joined.len() > 253 {
        return Err("the host is longer than 253 characters".to_string());
    }
    let numeric_tail = labels.last().is_some_and(|last| {
        last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x")
    });
    if numeric_tail {
        let address: Ipv4Addr = joined.parse().map_err(|_| format!("{joined:?} looks like an IPv4 address but is not a dotted quad"))?;
        return Ok(address.to_string());
    }
    Ok(joined)
}

fn ascii_label(label: &str) -> Result<String, String> {
    if label.is_empty() {
        return Err("the host has an empty label".to_string());
    }
    let lowered = label.to_lowercase();
    let ascii = if lowered.is_ascii() {
        lowered
    } else {
        let encoded = punycode(&lowered).ok_or_else(|| format!("the label {label:?} is too large to encode"))?;
        format!("xn--{encoded}")
    };
    if ascii.len() > 63 {
        return Err(format!("the label {label:?} is longer than 63 characters"));
    }
    if !ascii.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') {
        return Err(format!("the label {label:?} holds a character a host name never has"));
    }
    Ok(ascii)
}

fn adapt(delta: u32, points: u32, first: bool) -> u32 {
    let mut delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > (35 * 26) / 2 {
        delta /= 35;
        k += 36;
    }
    k + (36 * delta) / (delta + 38)
}

fn digit(value: u32) -> char {
    char::from_u32(if value < 26 { u32::from(b'a') + value } else { u32::from(b'0') + value - 26 }).unwrap_or('?')
}

fn punycode(input: &str) -> Option<String> {
    let points: Vec<u32> = input.chars().map(u32::from).collect();
    let mut out: String = input.chars().filter(char::is_ascii).collect();
    let basic = u32::try_from(out.len()).ok()?;
    let total = u32::try_from(points.len()).ok()?;
    if basic > 0 {
        out.push('-');
    }
    let (mut n, mut delta, mut bias, mut handled) = (128u32, 0u32, 72u32, basic);
    while handled < total {
        let next = points.iter().copied().filter(|point| *point >= n).min()?;
        delta = delta.checked_add((next - n).checked_mul(handled + 1)?)?;
        n = next;
        for point in &points {
            if *point < n {
                delta = delta.checked_add(1)?;
            }
            if *point == n {
                let mut q = delta;
                let mut k = 36;
                loop {
                    let threshold = if k <= bias {
                        1
                    } else if k >= bias + 26 {
                        26
                    } else {
                        k - bias
                    };
                    if q < threshold {
                        break;
                    }
                    out.push(digit(threshold + (q - threshold) % (36 - threshold)));
                    q = (q - threshold) / (36 - threshold);
                    k += 36;
                }
                out.push(digit(q));
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n += 1;
    }
    Some(out)
}

pub fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punycode_matches_the_standard_vectors() {
        assert_eq!(punycode("bücher").as_deref(), Some("bcher-kva"));
        assert_eq!(punycode("münchen").as_deref(), Some("mnchen-3ya"));
        assert_eq!(punycode("日本語").as_deref(), Some("wgv71a119e"));
        assert_eq!(punycode("ドメイン名例").as_deref(), Some("eckwd4c7cu47r2wf"));
    }

    #[test]
    fn dot_segments_are_removed() {
        assert_eq!(dot_free("/a/b/../c/./d"), "/a/c/d");
        assert_eq!(dot_free("/a/b/.."), "/a/");
        assert_eq!(dot_free("/../../x?q=1"), "/x?q=1");
    }
}
