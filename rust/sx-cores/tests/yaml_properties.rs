use sx_cores::yaml::{get, keys, set};

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

const PLAIN: [&str; 14] = [
    "demo", "1.2.3", "3", "true", "registry.example.com:5000/app:1.2", "a#b", "-1.5", "~", "http://x.y/z?q=1#f",
    "caf\u{e9}", "x y z", "0x1F", "2024-01-02", "v1",
];
const KEYS: [&str; 6] = ["a", "b2", "key-x", "x.y", "name", "Z"];
const QUOTED_KEYS: [&str; 4] = ["a b", "k: v", "it's", "#hash"];
const NASTY: [&str; 40] = [
    " ", ":", ": ", " #", "#", "'", "\"", "\\", "\n", "\t", "-", "- ", "?", "[", "]", "{", "}", ",", "&", "*", "!", "|",
    ">", "%", "@", "`", "a", "Z", "0", "9", ".", "\u{e9}", "\u{2615}", "\u{85}", "\u{2028}", "\u{feff}", "\0", "\r",
    "---", "true",
];

fn nasty_value(rng: &mut Rng) -> String {
    (0..rng.below(8)).map(|_| *rng.pick(&NASTY)).collect()
}

struct Edit {
    path: Vec<String>,
    start: usize,
    end: usize,
    value: String,
}

#[derive(Default)]
struct Built {
    text: String,
    scalars: Vec<Edit>,
    collections: Vec<(Vec<String>, Vec<String>)>,
}

fn double(value: &str) -> String {
    let escaped: String = value
        .chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            other => other.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

fn scalar_text(rng: &mut Rng) -> (String, String) {
    match rng.below(4) {
        0 | 1 => {
            let value = (*rng.pick(&PLAIN)).to_string();
            (value.clone(), value)
        }
        2 => {
            let value = format!("{}{}", rng.pick(&PLAIN), rng.pick(&[" #x", ": y", "'s", " "]));
            (value.clone(), format!("'{}'", value.replace('\'', "''")))
        }
        _ => {
            let value = format!("{}{}", rng.pick(&["tab\there", "line\nbreak", "q\"uote", "back\\slash", "plain"]), rng.pick(&["", " #", ": "]));
            (value.clone(), double(&value))
        }
    }
}

fn key_text(rng: &mut Rng, taken: &[String]) -> (String, String) {
    loop {
        let (key, text) = match rng.below(6) {
            0 => {
                let key = (*rng.pick(&QUOTED_KEYS)).to_string();
                let text = if key.contains('\'') { double(&key) } else { format!("'{key}'") };
                (key, text)
            }
            _ => {
                let key = (*rng.pick(&KEYS)).to_string();
                (key.clone(), key)
            }
        };
        if !taken.contains(&key) {
            return (key, text);
        }
        if taken.len() > 8 {
            let key = format!("k{}", taken.len());
            return (key.clone(), key);
        }
    }
}

fn comment(rng: &mut Rng, out: &mut String, indent: usize) {
    match rng.below(6) {
        0 => out.push_str(&format!("{}# note\n", " ".repeat(indent))),
        1 => out.push('\n'),
        _ => {}
    }
}

fn trailing(rng: &mut Rng, out: &mut String) {
    if rng.below(4) == 0 {
        out.push_str("  # trailing");
    }
    out.push('\n');
}

fn emit_value(rng: &mut Rng, built: &mut Built, path: Vec<String>, indent: usize, depth: usize, in_seq: bool) {
    let choice = if depth >= 3 { rng.below(3) } else { rng.below(6) };
    match choice {
        0 | 1 => {
            let (value, text) = scalar_text(rng);
            built.text.push(' ');
            let start = built.text.len();
            built.text.push_str(&text);
            built.scalars.push(Edit { path, start, end: start + text.len(), value });
            trailing(rng, &mut built.text);
        }
        2 => {
            let at = built.text.len();
            built.scalars.push(Edit { path, start: at, end: at, value: String::new() });
            trailing(rng, &mut built.text);
        }
        3 | 4 => {
            built.text.push('\n');
            let deeper = indent + 1 + rng.below(4);
            emit_map(rng, built, path, deeper, depth + 1);
        }
        _ => {
            built.text.push('\n');
            let seq_indent = if in_seq { indent + 2 } else { indent + rng.below(3) };
            emit_seq(rng, built, path, seq_indent, depth + 1);
        }
    }
}

fn emit_map(rng: &mut Rng, built: &mut Built, path: Vec<String>, indent: usize, depth: usize) {
    let mut taken: Vec<String> = Vec::new();
    for _ in 0..1 + rng.below(3) {
        comment(rng, &mut built.text, indent);
        let (key, text) = key_text(rng, &taken);
        built.text.push_str(&" ".repeat(indent));
        built.text.push_str(&text);
        built.text.push(':');
        let mut child = path.clone();
        child.push(key.clone());
        taken.push(key);
        emit_value(rng, built, child, indent, depth, false);
    }
    built.collections.push((path, taken));
}

fn emit_seq(rng: &mut Rng, built: &mut Built, path: Vec<String>, indent: usize, depth: usize) {
    let count = 1 + rng.below(3);
    for index in 0..count {
        comment(rng, &mut built.text, indent);
        built.text.push_str(&" ".repeat(indent));
        built.text.push('-');
        let mut child = path.clone();
        child.push(index.to_string());
        if rng.below(4) == 0 && depth < 3 {
            built.text.push(' ');
            let column = indent + 2;
            let mut taken: Vec<String> = Vec::new();
            for entry in 0..1 + rng.below(2) {
                if entry > 0 {
                    built.text.push_str(&" ".repeat(column));
                }
                let (key, text) = key_text(rng, &taken);
                built.text.push_str(&text);
                built.text.push(':');
                let mut grandchild = child.clone();
                grandchild.push(key.clone());
                taken.push(key);
                emit_value(rng, built, grandchild, column, depth + 1, false);
            }
            built.collections.push((child, taken));
        } else {
            emit_value(rng, built, child, indent, depth + 1, true);
        }
    }
    built.collections.push((path, (0..count).map(|index| index.to_string()).collect()));
}

fn document(rng: &mut Rng) -> Built {
    let mut built = Built::default();
    if rng.below(5) == 0 {
        built.text.push_str("# header\n---\n");
    }
    let indent = rng.below(2);
    if rng.below(5) == 0 {
        emit_seq(rng, &mut built, Vec::new(), indent, 0);
    } else {
        emit_map(rng, &mut built, Vec::new(), indent, 0);
    }
    built
}

fn view(path: &[String]) -> Vec<&str> {
    path.iter().map(String::as_str).collect()
}

#[test]
fn the_generated_documents_read_back_as_built() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..400 {
        let built = document(&mut rng);
        for edit in &built.scalars {
            assert_eq!(
                get(&built.text, &view(&edit.path)).unwrap_or_else(|e| panic!("case {case}: {e}\n{}", built.text)).as_deref(),
                Some(edit.value.as_str()),
                "case {case} {:?}\n{}",
                edit.path,
                built.text
            );
        }
        for (path, expected) in &built.collections {
            assert_eq!(&keys(&built.text, &view(path)).expect("keys"), expected, "case {case} {path:?}\n{}", built.text);
        }
    }
}

#[test]
fn editing_a_scalar_changes_only_that_scalar() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut edits = 0;
    for case in 0..400 {
        let built = document(&mut rng);
        for edit in built.scalars.iter().take(6) {
            let value = if rng.below(3) == 0 { (*rng.pick(&PLAIN)).to_string() } else { nasty_value(&mut rng) };
            let path = view(&edit.path);
            let edited = set(&built.text, &path, &value).unwrap_or_else(|e| panic!("case {case}: {e}\n{}", built.text));
            let head = built.text.get(..edit.start).expect("boundary");
            let tail = built.text.get(edit.end..).expect("boundary");
            assert!(edited.starts_with(head), "case {case}: the bytes before the scalar changed\n{}", built.text);
            assert!(edited.ends_with(tail), "case {case}: the bytes after the scalar changed\n{}", built.text);
            assert_eq!(get(&edited, &path).expect("reads back").as_deref(), Some(value.as_str()), "case {case}: {value:?}\n{edited}");
            for other in &built.scalars {
                if other.path != edit.path {
                    assert_eq!(
                        get(&edited, &view(&other.path)).expect("reads").as_deref(),
                        Some(other.value.as_str()),
                        "case {case}: {:?} moved when {:?} was edited\n{edited}",
                        other.path,
                        edit.path
                    );
                }
            }
            for (collection, expected) in &built.collections {
                assert_eq!(&keys(&edited, &view(collection)).expect("keys"), expected, "case {case}");
            }
            edits += 1;
        }
    }
    assert!(edits > 1500, "only {edits} edits ran");
}

#[test]
fn setting_the_current_text_of_a_plain_scalar_is_the_identity() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for _ in 0..300 {
        let built = document(&mut rng);
        for edit in &built.scalars {
            let literal = built.text.get(edit.start..edit.end).expect("boundary");
            if literal == edit.value && !edit.value.is_empty() {
                let again = set(&built.text, &view(&edit.path), &edit.value).expect("set");
                assert_eq!(again, built.text);
            }
        }
    }
}

#[test]
fn damaged_documents_never_panic_and_never_edit_the_wrong_thing() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let paths: [&[&str]; 4] = [&[], &["a"], &["a", "0"], &["b2", "key-x"]];
    let mut succeeded = 0;
    for _ in 0..3000 {
        let mut text = document(&mut rng).text;
        for _ in 0..1 + rng.below(3) {
            let boundaries: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
            if boundaries.is_empty() {
                break;
            }
            let at = *rng.pick(&boundaries);
            match rng.below(4) {
                0 => {
                    text.remove(at);
                }
                1 => text.insert_str(at, rng.pick(&NASTY)),
                2 => text.insert_str(at, "\n  "),
                _ => text.insert(at, '\t'),
            }
        }
        for path in paths {
            let _ = get(&text, path);
            let _ = keys(&text, path);
            if let Ok(edited) = set(&text, path, "new value") {
                succeeded += 1;
                assert_eq!(get(&edited, path).expect("reads back").as_deref(), Some("new value"), "{text:?}");
            }
        }
    }
    assert!(succeeded > 50, "only {succeeded} damaged edits succeeded");
}
