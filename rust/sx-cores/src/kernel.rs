use std::fmt;
use std::sync::Arc;

use sx_kernel::json::Json;
use sx_kernel::machine::{CoreError, CoreImplementations};
use sx_kernel::registry::{
    Assurance, Core, Evidence, Example, Function, LiteralCheck, LoadError, Registry, RegistryError, load_declarations,
};
use sx_kernel::types::{BOOL, EnumType, FLOAT, INT, NominalType, RecordType, STRING, Type};
use sx_kernel::values::{RecordValue, Value};

use crate::decimal::{Decimal, DecimalError, Rounding};
use crate::git::{self, Checkout, Head};
use crate::github::{self, Check, CiState, UpstreamState};
use crate::json;
use crate::re;
use crate::semver::{self, Version};
use crate::text::{self, TextError};
use crate::yaml;

const VECTORS_TEST: &str = "tests/test_core_vectors.py::test_committed_vectors_match_the_python_core";
const STATUS_PARITY: &str =
    "hosts/repomod/tests/test_status_parity.py::test_status_offline_json_matches_python_on_every_fixture";

fn option(t: Type) -> Type {
    Type::Option(Box::new(t))
}

fn list(t: Type) -> Type {
    Type::List(Box::new(t))
}

fn record(name: Option<&str>, fields: Vec<(&str, Type)>) -> Type {
    Type::Record(RecordType {
        fields: fields.into_iter().map(|(n, t)| (n.to_string(), t)).collect(),
        name: name.map(str::to_string),
        open: false,
    })
}

fn enumeration(name: &str, variants: Vec<(&str, Option<Type>)>) -> Type {
    Type::Enum(Arc::new(EnumType {
        name: name.to_string(),
        variants: variants.into_iter().map(|(v, p)| (v.to_string(), p)).collect(),
    }))
}

fn function(name: &str, params: Vec<Type>, output: Type, description: &str, example: (&str, &str)) -> Function {
    Function {
        name: name.to_string(),
        params,
        output,
        description: description.to_string(),
        example: Some(Example { program: example.0.to_string(), result: example.1.to_string() }),
        literal_checks: Vec::new(),
    }
}

fn conformance(references: &[&str]) -> Vec<Evidence> {
    vec![Evidence {
        level: Assurance::ConformanceTested,
        references: references.iter().map(|r| (*r).to_string()).collect(),
    }]
}

pub fn text_core() -> Core {
    let functions = vec![
        function(
            "text.casefold",
            vec![STRING],
            STRING,
            "Lowercases for case-insensitive comparison, as Python str.casefold does.",
            ("(text.casefold \"LaPuta\")", "\"laputa\""),
        ),
        function(
            "text.split",
            vec![STRING, STRING],
            list(STRING),
            "Splits the first string at every occurrence of the second.",
            ("(text.split \"a,b,,c\" \",\")", "[\"a\" \"b\" \"\" \"c\"]"),
        ),
        function(
            "text.join",
            vec![STRING, list(STRING)],
            STRING,
            "Joins the strings with the separator between each pair.",
            ("(text.join \", \" [\"a\" \"b\"])", "\"a, b\""),
        ),
        function(
            "text.chars",
            vec![STRING],
            list(STRING),
            "Splits a string into one-character strings.",
            ("(text.chars \"ab\")", "[\"a\" \"b\"]"),
        ),
        function(
            "text.trim",
            vec![STRING],
            STRING,
            "Removes leading and trailing whitespace.",
            ("(text.trim \"  a \")", "\"a\""),
        ),
        function("text.length", vec![STRING], INT, "Counts characters.", ("(text.length \"abc\")", "3")),
        function(
            "text.starts-with?",
            vec![STRING, STRING],
            BOOL,
            "True when the first string starts with the second.",
            ("(text.starts-with? \"v1.2\" \"v\")", "true"),
        ),
        function(
            "text.ends-with?",
            vec![STRING, STRING],
            BOOL,
            "True when the first string ends with the second.",
            ("(text.ends-with? \"a.md\" \".md\")", "true"),
        ),
        function(
            "text.contains?",
            vec![STRING, STRING],
            BOOL,
            "True when the second string occurs in the first.",
            ("(text.contains? \"hello\" \"ell\")", "true"),
        ),
        function(
            "text.replace",
            vec![STRING, STRING, STRING],
            STRING,
            "Replaces every occurrence of the second string with the third.",
            ("(text.replace \"a-b-c\" \"-\" \"+\")", "\"a+b+c\""),
        ),
        function("text.from-int", vec![INT], STRING, "Writes an Int in decimal.", ("(text.from-int -42)", "\"-42\"")),
        function(
            "text.to-int",
            vec![STRING],
            option(INT),
            "Reads a decimal Int, or none if the string is not one.",
            ("(text.to-int \"17\")", "(some 17)"),
        ),
    ];
    Core {
        id: "text".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: Vec::new(),
        evidence: conformance(&[
            "cores/text/tests/test_text_core.py::test_casefold_matches_python_for_every_code_point",
            "cores/text/tests/test_text_core.py::test_every_example_produces_its_documented_result",
            VECTORS_TEST,
        ]),
    }
}

fn version_type() -> Type {
    record(Some("semver.Version"), vec![("major", INT), ("minor", INT), ("patch", INT)])
}

pub fn semver_core() -> Core {
    let version = version_type();
    let functions = vec![
        function(
            "semver.parse",
            vec![STRING],
            option(version.clone()),
            "Reads MAJOR.MINOR.PATCH, or none if the string is not exactly that.",
            ("(semver.parse \"1.4.2\")", "(some {:major 1 :minor 4 :patch 2})"),
        ),
        function(
            "semver.compare",
            vec![version.clone(), version.clone()],
            INT,
            "-1, 0, or 1 as the first version is lower, equal, or higher.",
            ("(semver.compare {:major 1 :minor 0 :patch 0} {:major 0 :minor 9 :patch 9})", "1"),
        ),
        function(
            "semver.show",
            vec![version.clone()],
            STRING,
            "Writes a version as MAJOR.MINOR.PATCH.",
            ("(semver.show {:major 2 :minor 0 :patch 1})", "\"2.0.1\""),
        ),
    ];
    Core {
        id: "semver".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: vec![("semver.Version".to_string(), version)],
        evidence: conformance(&[
            "cores/semver/tests/test_semver_core.py::test_compare_orders_versions_by_number_not_by_text",
            "cores/semver/tests/test_semver_core.py::test_every_example_produces_its_documented_result",
            VECTORS_TEST,
        ]),
    }
}

fn head_type() -> Type {
    record(
        Some("git.Head"),
        vec![
            ("staged", INT),
            ("unstaged", INT),
            ("untracked", INT),
            ("ahead", option(INT)),
            ("behind", option(INT)),
            ("upstream", option(STRING)),
        ],
    )
}

fn checkout_type() -> Type {
    let head = head_type();
    enumeration(
        "git.Checkout",
        vec![
            ("Missing", None),
            ("Invalid", Some(STRING)),
            (
                "OriginMismatch",
                Some(record(None, vec![("branch", option(STRING)), ("head", head.clone()), ("error", STRING)])),
            ),
            ("Detached", Some(head.clone())),
            ("OnBranch", Some(record(None, vec![("branch", STRING), ("head", head)]))),
        ],
    )
}

pub fn git_core() -> Core {
    let (head, checkout) = (head_type(), checkout_type());
    let stale = format!("True when either age is known and at least {} days.", git::STALE_LOCAL_WORK_DAYS);
    let functions = vec![
        function(
            "git.head",
            vec![checkout.clone()],
            option(head.clone()),
            "The HEAD observation of a checkout, or none when the checkout is missing or invalid.",
            ("(git.head Missing)", "none"),
        ),
        function(
            "git.branch",
            vec![checkout.clone()],
            option(STRING),
            "The branch a checkout is on, or none when it has no HEAD or HEAD is detached.",
            ("(git.branch Missing)", "none"),
        ),
        function(
            "git.error",
            vec![checkout.clone()],
            option(STRING),
            "Why a checkout is invalid or has the wrong origin, or none when nothing is wrong.",
            ("(git.error Missing)", "none"),
        ),
        function(
            "git.local-label",
            vec![checkout.clone(), option(STRING), option(INT), option(INT)],
            STRING,
            "Summarizes a checkout, given the default branch, the days on this branch, and the days since the oldest \
             unpushed commit.",
            ("(git.local-label Missing none none none)", "\"missing\""),
        ),
        function(
            "git.age-days",
            vec![INT, option(INT)],
            option(INT),
            "Whole days from a Unix time in seconds to now in milliseconds, never negative, or none when the time is \
             unknown.",
            ("(git.age-days 172800000 (some 0))", "(some 2)"),
        ),
        function(
            "git.stale-work?",
            vec![option(INT), option(INT)],
            BOOL,
            &stale,
            ("(git.stale-work? (some 3) (some 7))", "true"),
        ),
    ];
    Core {
        id: "git".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: vec![("git.Head".to_string(), head), ("git.Checkout".to_string(), checkout)],
        evidence: conformance(&[
            "cores/git/tests/test_git_core.py::test_a_missing_observation_is_never_read_as_clean",
            "cores/git/tests/test_git_core.py::test_every_example_produces_its_documented_result",
            VECTORS_TEST,
            "hosts/repomod/tests/test_git_parity.py::test_local_label_matches_repomod_for_every_status_shape",
            STATUS_PARITY,
        ]),
    }
}

fn nullary_enum(name: &str, variants: &[&str]) -> Type {
    enumeration(name, variants.iter().map(|v| (*v, None)).collect())
}

pub fn github_core() -> Core {
    let ci_states: Vec<&str> = CiState::ALL.iter().map(|s| s.variant()).collect();
    let upstream_states: Vec<&str> = UpstreamState::ALL.iter().map(|s| s.variant()).collect();
    let ci_state = nullary_enum("github.CiState", &ci_states);
    let upstream_state = nullary_enum("github.UpstreamState", &upstream_states);
    let check = record(
        Some("github.Check"),
        vec![
            ("name", STRING),
            ("state", STRING),
            ("source", option(STRING)),
            ("updated-at", option(STRING)),
            ("suite-id", option(STRING)),
            ("run-id", option(STRING)),
            ("attempt", option(INT)),
        ],
    );
    let workflow =
        record(Some("github.Workflow"), vec![("name", STRING), ("state", ci_state.clone()), ("url", option(STRING))]);
    let ci_report = record(
        Some("github.CiReport"),
        vec![
            ("sha", option(STRING)),
            ("default-branch", option(STRING)),
            ("state", ci_state.clone()),
            ("age-seconds", INT),
            ("workflows", option(list(workflow.clone()))),
        ],
    );
    let ci_cache = enumeration("github.CiCache", vec![("NotCached", Some(STRING)), ("Cached", Some(ci_report.clone()))]);
    let upstream = record(
        Some("github.Upstream"),
        vec![
            ("parent", STRING),
            ("parent-branch", option(STRING)),
            ("parent-sha", option(STRING)),
            ("behind-by", INT),
            ("ahead-by", INT),
            ("state", upstream_state.clone()),
            ("age-seconds", INT),
        ],
    );
    let functions = vec![
        function(
            "github.ci-state",
            vec![list(check.clone()), BOOL],
            ci_state.clone(),
            "Aggregates a commit's checks into one state. Reruns replace earlier attempts, a newer external status \
             replaces an older one from the same creator, and an incomplete listing is Unknown.",
            ("(github.ci-state [] true)", "NoChecks"),
        ),
        function(
            "github.ci-state-name",
            vec![ci_state.clone()],
            STRING,
            "The lowercase name of a CI state, with none for NoChecks.",
            ("(github.ci-state-name NoChecks)", "\"none\""),
        ),
        function(
            "github.parse-ci-state",
            vec![STRING],
            ci_state.clone(),
            "Reads a lowercase CI state name. A name that is no state is Unknown, never Passed.",
            ("(github.parse-ci-state \"passed\")", "Passed"),
        ),
        function(
            "github.upstream-state-name",
            vec![upstream_state.clone()],
            STRING,
            "The lowercase name of an upstream comparison state.",
            ("(github.upstream-state-name Diverged)", "\"diverged\""),
        ),
    ];
    let types = vec![
        ("github.CiState", ci_state),
        ("github.UpstreamState", upstream_state),
        ("github.Check", check),
        ("github.Workflow", workflow),
        ("github.CiReport", ci_report),
        ("github.CiCache", ci_cache),
        ("github.Upstream", upstream),
    ];
    Core {
        id: "github".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: types.into_iter().map(|(n, t)| (n.to_string(), t)).collect(),
        evidence: conformance(&[
            "cores/github/tests/test_github_core.py::test_aggregation_cases_from_repomod",
            "cores/github/tests/test_github_core.py::test_every_example_produces_its_documented_result",
            VECTORS_TEST,
            "hosts/repomod/tests/test_github_parity.py::test_ci_state_matches_repomod_for_generated_check_sets",
            STATUS_PARITY,
        ]),
    }
}

fn canonical_decimal(text: &str) -> Result<String, String> {
    Decimal::parse(text).map(Decimal::show).map_err(|error| error.to_string())
}

fn decimal_type() -> Type {
    Type::Nominal(Arc::new(NominalType::validated(
        "decimal.Decimal",
        canonical_decimal,
        "decimal.compare",
        "a string holding a decimal literal such as \"-12.5\", with at most 38 significant and 38 fractional digits",
    )))
}

pub fn decimal_core() -> Core {
    let decimal = decimal_type();
    let modes: Vec<&str> = Rounding::ALL.iter().map(|mode| mode.variant()).collect();
    let rounding = nullary_enum("decimal.Rounding", &modes);
    let quarter = "(decimal.div (decimal.from-int 1) (decimal.from-int 4) 2 HalfEven)";
    let functions = vec![
        function(
            "decimal.parse",
            vec![STRING],
            option(decimal.clone()),
            "Reads an optional minus sign, digits, and optionally a point and digits, or none for anything else, \
             including a value of more than 38 significant or fractional digits.",
            ("(decimal.parse \"12.50\")", "(some \"12.5\")"),
        ),
        function(
            "decimal.show",
            vec![decimal.clone()],
            STRING,
            "Writes a decimal without trailing fractional zeros, so 1.50 is 1.5 and -0 is 0.",
            ("(decimal.show (decimal.from-int -7))", "\"-7\""),
        ),
        function(
            "decimal.format",
            vec![decimal.clone(), INT],
            STRING,
            "Writes a decimal with exactly the given number of places from 0 to 38, rounding half to even.",
            (&format!("(decimal.format {quarter} 1)"), "\"0.2\""),
        ),
        function(
            "decimal.from-int",
            vec![INT],
            decimal.clone(),
            "The decimal equal to an Int.",
            ("(decimal.from-int 42)", "\"42\""),
        ),
        function(
            "decimal.add",
            vec![decimal.clone(), decimal.clone()],
            decimal.clone(),
            "Adds exactly. A result beyond 38 significant or fractional digits stops the program.",
            (&format!("(decimal.add {quarter} {quarter})"), "\"0.5\""),
        ),
        function(
            "decimal.sub",
            vec![decimal.clone(), decimal.clone()],
            decimal.clone(),
            "Subtracts exactly. A result beyond 38 significant or fractional digits stops the program.",
            (&format!("(decimal.sub (decimal.from-int 1) {quarter})"), "\"0.75\""),
        ),
        function(
            "decimal.mul",
            vec![decimal.clone(), decimal.clone()],
            decimal.clone(),
            "Multiplies exactly. A result beyond 38 significant or fractional digits stops the program.",
            (&format!("(decimal.mul {quarter} {quarter})"), "\"0.0625\""),
        ),
        function(
            "decimal.div",
            vec![decimal.clone(), decimal.clone(), INT, rounding.clone()],
            decimal.clone(),
            "Divides and rounds to the given number of places from 0 to 38 with a rounding mode. Dividing by zero \
             stops the program.",
            ("(decimal.div (decimal.from-int 2) (decimal.from-int 3) 4 HalfEven)", "\"0.6667\""),
        ),
        function(
            "decimal.round",
            vec![decimal.clone(), INT, rounding.clone()],
            decimal.clone(),
            "Rounds to the given number of places from 0 to 38 with a rounding mode. HalfEven and HalfUp send ties \
             to the even and the farther neighbor; Down and Up go toward and away from zero; Floor and Ceiling go \
             toward negative and positive infinity.",
            ("(decimal.round (decimal.div (decimal.from-int 5) (decimal.from-int 2) 1 Down) 0 HalfEven)", "\"2\""),
        ),
        function(
            "decimal.compare",
            vec![decimal.clone(), decimal.clone()],
            INT,
            "-1, 0, or 1 as the first decimal is less than, equal to, or greater than the second. Decimals are not \
             sort keys: compare them with this.",
            ("(decimal.compare (decimal.from-int 1) (decimal.from-int 2))", "-1"),
        ),
        function(
            "decimal.to-float",
            vec![decimal.clone()],
            FLOAT,
            "The Float nearest a decimal.",
            (&format!("(decimal.to-float {quarter})"), "0.25"),
        ),
    ];
    Core {
        id: "decimal".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: vec![("decimal.Decimal".to_string(), decimal), ("decimal.Rounding".to_string(), rounding)],
        evidence: vec![
            Evidence {
                level: Assurance::ConformanceTested,
                references: [
                    "rust/sx-cores/tests/decimal_vectors.rs::every_decimal_vector_holds_through_the_kernel",
                    "rust/sx-cores/tests/decimal_vectors.rs::every_decimal_example_produces_its_documented_result",
                    "tests/test_decimal_vectors.py::test_committed_vectors_match_python_decimal",
                ]
                .map(str::to_string)
                .to_vec(),
            },
            Evidence {
                level: Assurance::Specified,
                references: ["lean/Sx/Decimal.lean", "lean/agree.py::decimal_vectors"].map(str::to_string).to_vec(),
            },
        ],
    }
}

pub fn yaml_core() -> Core {
    let document = "\"app:\\n  replicas: 3 # scale\\n  tags:\\n  - a\\n  - b\\n\"";
    let functions = vec![
        function(
            "yaml.get",
            vec![STRING, list(STRING)],
            option(STRING),
            "Reads the scalar at a path of mapping keys and sequence indexes, or none when the path is absent. A \
             null or empty value reads as an empty string. A path that names a mapping or sequence, and text \
             outside the supported YAML subset, are refusals.",
            (&format!("(yaml.get {document} [\"app\" \"replicas\"])"), "(some \"3\")"),
        ),
        function(
            "yaml.set",
            vec![STRING, list(STRING), STRING],
            STRING,
            "Replaces the scalar at a path and keeps every other byte, comments and indentation included. The \
             value is written in the old scalar's quoting style, and quoted when plain text would change its \
             meaning. An absent path, a path that names a mapping or sequence, and text outside the supported \
             YAML subset are refusals.",
            (&format!("(yaml.set {document} [\"app\" \"replicas\"] \"5\")"), "\"app:\\n  replicas: 5 # scale\\n  tags:\\n  - a\\n  - b\\n\""),
        ),
        function(
            "yaml.keys",
            vec![STRING, list(STRING)],
            list(STRING),
            "Lists the keys of the mapping at a path in document order, or the indexes of a sequence as strings. \
             An absent or null path has none. A scalar, and text outside the supported YAML subset, are refusals.",
            (&format!("(yaml.keys {document} [\"app\"])"), "[\"replicas\" \"tags\"]"),
        ),
    ];
    Core {
        id: "yaml".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: Vec::new(),
        evidence: conformance(&[
            "rust/sx-cores/tests/yaml_golden.rs::every_supported_golden_case_reads_and_edits_as_expected",
            "rust/sx-cores/tests/yaml_golden.rs::every_refused_golden_case_fails_with_its_reason",
            "rust/sx-cores/tests/yaml_golden.rs::every_yaml_example_produces_its_documented_result",
            "rust/sx-cores/tests/yaml_properties.rs::editing_a_scalar_changes_only_that_scalar",
        ]),
    }
}

fn pattern_function(name: &str, mut params: Vec<Type>, output: Type, description: &str, example: (&str, &str)) -> Function {
    params.insert(0, STRING);
    let mut found = function(name, params, output, description, example);
    found.literal_checks = vec![LiteralCheck { argument: 0, check: re::validate }];
    found
}

pub fn re_core() -> Core {
    let functions = vec![
        pattern_function(
            "re.find",
            vec![STRING],
            option(list(STRING)),
            "Finds the first match anywhere in the string. The list holds the whole match, then each capture group \
             in order of its opening parenthesis, so (nth m 1) is group 1. A group that did not take part in the \
             match is an empty string. A pattern that is invalid, nests deeper than 50 groups, or compiles to more than \
             1 MiB stops the program with invalid-input; a literal pattern is rejected when the program is checked. \
             Every re function takes the pattern first.",
            ("(re.find #\"(\\d+)-(\\d+)\" \"ab 12-345 x\")", "(some [\"12-345\" \"12\" \"345\"])"),
        ),
        pattern_function(
            "re.find-all",
            vec![STRING],
            list(list(STRING)),
            "Finds every non-overlapping match, left to right, each as re.find lists it. An empty match is found \
             at every position it fits, so a pattern that can match nothing, such as x*, also matches between \
             characters.",
            ("(re.find-all #\"(\\w)=(\\d)\" \"a=1 b=2\")", "[[\"a=1\" \"a\" \"1\"] [\"b=2\" \"b\" \"2\"]]"),
        ),
        pattern_function(
            "re.matches",
            vec![STRING],
            option(list(STRING)),
            "Like re.find, but only when the pattern matches the whole string, so the pattern is anchored at both \
             ends.",
            ("(re.matches #\"(\\d+)\\.(\\d+)\" \"3.14\")", "(some [\"3.14\" \"3\" \"14\"])"),
        ),
        pattern_function(
            "re.test",
            vec![STRING],
            BOOL,
            "True when the pattern matches anywhere in the string.",
            ("(re.test #\"^v\\d+$\" \"v12\")", "true"),
        ),
        pattern_function(
            "re.replace",
            vec![STRING, STRING],
            STRING,
            "Replaces every match in the second string with the third. In the replacement, $1 or ${1} writes a \
             capture group, ${name} a named group, and $$ a dollar sign; write ${1} when a letter or digit \
             follows, because $1x names a group called 1x. A group that does not exist writes nothing.",
            ("(re.replace #\"(\\d+)-(\\d+)\" \"10-20\" \"${2}-${1}\")", "\"20-10\""),
        ),
        pattern_function(
            "re.split",
            vec![STRING],
            list(STRING),
            "Splits the string at every match. Capture groups do not add pieces. A match at either end leaves an \
             empty string there.",
            ("(re.split #\"\\s*,\\s*\" \"a , b,c\")", "[\"a\" \"b\" \"c\"]"),
        ),
    ];
    Core {
        id: "re".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: Vec::new(),
        evidence: conformance(&[
            "rust/sx-cores/tests/re_core.rs::every_re_function_yields_its_documented_result_on_a_table_of_inputs",
            "rust/sx-cores/tests/re_core.rs::a_literal_pattern_that_does_not_compile_is_rejected_at_check_time_at_its_position",
            "rust/sx-cores/tests/re_core.rs::a_computed_pattern_that_does_not_compile_fails_with_invalid_input",
            "rust/sx-cores/tests/re_core.rs::every_re_example_produces_its_documented_result",
        ]),
    }
}

pub fn json_core() -> Core {
    let document = "\"{\\\"app\\\": {\\\"replicas\\\": 3, \\\"tags\\\": [\\\"a\\\", \\\"b\\\"], \\\"debug\\\": null}}\"";
    let kinds: Vec<&str> = json::Kind::ALL.iter().map(|kind| kind.variant()).collect();
    let kind = nullary_enum("json.Kind", &kinds);
    let functions = vec![
        function(
            "json.get",
            vec![STRING, list(STRING)],
            option(STRING),
            "Reads the scalar at a path of object keys and array indexes as text: a string's decoded value, a \
             number exactly as written, or true or false. Gives none when the path is absent or the value is \
             null. A path that names an object or array, and text that is not valid JSON, are refusals.",
            (&format!("(json.get {document} [\"app\" \"replicas\"])"), "(some \"3\")"),
        ),
        function(
            "json.keys",
            vec![STRING, list(STRING)],
            list(STRING),
            "Lists the keys of the object at a path in document order, or the indexes of an array as strings. An \
             absent or null path has none. A scalar, and text that is not valid JSON, are refusals.",
            (&format!("(json.keys {document} [\"app\" \"tags\"])"), "[\"0\" \"1\"]"),
        ),
        function(
            "json.kind",
            vec![STRING, list(STRING)],
            kind.clone(),
            "Names the shape of the value at a path: Absent when the path is absent, then Null, Boolean, \
             Number, String, Array, or Object. Text that is not valid JSON is a refusal.",
            (&format!("(json.kind {document} [\"app\" \"debug\"])"), "Null"),
        ),
    ];
    Core {
        id: "json".to_string(),
        version: "0.1.0".to_string(),
        functions,
        types: vec![("json.Kind".to_string(), kind)],
        evidence: vec![
            Evidence {
                level: Assurance::ConformanceTested,
                references: [
                    "rust/sx-cores/tests/json_golden.rs::every_supported_golden_case_reads_as_expected",
                    "rust/sx-cores/tests/json_golden.rs::every_refused_golden_case_fails_with_its_reason",
                    "rust/sx-cores/tests/json_golden.rs::every_json_example_produces_its_documented_result",
                    "rust/sx-cores/tests/json_properties.rs::generated_documents_read_back_as_built",
                ]
                .map(str::to_string)
                .to_vec(),
            },
            Evidence {
                level: Assurance::Specified,
                references: ["lean/Sx/JsonCore.lean", "lean/agree.py::json_golden"].map(str::to_string).to_vec(),
            },
        ],
    }
}

pub fn cores() -> Vec<Core> {
    vec![text_core(), semver_core(), git_core(), github_core(), decimal_core(), yaml_core(), re_core(), json_core()]
}

pub fn core(id: &str) -> Option<Core> {
    cores().into_iter().find(|core| core.id == id)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredError {
    Load(LoadError),
    Registry(RegistryError),
    UnknownCore(String),
}

impl fmt::Display for DeclaredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeclaredError::Load(error) => error.fmt(f),
            DeclaredError::Registry(error) => error.fmt(f),
            DeclaredError::UnknownCore(id) => write!(f, "no core is named {id:?}"),
        }
    }
}

impl std::error::Error for DeclaredError {}

pub fn declared_registry(declarations: &Json) -> Result<Registry, DeclaredError> {
    let mut registry = Registry::new();
    let ids = declarations.get("cores").and_then(Json::as_array).unwrap_or(&[]);
    for id in ids {
        let id = id.as_str().ok_or_else(|| DeclaredError::UnknownCore(format!("{id:?}")))?;
        let found = core(id).ok_or_else(|| DeclaredError::UnknownCore(id.to_string()))?;
        registry.add_core(found).map_err(DeclaredError::Registry)?;
    }
    load_declarations(&mut registry, declarations).map_err(DeclaredError::Load)?;
    Ok(registry)
}

fn malformed(function: &str) -> CoreError {
    CoreError::Failed(format!("{function} received arguments that do not match its signature"))
}

fn string_of(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s),
        _ => None,
    }
}

fn int_of(value: &Value) -> Option<i64> {
    match value {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

fn bool_of(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn optional<'v, T>(value: &'v Value, read: impl FnOnce(&'v Value) -> Option<T>) -> Option<Option<T>> {
    match value {
        Value::None => Some(None),
        Value::Some(inner) => read(inner).map(Some),
        _ => None,
    }
}

fn record_of(value: &Value) -> Option<&RecordValue> {
    match value {
        Value::Record(record) => Some(record),
        _ => None,
    }
}

fn field<'v>(record: &'v RecordValue, name: &str) -> Option<&'v Value> {
    record.get(name)
}

fn strings_of(value: &Value) -> Option<Vec<&str>> {
    match value {
        Value::List(items) => items.iter().map(string_of).collect(),
        _ => None,
    }
}

fn string_list(items: Vec<String>) -> Value {
    Value::list(items.iter().map(|s| Value::string(s)).collect())
}

fn some_or_none(value: Option<Value>) -> Value {
    value.map_or(Value::None, Value::some)
}

fn version_of(value: &Value) -> Option<Version> {
    let record = record_of(value)?;
    Some(Version {
        major: int_of(field(record, "major")?)?,
        minor: int_of(field(record, "minor")?)?,
        patch: int_of(field(record, "patch")?)?,
    })
}

fn version_value(version: Version) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("major"), Value::Int(version.major)),
        (Arc::from("minor"), Value::Int(version.minor)),
        (Arc::from("patch"), Value::Int(version.patch)),
    ]))
}

fn head_of(value: &Value) -> Option<Head> {
    let record = record_of(value)?;
    Some(Head {
        staged: int_of(field(record, "staged")?)?,
        unstaged: int_of(field(record, "unstaged")?)?,
        untracked: int_of(field(record, "untracked")?)?,
        ahead: optional(field(record, "ahead")?, int_of)?,
        behind: optional(field(record, "behind")?, int_of)?,
        upstream: optional(field(record, "upstream")?, string_of)?.map(str::to_string),
    })
}

fn optional_string(value: Option<&str>) -> Value {
    value.map_or(Value::None, |s| Value::some(Value::string(s)))
}

fn optional_int(value: Option<i64>) -> Value {
    value.map_or(Value::None, |n| Value::some(Value::Int(n)))
}

fn head_value(head: &Head) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("staged"), Value::Int(head.staged)),
        (Arc::from("unstaged"), Value::Int(head.unstaged)),
        (Arc::from("untracked"), Value::Int(head.untracked)),
        (Arc::from("ahead"), optional_int(head.ahead)),
        (Arc::from("behind"), optional_int(head.behind)),
        (Arc::from("upstream"), optional_string(head.upstream.as_deref())),
    ]))
}

fn checkout_of(value: &Value) -> Option<Checkout> {
    let Value::Variant(variant) = value else {
        return None;
    };
    let payload = variant.payload.as_ref();
    Some(match &*variant.variant {
        "Missing" => Checkout::Missing,
        "Invalid" => Checkout::Invalid(string_of(payload?)?.to_string()),
        "OriginMismatch" => {
            let record = record_of(payload?)?;
            Checkout::OriginMismatch {
                branch: optional(field(record, "branch")?, string_of)?.map(str::to_string),
                head: head_of(field(record, "head")?)?,
                error: string_of(field(record, "error")?)?.to_string(),
            }
        }
        "Detached" => Checkout::Detached(head_of(payload?)?),
        "OnBranch" => {
            let record = record_of(payload?)?;
            Checkout::OnBranch {
                branch: string_of(field(record, "branch")?)?.to_string(),
                head: head_of(field(record, "head")?)?,
            }
        }
        _ => return None,
    })
}

fn check_of(value: &Value) -> Option<Check> {
    let record = record_of(value)?;
    let text = |name: &str| optional(field(record, name)?, string_of).map(|s| s.map(str::to_string));
    Some(Check {
        name: string_of(field(record, "name")?)?.to_string(),
        state: string_of(field(record, "state")?)?.to_string(),
        source: text("source")?,
        updated_at: text("updated-at")?,
        suite_id: text("suite-id")?,
        run_id: text("run-id")?,
        attempt: optional(field(record, "attempt")?, int_of)?,
    })
}

fn variant_name(value: &Value) -> Option<&str> {
    match value {
        Value::Variant(variant) if variant.payload.is_none() => Some(&variant.variant),
        _ => None,
    }
}

fn ci_state_of(value: &Value) -> Option<CiState> {
    let name = variant_name(value)?;
    CiState::ALL.into_iter().find(|s| s.variant() == name)
}

fn upstream_state_of(value: &Value) -> Option<UpstreamState> {
    let name = variant_name(value)?;
    UpstreamState::ALL.into_iter().find(|s| s.variant() == name)
}

fn ci_state_value(state: CiState) -> Value {
    Value::variant("github.CiState", state.variant(), None)
}

fn arithmetic<T>(result: Result<T, DecimalError>) -> Result<T, CoreError> {
    result.map_err(|error| CoreError::Arithmetic(error.to_string()))
}

fn decimal_of(value: &Value) -> Option<Decimal> {
    Decimal::parse(string_of(value)?).ok()
}

fn decimal_value(decimal: Decimal) -> Value {
    Value::string(&decimal.show())
}

fn rounding_of(value: &Value) -> Option<Rounding> {
    Rounding::from_variant(variant_name(value)?)
}

fn decimal_call(function: &str, args: &[Value]) -> Option<Result<Value, CoreError>> {
    let value = match (function, args) {
        ("decimal.parse", [s]) => Ok(some_or_none(Decimal::parse(string_of(s)?).ok().map(decimal_value))),
        ("decimal.show", [d]) => Ok(Value::string(&decimal_of(d)?.show())),
        ("decimal.format", [d, places]) => arithmetic(decimal_of(d)?.format(int_of(places)?)).map(|text| Value::string(&text)),
        ("decimal.from-int", [n]) => Ok(decimal_value(Decimal::from_int(int_of(n)?))),
        ("decimal.add", [a, b]) => arithmetic(decimal_of(a)?.sum(decimal_of(b)?)).map(decimal_value),
        ("decimal.sub", [a, b]) => arithmetic(decimal_of(a)?.subtract(decimal_of(b)?)).map(decimal_value),
        ("decimal.mul", [a, b]) => arithmetic(decimal_of(a)?.multiply(decimal_of(b)?)).map(decimal_value),
        ("decimal.div", [a, b, places, mode]) => {
            arithmetic(decimal_of(a)?.divide(decimal_of(b)?, int_of(places)?, rounding_of(mode)?)).map(decimal_value)
        }
        ("decimal.round", [d, places, mode]) => {
            arithmetic(decimal_of(d)?.round(int_of(places)?, rounding_of(mode)?)).map(decimal_value)
        }
        ("decimal.compare", [a, b]) => Ok(Value::Int(decimal_of(a)?.compare(decimal_of(b)?) as i64)),
        ("decimal.to-float", [d]) => match decimal_of(d)?.to_float() {
            Some(float) => Ok(Value::float(float)),
            None => Err(CoreError::Failed("the decimal has no finite Float".to_string())),
        },
        _ => return None,
    };
    Some(value)
}

fn yaml_call(function: &str, args: &[Value]) -> Option<Result<Value, CoreError>> {
    let outcome = match (function, args) {
        ("yaml.get", [text, path]) => {
            let found = yaml::get(string_of(text)?, &strings_of(path)?);
            found.map(|found| optional_string(found.as_deref()))
        }
        ("yaml.set", [text, path, value]) => {
            let edited = yaml::set(string_of(text)?, &strings_of(path)?, string_of(value)?);
            edited.map(|edited| Value::string(&edited))
        }
        ("yaml.keys", [text, path]) => yaml::keys(string_of(text)?, &strings_of(path)?).map(string_list),
        _ => return Some(Err(CoreError::Unimplemented)),
    };
    Some(outcome.map_err(|error| CoreError::InvalidInput(error.to_string())))
}

fn re_call(function: &str, args: &[Value]) -> Option<Result<Value, CoreError>> {
    let outcome = match (function, args) {
        ("re.find", [pattern, s]) => re::find(string_of(pattern)?, string_of(s)?).map(optional_groups),
        ("re.find-all", [pattern, s]) => re::find_all(string_of(pattern)?, string_of(s)?)
            .map(|found| Value::list(found.into_iter().map(string_list).collect())),
        ("re.matches", [pattern, s]) => re::matches(string_of(pattern)?, string_of(s)?).map(optional_groups),
        ("re.test", [pattern, s]) => re::test(string_of(pattern)?, string_of(s)?).map(Value::Bool),
        ("re.replace", [pattern, s, replacement]) => {
            re::replace(string_of(pattern)?, string_of(s)?, string_of(replacement)?).map(|text| Value::string(&text))
        }
        ("re.split", [pattern, s]) => re::split(string_of(pattern)?, string_of(s)?).map(string_list),
        _ => return Some(Err(CoreError::Unimplemented)),
    };
    Some(outcome.map_err(CoreError::InvalidInput))
}

fn optional_groups(found: Option<Vec<String>>) -> Value {
    some_or_none(found.map(string_list))
}

fn json_call(function: &str, args: &[Value]) -> Option<Result<Value, CoreError>> {
    let outcome = match (function, args) {
        ("json.get", [text, path]) => json::get(string_of(text)?, &strings_of(path)?).map(|found| optional_string(found.as_deref())),
        ("json.keys", [text, path]) => json::keys(string_of(text)?, &strings_of(path)?).map(string_list),
        ("json.kind", [text, path]) => {
            json::kind(string_of(text)?, &strings_of(path)?).map(|kind| Value::variant("json.Kind", kind.variant(), None))
        }
        _ => return Some(Err(CoreError::Unimplemented)),
    };
    Some(outcome.map_err(|error| CoreError::InvalidInput(error.to_string())))
}

pub struct Cores;

impl Cores {
    fn evaluate(function: &str, args: &[Value]) -> Option<Result<Value, CoreError>> {
        let value = match (function, args) {
            ("text.casefold", [s]) => Value::string(&text::casefold(string_of(s)?)),
            ("text.split", [s, separator]) => match text::split(string_of(s)?, string_of(separator)?) {
                Ok(parts) => string_list(parts),
                Err(TextError::EmptySeparator) => return Some(Err(CoreError::InvalidInput("empty separator".to_string()))),
            },
            ("text.join", [separator, parts]) => Value::string(&text::join(string_of(separator)?, &strings_of(parts)?)),
            ("text.chars", [s]) => string_list(text::chars(string_of(s)?)),
            ("text.trim", [s]) => Value::string(&text::trim(string_of(s)?)),
            ("text.length", [s]) => Value::Int(text::length(string_of(s)?)),
            ("text.starts-with?", [s, start]) => Value::Bool(text::starts_with(string_of(s)?, string_of(start)?)),
            ("text.ends-with?", [s, end]) => Value::Bool(text::ends_with(string_of(s)?, string_of(end)?)),
            ("text.contains?", [s, part]) => Value::Bool(text::contains(string_of(s)?, string_of(part)?)),
            ("text.replace", [s, old, new]) => {
                Value::string(&text::replace(string_of(s)?, string_of(old)?, string_of(new)?))
            }
            ("text.from-int", [n]) => Value::string(&text::from_int(int_of(n)?)),
            ("text.to-int", [s]) => optional_int(text::to_int(string_of(s)?)),
            ("semver.parse", [s]) => some_or_none(semver::parse(string_of(s)?).map(version_value)),
            ("semver.compare", [a, b]) => Value::Int(semver::compare(version_of(a)?, version_of(b)?)),
            ("semver.show", [v]) => Value::string(&semver::show(version_of(v)?)),
            ("git.head", [c]) => some_or_none(git::head(&checkout_of(c)?).map(head_value)),
            ("git.branch", [c]) => optional_string(git::branch(&checkout_of(c)?)),
            ("git.error", [c]) => optional_string(git::error(&checkout_of(c)?)),
            ("git.local-label", [c, default_branch, branch_days, unpushed_days]) => Value::string(&git::local_label(
                &checkout_of(c)?,
                optional(default_branch, string_of)?,
                optional(branch_days, int_of)?,
                optional(unpushed_days, int_of)?,
            )),
            ("git.age-days", [now, since]) => optional_int(git::age_days(int_of(now)?, optional(since, int_of)?)),
            ("git.stale-work?", [branch_days, unpushed_days]) => {
                Value::Bool(git::stale_work(optional(branch_days, int_of)?, optional(unpushed_days, int_of)?))
            }
            ("github.ci-state", [checks, complete]) => {
                let Value::List(items) = checks else {
                    return None;
                };
                let checks: Vec<Check> = items.iter().map(check_of).collect::<Option<_>>()?;
                ci_state_value(github::ci_state(&checks, bool_of(complete)?))
            }
            ("github.ci-state-name", [state]) => Value::string(github::ci_state_name(ci_state_of(state)?)),
            ("github.parse-ci-state", [label]) => ci_state_value(github::parse_ci_state(string_of(label)?)),
            ("github.upstream-state-name", [state]) => {
                Value::string(github::upstream_state_name(upstream_state_of(state)?))
            }
            (function, args) if function.starts_with("decimal.") => return decimal_call(function, args),
            (function, args) if function.starts_with("yaml.") => return yaml_call(function, args),
            (function, args) if function.starts_with("json.") => return json_call(function, args),
            (function, args) if function.starts_with("re.") => return re_call(function, args),
            _ => return Some(Err(CoreError::Unimplemented)),
        };
        Some(Ok(value))
    }
}

impl CoreImplementations for Cores {
    fn apply(&self, function: &str, args: &[Value]) -> Result<Value, CoreError> {
        Cores::evaluate(function, args).unwrap_or_else(|| Err(malformed(function)))
    }
}
