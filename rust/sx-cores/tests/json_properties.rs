use sx_cores::json::{Kind, get, keys, kind};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).expect("fits")
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[derive(Clone, Debug)]
enum Built {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    Array(Vec<Built>),
    Object(Vec<(String, Built)>),
}

const NUMBERS: [&str; 12] = ["0", "-0", "7", "-12", "1.0", "1.50", "2e10", "1E-3", "0.5e+2", "123456789012345678901234567890", "1e400", "-9.99"];
const TEXTS: [&str; 12] = ["", "a", "hello world", "caf\u{e9}", "\u{2615}", "\u{1f600}", "q\"uote", "back\\slash", "line\nbreak", "tab\t", "\u{7}bell", "\0nul"];
const KEYS: [&str; 8] = ["a", "b", "name", "x.y", "a b", "\u{e9}", "", "k\"q"];
const BLANKS: [&str; 5] = ["", " ", "\n", "\r\n\t", "  \n  "];

fn build(rng: &mut Rng, depth: usize) -> Built {
    let leaf = depth >= 4 || rng.below(3) == 0;
    if leaf {
        return match rng.below(4) {
            0 => Built::Null,
            1 => Built::Bool(rng.below(2) == 0),
            2 => Built::Number((*rng.pick(&NUMBERS)).to_string()),
            _ => Built::Text((*rng.pick(&TEXTS)).to_string()),
        };
    }
    if rng.below(2) == 0 {
        Built::Array((0..rng.below(4)).map(|_| build(rng, depth + 1)).collect())
    } else {
        let mut used: Vec<&str> = Vec::new();
        let mut entries = Vec::new();
        for _ in 0..rng.below(4) {
            let key = *rng.pick(&KEYS);
            if !used.contains(&key) {
                used.push(key);
                entries.push((key.to_string(), build(rng, depth + 1)));
            }
        }
        Built::Object(entries)
    }
}

fn quote(text: &str, rng: &mut Rng) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c if u32::from(c) > 0xffff => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04X}"));
                }
            }
            c if rng.below(5) == 0 && !c.is_ascii() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write(node: &Built, rng: &mut Rng, out: &mut String) {
    let blanks = |rng: &mut Rng| *rng.pick(&BLANKS);
    match node {
        Built::Null => out.push_str("null"),
        Built::Bool(flag) => out.push_str(&flag.to_string()),
        Built::Number(text) => out.push_str(text),
        Built::Text(text) => out.push_str(&quote(text, rng)),
        Built::Array(items) => {
            out.push('[');
            out.push_str(blanks(rng));
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push_str(blanks(rng));
                    out.push(',');
                    out.push_str(blanks(rng));
                }
                write(item, rng, out);
            }
            out.push_str(blanks(rng));
            out.push(']');
        }
        Built::Object(entries) => {
            out.push('{');
            out.push_str(blanks(rng));
            for (at, (key, item)) in entries.iter().enumerate() {
                if at > 0 {
                    out.push_str(blanks(rng));
                    out.push(',');
                    out.push_str(blanks(rng));
                }
                out.push_str(&quote(key, rng));
                out.push_str(blanks(rng));
                out.push(':');
                out.push_str(blanks(rng));
                write(item, rng, out);
            }
            out.push_str(blanks(rng));
            out.push('}');
        }
    }
}

fn check(node: &Built, path: &mut Vec<String>, text: &str) {
    let shown: Vec<&str> = path.iter().map(String::as_str).collect();
    let context = format!("{path:?} in {text}");
    match node {
        Built::Null => {
            assert_eq!(get(text, &shown).expect(&context), None, "{context}");
            assert_eq!(kind(text, &shown).expect(&context), Kind::Null, "{context}");
            assert_eq!(keys(text, &shown).expect(&context), Vec::<String>::new(), "{context}");
        }
        Built::Bool(flag) => {
            assert_eq!(get(text, &shown).expect(&context), Some(flag.to_string()), "{context}");
            assert_eq!(kind(text, &shown).expect(&context), Kind::Boolean, "{context}");
            assert!(keys(text, &shown).is_err(), "{context}");
        }
        Built::Number(number) => {
            assert_eq!(get(text, &shown).expect(&context).as_deref(), Some(number.as_str()), "{context}");
            assert_eq!(kind(text, &shown).expect(&context), Kind::Number, "{context}");
            assert!(keys(text, &shown).is_err(), "{context}");
        }
        Built::Text(value) => {
            assert_eq!(get(text, &shown).expect(&context).as_deref(), Some(value.as_str()), "{context}");
            assert_eq!(kind(text, &shown).expect(&context), Kind::String, "{context}");
            assert!(keys(text, &shown).is_err(), "{context}");
        }
        Built::Array(items) => {
            assert!(get(text, &shown).is_err(), "{context}");
            assert_eq!(kind(text, &shown).expect(&context), Kind::Array, "{context}");
            let indexes: Vec<String> = (0..items.len()).map(|i| i.to_string()).collect();
            assert_eq!(keys(text, &shown).expect(&context), indexes, "{context}");
            for (at, item) in items.iter().enumerate() {
                path.push(at.to_string());
                check(item, path, text);
                path.pop();
            }
            path.push(items.len().to_string());
            let beyond: Vec<&str> = path.iter().map(String::as_str).collect();
            assert_eq!(kind(text, &beyond).expect(&context), Kind::Absent, "{context}");
            path.pop();
        }
        Built::Object(entries) => {
            assert!(get(text, &shown).is_err(), "{context}");
            assert_eq!(kind(text, &shown).expect(&context), Kind::Object, "{context}");
            let names: Vec<String> = entries.iter().map(|(key, _)| key.clone()).collect();
            assert_eq!(keys(text, &shown).expect(&context), names, "{context}");
            for (key, item) in entries {
                path.push(key.clone());
                check(item, path, text);
                path.pop();
            }
        }
    }
}

#[test]
fn generated_documents_read_back_as_built() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..400 {
        let node = build(&mut rng, 0);
        let mut text = String::new();
        text.push_str(rng.pick(&BLANKS));
        write(&node, &mut rng, &mut text);
        text.push_str(rng.pick(&BLANKS));
        check(&node, &mut Vec::new(), &text);
    }
}

#[test]
fn a_truncated_container_document_is_always_refused() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut refused = 0;
    for _ in 0..200 {
        let node = Built::Array(vec![build(&mut rng, 1), build(&mut rng, 1), build(&mut rng, 1)]);
        let mut text = String::new();
        write(&node, &mut rng, &mut text);
        let cuts: Vec<usize> = text.char_indices().map(|(at, _)| at).filter(|at| *at > 0).collect();
        for at in [cuts[cuts.len() / 3], cuts[cuts.len() / 2], cuts[cuts.len() - 1]] {
            assert!(kind(&text[..at], &[]).is_err(), "{:?} parsed", &text[..at]);
            refused += 1;
        }
    }
    assert!(refused >= 600);
}
