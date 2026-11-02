use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use deser_value::{Kind, Value};
use sx_cores::git::{self, Checkout, Head};
use sx_cores::github::{self, Check, CiState, UpstreamState};
use sx_cores::semver::{self, Version};
use sx_cores::text::{self, TextError};

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    fn from_value(value: &Value) -> Json {
        match value.kind() {
            Kind::Null => Json::Null,
            Kind::Bool(b) => Json::Bool(*b),
            Kind::Str(_) => Json::Str(value.as_str().expect("string").to_owned()),
            Kind::Seq(seq) => Json::List(seq.iter().map(Json::from_value).collect()),
            Kind::Map(map) => Json::Object(
                map.iter()
                    .map(|(key, value)| (key.as_str().expect("string key").to_owned(), Json::from_value(value)))
                    .collect(),
            ),
            _ => Json::Int(value.as_i64().expect("an integer that fits in i64")),
        }
    }

    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("expected a string, found {other:?}"),
        }
    }

    fn int(&self) -> i64 {
        match self {
            Json::Int(n) => *n,
            other => panic!("expected an integer, found {other:?}"),
        }
    }

    fn bool(&self) -> bool {
        match self {
            Json::Bool(b) => *b,
            other => panic!("expected a bool, found {other:?}"),
        }
    }

    fn list(&self) -> &[Json] {
        match self {
            Json::List(items) => items,
            other => panic!("expected a list, found {other:?}"),
        }
    }

    fn field(&self, name: &str) -> &Json {
        match self {
            Json::Object(fields) => fields.get(name).unwrap_or_else(|| panic!("no field {name} in {self:?}")),
            other => panic!("expected a record, found {other:?}"),
        }
    }

    fn option<T>(&self, decode: impl FnOnce(&Json) -> T) -> Option<T> {
        match self {
            Json::Null => None,
            other => Some(decode(other)),
        }
    }

    fn string(s: &str) -> Json {
        Json::Str(s.to_owned())
    }

    fn strings<S: AsRef<str>>(items: &[S]) -> Json {
        Json::List(items.iter().map(|s| Json::string(s.as_ref())).collect())
    }

    fn maybe<T>(option: Option<T>, encode: impl FnOnce(T) -> Json) -> Json {
        option.map_or(Json::Null, encode)
    }
}

enum Expected {
    Result(Json),
    Fails(String),
}

struct Vector {
    function: String,
    args: Vec<Json>,
    expected: Expected,
}

fn load(core: &str) -> Vec<Vector> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cores").join(format!("{core}.json"));
    let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let document: Value = deser_json::from_str(&source).expect("vector file is JSON");
    let document = Json::from_value(&document);
    assert_eq!(document.field("core").str(), core);
    document
        .field("vectors")
        .list()
        .iter()
        .map(|vector| {
            let Json::Object(fields) = vector else { panic!("vector is not an object") };
            let expected = match (fields.get("result"), fields.get("fails")) {
                (Some(result), None) => Expected::Result(result.clone()),
                (None, Some(kind)) => Expected::Fails(kind.str().to_owned()),
                _ => panic!("a vector has exactly one of result and fails: {vector:?}"),
            };
            Vector { function: vector.field("function").str().to_owned(), args: vector.field("args").list().to_vec(), expected }
        })
        .collect()
}

fn check_core(core: &str, functions: &[&str], run: impl Fn(&str, &[Json]) -> Result<Json, String>) {
    let vectors = load(core);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for vector in &vectors {
        let outcome = run(&vector.function, &vector.args);
        let matches = match (&vector.expected, &outcome) {
            (Expected::Result(want), Ok(got)) => want == got,
            (Expected::Fails(want), Err(got)) => want == got,
            _ => false,
        };
        let expected = match &vector.expected {
            Expected::Result(json) => format!("{json:?}"),
            Expected::Fails(kind) => format!("fails with {kind}"),
        };
        assert!(matches, "{} {:?}\n  want {expected}\n  got  {outcome:?}", vector.function, vector.args);
        *counts.entry(vector.function.as_str()).or_default() += 1;
    }
    for function in functions {
        assert!(counts.get(function).is_some_and(|&n| n > 0), "no vectors for {function}");
    }
    assert_eq!(counts.len(), functions.len(), "vectors name a function this test does not know: {counts:?}");
}

fn text_run(function: &str, args: &[Json]) -> Result<Json, String> {
    let s = |index: usize| args[index].str();
    Ok(match function {
        "text.casefold" => Json::string(&text::casefold(s(0))),
        "text.split" => match text::split(s(0), s(1)) {
            Ok(parts) => Json::strings(&parts),
            Err(TextError::EmptySeparator) => return Err("ValueError".to_owned()),
        },
        "text.join" => {
            let parts: Vec<&str> = args[1].list().iter().map(Json::str).collect();
            Json::string(&text::join(s(0), &parts))
        }
        "text.chars" => Json::strings(&text::chars(s(0))),
        "text.trim" => Json::string(&text::trim(s(0))),
        "text.length" => Json::Int(text::length(s(0))),
        "text.starts-with?" => Json::Bool(text::starts_with(s(0), s(1))),
        "text.ends-with?" => Json::Bool(text::ends_with(s(0), s(1))),
        "text.contains?" => Json::Bool(text::contains(s(0), s(1))),
        "text.replace" => Json::string(&text::replace(s(0), s(1), s(2))),
        "text.from-int" => Json::string(&text::from_int(args[0].int())),
        "text.to-int" => Json::maybe(text::to_int(s(0)), Json::Int),
        other => panic!("unknown function {other}"),
    })
}

#[test]
fn text_matches_the_python_vectors() {
    check_core(
        "text",
        &[
            "text.casefold", "text.split", "text.join", "text.chars", "text.trim", "text.length",
            "text.starts-with?", "text.ends-with?", "text.contains?", "text.replace", "text.from-int", "text.to-int",
        ],
        text_run,
    );
}

fn version(json: &Json) -> Version {
    Version { major: json.field("major").int(), minor: json.field("minor").int(), patch: json.field("patch").int() }
}

fn version_json(v: Version) -> Json {
    Json::Object(BTreeMap::from([
        ("major".to_owned(), Json::Int(v.major)),
        ("minor".to_owned(), Json::Int(v.minor)),
        ("patch".to_owned(), Json::Int(v.patch)),
    ]))
}

#[test]
fn semver_matches_the_python_vectors() {
    check_core("semver", &["semver.parse", "semver.compare", "semver.show"], |function, args| {
        Ok(match function {
            "semver.parse" => Json::maybe(semver::parse(args[0].str()), version_json),
            "semver.compare" => Json::Int(semver::compare(version(&args[0]), version(&args[1]))),
            "semver.show" => Json::string(&semver::show(version(&args[0]))),
            other => panic!("unknown function {other}"),
        })
    });
}

fn head(json: &Json) -> Head {
    Head {
        staged: json.field("staged").int(),
        unstaged: json.field("unstaged").int(),
        untracked: json.field("untracked").int(),
        ahead: json.field("ahead").option(Json::int),
        behind: json.field("behind").option(Json::int),
        upstream: json.field("upstream").option(|j| j.str().to_owned()),
    }
}

fn head_json(h: &Head) -> Json {
    Json::Object(BTreeMap::from([
        ("staged".to_owned(), Json::Int(h.staged)),
        ("unstaged".to_owned(), Json::Int(h.unstaged)),
        ("untracked".to_owned(), Json::Int(h.untracked)),
        ("ahead".to_owned(), Json::maybe(h.ahead, Json::Int)),
        ("behind".to_owned(), Json::maybe(h.behind, Json::Int)),
        ("upstream".to_owned(), Json::maybe(h.upstream.as_deref(), Json::string)),
    ]))
}

fn checkout(json: &Json) -> Checkout {
    if json.str_or_none() == Some("Missing") {
        return Checkout::Missing;
    }
    let payload = json.field("value");
    match json.field("tag").str() {
        "Invalid" => Checkout::Invalid(payload.str().to_owned()),
        "OriginMismatch" => Checkout::OriginMismatch {
            branch: payload.field("branch").option(|j| j.str().to_owned()),
            head: head(payload.field("head")),
            error: payload.field("error").str().to_owned(),
        },
        "Detached" => Checkout::Detached(head(payload)),
        "OnBranch" => Checkout::OnBranch {
            branch: payload.field("branch").str().to_owned(),
            head: head(payload.field("head")),
        },
        other => panic!("unknown checkout variant {other}"),
    }
}

impl Json {
    fn str_or_none(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[test]
fn git_matches_the_python_vectors() {
    check_core(
        "git",
        &["git.head", "git.branch", "git.error", "git.local-label", "git.age-days", "git.stale-work?"],
        |function, args| {
            Ok(match function {
                "git.head" => Json::maybe(git::head(&checkout(&args[0])), head_json),
                "git.branch" => Json::maybe(git::branch(&checkout(&args[0])), Json::string),
                "git.error" => Json::maybe(git::error(&checkout(&args[0])), Json::string),
                "git.local-label" => {
                    let default = args[1].option(|j| j.str().to_owned());
                    Json::string(&git::local_label(
                        &checkout(&args[0]),
                        default.as_deref(),
                        args[2].option(Json::int),
                        args[3].option(Json::int),
                    ))
                }
                "git.age-days" => Json::maybe(git::age_days(args[0].int(), args[1].option(Json::int)), Json::Int),
                "git.stale-work?" => {
                    Json::Bool(git::stale_work(args[0].option(Json::int), args[1].option(Json::int)))
                }
                other => panic!("unknown function {other}"),
            })
        },
    );
}

fn check(json: &Json) -> Check {
    let text = |name: &str| json.field(name).option(|j| j.str().to_owned());
    Check {
        name: json.field("name").str().to_owned(),
        state: json.field("state").str().to_owned(),
        source: text("source"),
        updated_at: text("updated-at"),
        suite_id: text("suite-id"),
        run_id: text("run-id"),
        attempt: json.field("attempt").option(Json::int),
    }
}

fn ci_state(variant: &str) -> CiState {
    CiState::ALL.into_iter().find(|state| state.variant() == variant).expect("a CiState variant")
}

fn upstream_state(variant: &str) -> UpstreamState {
    UpstreamState::ALL.into_iter().find(|state| state.variant() == variant).expect("an UpstreamState variant")
}

#[test]
fn github_matches_the_python_vectors() {
    check_core(
        "github",
        &["github.ci-state", "github.ci-state-name", "github.parse-ci-state", "github.upstream-state-name"],
        |function, args| {
            Ok(match function {
                "github.ci-state" => {
                    let checks: Vec<Check> = args[0].list().iter().map(check).collect();
                    Json::string(github::ci_state(&checks, args[1].bool()).variant())
                }
                "github.ci-state-name" => Json::string(github::ci_state_name(ci_state(args[0].str()))),
                "github.parse-ci-state" => Json::string(github::parse_ci_state(args[0].str()).variant()),
                "github.upstream-state-name" => {
                    Json::string(github::upstream_state_name(upstream_state(args[0].str())))
                }
                other => panic!("unknown function {other}"),
            })
        },
    );
}
