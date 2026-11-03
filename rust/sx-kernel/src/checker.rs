use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::authority::{Authority, Grant, Screen, Verdict};
use crate::budgets::Budgets;
use crate::diagnostics::Diagnostic;
use crate::ir::{
    self, Binder, Literal, Conversion, HigherOrder, Lambda, MatchPattern, NodeKind as Ir, PreludeOp, WidenedField, WrapKind,
};
use crate::registry::{Capability, Effect, Function, Registry};
use crate::syntax::{Items, Node, NodeKind, Span, show};
use crate::types::{type_depth, 
    BOOL, BYTES, CallErrorKind, EnumType, FLOAT, INT, NominalType, Primitive, RecordType, STRING, Type, UNIT, call_error,
    call_error_kind, encodable_problem, show_type,
};
use crate::values::{MapValue, Value, show_value};

mod js;
mod script;

pub use script::disagreement as input_disagreement;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    Sx,
    JavaScript,
}

const SPECIAL_FORMS: [&str; 18] = [
    "let", "if", "cond", "match", "fn", "get", "call", "attempt", "retry", "wait-until", "loop", "recur", "par",
    "par-map", "some", "ok", "err", "sleep",
];
const RESERVED_SUGGESTIONS: [(&str, &str); 13] = [
    ("ok", "ok?"),
    ("err", "error"),
    ("some", "found"),
    ("none", "missing"),
    ("fn", "f"),
    ("if", "test"),
    ("get", "fetched"),
    ("call", "called"),
    ("match", "matched"),
    ("retry", "retries"),
    ("let", "bound"),
    ("cond", "cases"),
    ("sleep", "slept"),
];
const HIGHER_ORDER: [&str; 9] = ["map", "filter", "keep", "any?", "all?", "reduce", "sort-by", "distinct-by", "group-by"];
const PRELUDE: [&str; 45] = [
    "+", "-", "*", "/", "quot", "rem", "float", "round", "floor", "ceil", "=", "not=", "<", "<=", ">", ">=", "not",
    "and", "or", "str", "to-json", "count", "first", "rest", "nth", "take", "concat", "flatten", "contains?", "identity",
    "assoc", "dissoc", "keys", "vals", "entries", "into-map", "map", "filter", "keep", "any?", "all?", "reduce",
    "sort-by", "distinct-by", "group-by",
];
const UNSUGGESTED_PRELUDE: [&str; 4] = ["put", "range", "enumerate", "safe-int"];
const FIXED_ARITY: [(&str, usize); 46] = [
    ("+", 2),
    ("*", 2),
    ("/", 2),
    ("quot", 2),
    ("rem", 2),
    ("float", 1),
    ("round", 1),
    ("floor", 1),
    ("ceil", 1),
    ("=", 2),
    ("not=", 2),
    ("<", 2),
    ("<=", 2),
    (">", 2),
    (">=", 2),
    ("not", 1),
    ("and", 2),
    ("identity", 1),
    ("or", 2),
    ("count", 1),
    ("to-json", 1),
    ("first", 1),
    ("rest", 1),
    ("nth", 2),
    ("take", 2),
    ("concat", 2),
    ("flatten", 1),
    ("contains?", 2),
    ("map", 2),
    ("filter", 2),
    ("keep", 2),
    ("any?", 2),
    ("all?", 2),
    ("reduce", 3),
    ("sort-by", 2),
    ("distinct-by", 2),
    ("group-by", 2),
    ("assoc", 3),
    ("dissoc", 2),
    ("keys", 1),
    ("vals", 1),
    ("entries", 1),
    ("into-map", 1),
    ("put", 3),
    ("enumerate", 1),
    ("safe-int", 1),
];
const DEFINITION: &str = "A definition belongs at the top level, before the program's final expression. Inside an expression, bind a local function with let: (let [f (fn [x] ...)] ...).";
const HABIT_HEADS: [(&str, &str, &str); 12] = [
    ("defn", "defn", DEFINITION),
    ("defn-", "defn", DEFINITION),
    (
        "def",
        "defn",
        "A definition belongs at the top level, before the program's final expression. Inside an expression, bind values with let.",
    ),
    ("while", "while", "Sx has no unbounded loops. Map over a list, or use loop with :max."),
    ("for", "while", "Sx has no for. Map over a list, or use loop with :max."),
    ("doseq", "while", "Sx has no doseq. Map over a list, and bind the result to _ if you only need the effects."),
    ("dotimes", "while", "Sx has no dotimes. Use loop with :max."),
    (
        "assoc",
        "assoc",
        "assoc takes a Map and a string key. For a record, build a new record literal: {:a (get r :a) :b value}.",
    ),
    ("assoc-in", "assoc", "Sx has no assoc-in. Build a new record literal."),
    ("update", "assoc", "Sx has no update. Build a new record literal."),
    ("merge", "assoc", "Sx has no merge. Build a new record literal."),
    (
        "dissoc",
        "assoc",
        "dissoc takes a Map and a string key. For a record, build a new record literal without the field.",
    ),
];
const MISSING_FUNCTIONS: [(&str, &str); 20] = [
    ("inc", "Add 1 instead: (+ x 1)."),
    ("dec", "Subtract 1 instead: (- x 1)."),
    ("last", "Use (nth xs (- (count xs) 1))."),
    ("empty?", "Use (= (count xs) 0)."),
    ("nil?", "Match on the optional value: (match x (some v) ... none ...)."),
    ("some?", "Match on the optional value: (match x (some v) ... none ...)."),
    ("into", "Use concat to join two lists."),
    ("conj", "Use concat with a one-element list: (concat xs [x])."),
    ("max", "Use reduce: (reduce (fn [m x] (if (> x m) x m)) (first xs) (rest xs))."),
    ("min", "Use reduce: (reduce (fn [m x] (if (< x m) x m)) (first xs) (rest xs))."),
    ("apply", "Call the function with its arguments directly."),
    ("println", "A program returns its result. It does not print."),
    ("print", "A program returns its result. It does not print."),
    ("remove", "Use filter with the opposite test."),
    ("sort", "Use sort-by with the value itself as the key: (sort-by (fn [x] x) xs)."),
    ("re-find", "Use re.find, as in (re.find #\"(\\d+)\" s). The re core must be in the declarations' \"cores\"."),
    ("re-matches", "Use re.matches, as in (re.matches #\"\\d+\" s). The re core must be in the declarations' \"cores\"."),
    ("re-seq", "Use re.find-all, as in (re.find-all #\"\\d+\" s). The re core must be in the declarations' \"cores\"."),
    ("re-pattern", "A pattern is a String: write #\"\\d+\" and pass it to re.find. The re core must be in the declarations' \"cores\"."),
    ("re-groups", "re.find already returns the whole match and then each group: (nth m 1) is group 1."),
];
const DEFAULT_BINDER: &str = "#present";
const PUT_BINDER: &str = "#put";
const INT_TO_FLOAT_HINT: &str = "Sx does not convert between number types. (float n) turns an Int into a Float.";
const FLOAT_TO_INT_HINT: &str =
    "Sx does not convert between number types. (round x), (floor x), and (ceil x) turn a Float into an Int.";
const BYTES_HINT: &str = "Bytes come from tools. No Sx function builds them from a string or a list.";
const OPTIONAL_HINT: &str = "This value is optional. Take it apart first: (match x (some v) ... none ...).";
const UNFILLED_LOOP_HINT: &str = "A loop binding that starts as [], {}, or none gets its type from the values that recur gives it, and no recur here does. Give recur a value of the right shape, or start the binding with one.";
const UNDOCUMENTED_FORMS: [&str; 2] = ["when", "do"];
const WHEN_HINT: &str = "when is optional: none when its test is false. Use if with an else value, or match on it: (match x (some v) ... none ...).";
const MAX_REPEATS: usize = 10_000;

pub(crate) fn is_special_form(name: &str) -> bool {
    SPECIAL_FORMS.contains(&name)
}

fn is_reserved(name: &str) -> bool {
    is_special_form(name) || name == "none"
}

fn is_prelude(name: &str) -> bool {
    PRELUDE.contains(&name) || UNSUGGESTED_PRELUDE.contains(&name)
}

fn is_higher_order(name: &str) -> bool {
    HIGHER_ORDER.contains(&name)
}

fn fixed_arity(op: &str) -> Option<usize> {
    FIXED_ARITY.iter().find(|(name, _)| *name == op).map(|(_, n)| *n)
}

fn habit_head(name: &str) -> Option<(&'static str, &'static str)> {
    HABIT_HEADS.iter().find(|(head, _, _)| *head == name).map(|(_, c, m)| (*c, *m))
}

fn missing_function(name: &str) -> Option<&'static str> {
    MISSING_FUNCTIONS.iter().find(|(n, _)| *n == name).map(|(_, hint)| *hint)
}

fn test_subject(who: &str) -> &'static str {
    match who {
        "if" => "An if condition",
        "when" => "A when condition",
        _ => "A cond test",
    }
}

fn test_help(who: &str) -> &str {
    if who == "when" { "if" } else { who }
}

fn binary_signature(op: &str) -> Option<&'static str> {
    Some(match op {
        "+" | "*" | "/" => "a b",
        "quot" | "rem" => "Int Int",
        "float" => "n",
        "round" | "floor" | "ceil" => "x",
        _ => return None,
    })
}

fn signature(op: &str) -> &'static str {
    match op {
        "=" | "not=" | "<" | "<=" | ">" | ">=" => "a b",
        "not" => "Bool",
        "and" | "or" => "Bool Bool",
        "count" | "first" | "rest" => "xs",
        "nth" => "xs i",
        "take" => "n xs",
        "concat" => "xs ys",
        "flatten" => "xss",
        "contains?" => "xs x",
        "reduce" => "f init xs",
        "identity" | "to-json" => "x",
        "assoc" => "map key value",
        "dissoc" => "map key",
        "keys" | "vals" | "entries" => "map",
        "into-map" => "pairs",
        "put" => "target key value",
        "range" => "start end [step]",
        "enumerate" => "xs",
        "safe-int" => "n",
        _ => binary_signature(op).unwrap_or("f xs"),
    }
}

fn first_upper(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(crate) fn is_plain_name(name: &str) -> bool {
    name.strip_prefix('_').unwrap_or(name).chars().next().is_some_and(char::is_lowercase)
}

fn option(t: Type) -> Type {
    Type::Option(Box::new(t))
}

fn list(t: Type) -> Type {
    Type::List(Box::new(t))
}

fn map_of(t: Type) -> Type {
    Type::Map(Box::new(t))
}

fn result_of(ok: Type, err: Type) -> Type {
    Type::Result(Box::new(ok), Box::new(err))
}

fn empty_record() -> Type {
    Type::Record(RecordType::plain(Vec::new()))
}

fn mentions_never(t: &Type) -> bool {
    match t {
        Type::Never => true,
        Type::List(inner) | Type::Map(inner) | Type::Option(inner) => mentions_never(inner),
        Type::Tuple(items) => items.iter().any(mentions_never),
        Type::Result(ok, err) => mentions_never(ok) || mentions_never(err),
        Type::Record(record) => record.fields.iter().any(|(_, field_type)| mentions_never(field_type)),
        _ => false,
    }
}

fn record_names(record: &RecordType) -> BTreeSet<&str> {
    record.fields.iter().map(|(name, _)| name.as_str()).collect()
}

fn always_recurs(at: &ir::Node) -> bool {
    match &at.kind {
        Ir::Recur { .. } => true,
        Ir::Let(ir::Let { body, .. }) | Ir::LocalFunction { body, .. } | Ir::Coerce { value: body, .. } => {
            always_recurs(body)
        }
        Ir::If(ir::If { then, otherwise, .. }) => always_recurs(then) && always_recurs(otherwise),
        Ir::Match(ir::Match { clauses, .. }) => clauses.iter().all(|(_, body)| always_recurs(body)),
        _ => false,
    }
}

fn float_fix(source: Option<&Node>) -> Option<String> {
    let source = source?;
    if let NodeKind::Symbol(name) = &source.kind
        && name.starts_with('#')
    {
        return None;
    }
    Some(format!("(float {})", show(source)))
}

fn article(text: &str) -> String {
    if text.chars().next().is_none_or(|c| "AEIOU".contains(c)) { format!("an {text}") } else { format!("a {text}") }
}

pub fn describe(t: &Type) -> String {
    if matches!(t, Type::Record(_)) && !t.is_call_error() {
        return "a record".to_string();
    }
    if *t == BYTES {
        return "Bytes".to_string();
    }
    article(&show_type(t))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ca != *cb);
            current.push((previous[j + 1] + 1).min(current[j] + 1).min(substitute));
        }
        previous = current;
    }
    previous[b.len()]
}

pub fn closest<S: AsRef<str>>(name: &str, candidates: &[S]) -> Option<String> {
    candidates
        .iter()
        .map(AsRef::as_ref)
        .filter(|c| *c != name)
        .map(|c| (edit_distance(name, c), c))
        .min()
        .filter(|(distance, _)| *distance <= 2)
        .map(|(_, c)| c.to_string())
}

fn name_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [a, b] => format!("{a} and {b}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
    }
}

fn suggested_name(name: &str) -> String {
    if let Some((_, suggestion)) = RESERVED_SUGGESTIONS.iter().find(|(n, _)| *n == name) {
        return (*suggestion).to_string();
    }
    if ["s", "x", "ch", "sh"].iter().any(|end| name.ends_with(end)) { format!("{name}es") } else { format!("{name}s") }
}

fn symbol(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Symbol(name) => Some(name),
        _ => None,
    }
}

fn keyword(node: &Node) -> Option<&str> {
    match &node.kind {
        NodeKind::Keyword(name) => Some(name),
        _ => None,
    }
}

fn form_items(node: &Node) -> Option<&[Node]> {
    match &node.kind {
        NodeKind::Form(items) => Some(items),
        _ => None,
    }
}

fn list_items(node: &Node) -> Option<&[Node]> {
    match &node.kind {
        NodeKind::List(items) => Some(items),
        _ => None,
    }
}

fn head_name(node: &Node) -> Option<&str> {
    form_items(node)?.first().and_then(symbol)
}

fn is_function_form(node: &Node) -> bool {
    head_name(node) == Some("fn")
}

fn is_else(node: &Node) -> bool {
    keyword(node) == Some("else")
}

fn pattern_names(node: &Node) -> Vec<String> {
    match &node.kind {
        NodeKind::Symbol(name) if is_plain_name(name) && name != "none" => vec![name.clone()],
        NodeKind::Form(items) => items.iter().skip(1).flat_map(pattern_names).collect(),
        NodeKind::List(items) => items.iter().flat_map(pattern_names).collect(),
        _ => Vec::new(),
    }
}

fn tuple_names(count: usize) -> String {
    let names: Vec<String> = (0..count).map(|i| format!("x{i}")).collect();
    format!("[{}] tuple", names.join(" "))
}

fn habit(construct: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("habit", span, message).construct(construct)
}

fn node(kind: Ir, span: Span) -> ir::Node {
    ir::Node { kind, span }
}

fn literal(value: Value, value_type: Type, span: Span) -> ir::Node {
    node(Ir::Literal { value, value_type }, span)
}

fn prelude(op: PreludeOp, args: Vec<ir::Node>, span: Span) -> ir::Node {
    node(Ir::Prelude { op, args }, span)
}

fn higher(op: &str) -> Option<HigherOrder> {
    match PreludeOp::from_name(op)? {
        PreludeOp::HigherOrder(h) => Some(h),
        _ => None,
    }
}

fn holds_map(t: &Type) -> bool {
    match t {
        Type::Map(_) => true,
        Type::List(inner) | Type::Option(inner) => holds_map(inner),
        Type::Tuple(items) => items.iter().any(holds_map),
        Type::Record(record) => record.fields.iter().any(|(_, field)| holds_map(field)),
        Type::Result(ok, err) => holds_map(ok) || holds_map(err),
        Type::Enum(e) => e.variants.iter().any(|(_, payload)| payload.as_ref().is_some_and(holds_map)),
        _ => false,
    }
}

fn lambda_node(lambda: Lambda) -> ir::Node {
    let span = lambda.span;
    node(Ir::Lambda(lambda), span)
}

fn empty_lambda(span: Span, captured: Option<String>) -> Lambda {
    Lambda {
        params: Vec::new(),
        param_types: Vec::new(),
        result: Type::Never,
        body: Box::new(literal(Value::Unit, UNIT, span)),
        captured,
        span,
    }
}

#[derive(Clone, Debug)]
pub struct CheckResult {
    pub program: Option<ir::Program>,
    pub diagnostics: Vec<Diagnostic>,
    pub header_inputs: Option<BTreeMap<String, Type>>,
}

impl CheckResult {
    fn rejected(diagnostics: Vec<Diagnostic>) -> CheckResult {
        CheckResult { program: None, diagnostics, header_inputs: None }
    }
}

struct FunctionBinding<'a> {
    form: &'a Node,
    scope: Scope<'a>,
    name: String,
}

#[derive(Clone)]
enum Binding<'a> {
    Value(Type),
    Function(Rc<FunctionBinding<'a>>),
}

struct Frame<'a> {
    name: String,
    binding: Binding<'a>,
    variants: Option<Vec<String>>,
    anchor: Option<Rc<Frame<'a>>>,
    parent: Scope<'a>,
}

#[derive(Clone)]
enum Fact {
    Present(Type),
    Variants { enum_type: Type, names: Vec<String> },
}

type Narrowed = Vec<(String, Fact)>;

#[derive(Clone, Default)]
struct Scope<'a>(Option<Rc<Frame<'a>>>);

impl<'a> Scope<'a> {
    fn get(&self, name: &str) -> Option<&Binding<'a>> {
        let mut at = self.0.as_deref();
        while let Some(frame) = at {
            if frame.name == name {
                return Some(&frame.binding);
            }
            at = frame.parent.0.as_deref();
        }
        None
    }

    fn with(&self, name: &str, binding: Binding<'a>) -> Scope<'a> {
        Scope(Some(Rc::new(Frame { name: name.to_string(), binding, variants: None, anchor: None, parent: self.clone() })))
    }

    fn with_variants(&self, name: &str, enum_type: Type, names: Vec<String>) -> Scope<'a> {
        let frame = Frame { name: name.to_string(), binding: Binding::Value(enum_type), variants: Some(names), anchor: None, parent: self.clone() };
        Scope(Some(Rc::new(frame)))
    }

    fn frame_of(&self, name: &str) -> Option<&Rc<Frame<'a>>> {
        let mut at = self.0.as_ref();
        while let Some(frame) = at {
            if frame.name == name {
                return Some(frame);
            }
            at = frame.parent.0.as_ref();
        }
        None
    }

    fn with_path(&self, path: &str, t: Type) -> Scope<'a> {
        let root = path.split('.').next().unwrap_or(path);
        let frame = Frame {
            name: path.to_string(),
            binding: Binding::Value(t),
            variants: None,
            anchor: self.frame_of(root).cloned(),
            parent: self.clone(),
        };
        Scope(Some(Rc::new(frame)))
    }

    fn narrowed_path(&self, path: &str) -> Option<&Type> {
        let frame = self.frame_of(path)?;
        let root = self.frame_of(path.split('.').next().unwrap_or(path))?;
        let anchor = frame.anchor.as_ref()?;
        match &frame.binding {
            Binding::Value(t) if Rc::ptr_eq(anchor, root) => Some(t),
            _ => None,
        }
    }

    fn variants_of(&self, name: &str) -> Option<&[String]> {
        let mut at = self.0.as_deref();
        while let Some(frame) = at {
            if frame.name == name {
                return frame.variants.as_deref();
            }
            at = frame.parent.0.as_deref();
        }
        None
    }

    fn narrow(&self, narrowed: &Narrowed) -> Scope<'a> {
        let mut scope = self.clone();
        for (name, fact) in narrowed {
            scope = match fact {
                Fact::Present(inner) if name.contains('.') => scope.with_path(name, inner.clone()),
                Fact::Present(inner) => scope.value(name, inner.clone()),
                Fact::Variants { enum_type, names } => scope.with_variants(name, enum_type.clone(), names.clone()),
            };
        }
        scope
    }

    fn value(&self, name: &str, t: Type) -> Scope<'a> {
        self.with(name, Binding::Value(t))
    }

    fn is_value(&self, name: &str) -> bool {
        matches!(self.get(name), Some(Binding::Value(_)))
    }

    fn visible(&self, functions: bool) -> Vec<String> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut found = Vec::new();
        let mut at = self.0.as_deref();
        while let Some(frame) = at {
            if seen.insert(&frame.name) && matches!(frame.binding, Binding::Function(_)) == functions {
                found.push(frame.name.clone());
            }
            at = frame.parent.0.as_deref();
        }
        found
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        let mut next = self.0.take();
        while let Some(frame) = next {
            match Rc::try_unwrap(frame) {
                Ok(mut frame) => next = frame.parent.0.take(),
                Err(_) => break,
            }
        }
    }
}

struct Test {
    condition: ir::Node,
    presence: bool,
    name: Option<String>,
    inner: Option<Type>,
    narrow_true: Narrowed,
    narrow_false: Narrowed,
}

impl Test {
    fn plain(condition: ir::Node) -> Test {
        Test { condition, presence: false, name: None, inner: None, narrow_true: Vec::new(), narrow_false: Vec::new() }
    }
}

#[derive(Clone)]
enum Shape {
    Bracket { element: Type, count: usize },
    Brace,
}

struct LoopContext {
    types: Vec<Type>,
}

struct EffectCall<'r> {
    capability: &'r Capability,
    span: Span,
    depth: usize,
}

#[derive(Default)]
struct Effects<'r> {
    calls: Vec<EffectCall<'r>>,
}

struct PresenceTest {
    inner: Type,
    span: Span,
    shown: String,
    who: &'static str,
}

struct Speculation {
    substitution: usize,
    diagnostics: usize,
    presence_tests: usize,
    late_sort_keys: usize,
    late_json_values: usize,
    record_literals: usize,
    instantiations: u64,
}

impl Speculation {
    fn start(checker: &Checker<'_>) -> Speculation {
        Speculation {
            substitution: checker.snapshot(),
            diagnostics: checker.diagnostics.len(),
            presence_tests: checker.presence_tests.len(),
            late_sort_keys: checker.late_sort_keys.len(),
            late_json_values: checker.late_json_values.len(),
            record_literals: checker.record_literals.len(),
            instantiations: checker.instantiations,
        }
    }

    fn forget(&self, checker: &mut Checker<'_>) {
        checker.diagnostics.truncate(self.diagnostics);
        checker.presence_tests.truncate(self.presence_tests);
        checker.late_sort_keys.truncate(self.late_sort_keys);
        checker.late_json_values.truncate(self.late_json_values);
        checker.record_literals.truncate(self.record_literals);
        checker.instantiations = self.instantiations;
    }

    fn undo(&self, checker: &mut Checker<'_>) {
        checker.rollback(self.substitution);
        self.forget(checker);
    }
}

type ArgumentKey = (usize, usize);

#[derive(Default)]
struct ArgumentTypes {
    observed: HashMap<ArgumentKey, Vec<Type>>,
    implicated: BTreeSet<ArgumentKey>,
}

impl ArgumentTypes {
    fn agreed(&self, key: ArgumentKey) -> Option<&Type> {
        match self.observed.get(&key)?.as_slice() {
            [only] if self.implicated.contains(&key) => Some(only),
            _ => None,
        }
    }
}

struct OpenArgument {
    key: ArgumentKey,
    function: String,
    parameter: String,
}

struct JoinedConcat {
    operands: (ir::Node, ir::Node),
    list: Type,
}

struct SortKeyOwner {
    body: *const Node,
    params: String,
}

#[derive(Default)]
struct Coercion<'s> {
    message: Option<String>,
    detail_hint: Option<String>,
    help: Option<String>,
    joining: bool,
    source: Option<&'s Node>,
    optional_read: Option<(String, String)>,
}

type Checked = (ir::Node, Type);

struct Checker<'r> {
    dialect: Dialect,
    registry: &'r Registry,
    authority: Authority<'r>,
    budgets: Budgets,
    substitution: Vec<Option<Type>>,
    trail: Vec<usize>,
    shapes: HashMap<usize, Shape>,
    presence_tests: Vec<PresenceTest>,
    late_sort_keys: Vec<(Type, Span)>,
    late_json_values: Vec<(Type, Span, &'static str)>,
    record_literals: Vec<(Span, Vec<String>)>,
    unfilled_loops: usize,
    earlier: ArgumentTypes,
    arguments: ArgumentTypes,
    open_arguments: Vec<OpenArgument>,
    diagnostics: Vec<Diagnostic>,
    effects: Vec<Effects<'r>>,
    capabilities_used: BTreeSet<String>,
    functions_used: BTreeSet<String>,
    inputs_used: BTreeSet<String>,
    tail: Option<LoopContext>,
    instantiating: Vec<String>,
    instantiations: u64,
    sort_key_owner: Option<SortKeyOwner>,
    repeat_depth: usize,
    nesting: usize,
    overflowed: std::cell::Cell<bool>,
}

fn variable_index(id: u64) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}

impl<'r> Checker<'r> {
    fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn placeholder(&self, span: Span) -> Checked {
        (literal(Value::Unit, UNIT, span), Type::Never)
    }

    fn fresh(&mut self) -> Type {
        let id = self.substitution.len();
        self.substitution.push(None);
        Type::Variable(id as u64)
    }

    fn snapshot(&self) -> usize {
        self.trail.len()
    }

    fn rollback(&mut self, mark: usize) {
        while self.trail.len() > mark {
            if let Some(id) = self.trail.pop()
                && let Some(slot) = self.substitution.get_mut(id)
            {
                *slot = None;
            }
        }
    }

    fn assign(&mut self, id: u64, t: Type) {
        let index = variable_index(id);
        if let Some(slot) = self.substitution.get_mut(index) {
            *slot = Some(t);
            self.trail.push(index);
        }
    }

    fn bound(&self, id: u64) -> Option<&Type> {
        self.substitution.get(variable_index(id)).and_then(Option::as_ref)
    }

    fn shape(&self, id: u64) -> Option<&Shape> {
        self.shapes.get(&variable_index(id))
    }

    fn resolve(&self, t: &Type) -> Type {
        let mut t = t;
        while let Type::Variable(id) = t {
            match self.bound(*id) {
                Some(next) => t = next,
                None => break,
            }
        }
        t.clone()
    }

    fn is_open(&self, t: &Type) -> bool {
        matches!(self.resolve(t), Type::Variable(id) if self.shape(id).is_none())
    }

    fn zonk(&self, t: &Type) -> Type {
        let t = self.resolve(t);
        match t {
            Type::Variable(id) => match self.shape(id) {
                Some(Shape::Bracket { element, .. }) => list(self.zonk(element)),
                Some(Shape::Brace) => empty_record(),
                None => t,
            },
            Type::List(element) => list(self.zonk(&element)),
            Type::Map(value) => map_of(self.zonk(&value)),
            Type::Tuple(items) => Type::Tuple(items.iter().map(|item| self.zonk(item)).collect()),
            Type::Option(inner) => option(self.zonk(&inner)),
            Type::Result(ok, err) => result_of(self.zonk(&ok), self.zonk(&err)),
            Type::Record(record) => Type::Record(RecordType {
                fields: record.fields.iter().map(|(name, ft)| (name.clone(), self.zonk(ft))).collect(),
                name: record.name.clone(),
                open: record.open,
            }),
            other => other,
        }
    }

    fn occurs(&self, variable: u64, t: &Type) -> bool {
        match self.resolve(t) {
            Type::Variable(other) => {
                other == variable
                    || matches!(self.shape(other), Some(Shape::Bracket { element, .. }) if self.occurs(variable, element))
            }
            Type::List(element) => self.occurs(variable, &element),
            Type::Map(value) => self.occurs(variable, &value),
            Type::Option(inner) => self.occurs(variable, &inner),
            Type::Tuple(items) => items.iter().any(|item| self.occurs(variable, item)),
            Type::Result(ok, err) => self.occurs(variable, &ok) || self.occurs(variable, &err),
            Type::Record(record) => record.fields.iter().any(|(_, ft)| self.occurs(variable, ft)),
            _ => false,
        }
    }

    fn unify(&mut self, a: &Type, b: &Type) -> bool {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b || matches!(a, Type::Never) || matches!(b, Type::Never) {
            return true;
        }
        if let Type::Variable(id) = a {
            return self.bind_variable(id, &b);
        }
        if let Type::Variable(id) = b {
            return self.bind_variable(id, &a);
        }
        match (&a, &b) {
            (Type::List(x), Type::List(y)) | (Type::Map(x), Type::Map(y)) | (Type::Option(x), Type::Option(y)) => {
                self.unify(x, y)
            }
            (Type::Tuple(xs), Type::Tuple(ys)) => {
                xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| self.unify(x, y))
            }
            (Type::Result(ok_a, err_a), Type::Result(ok_b, err_b)) => {
                self.unify(ok_a, ok_b) && self.unify(err_a, err_b)
            }
            (Type::Record(x), Type::Record(y)) => {
                if record_names(x) != record_names(y) {
                    return false;
                }
                x.fields.iter().all(|(name, ft)| {
                    let other = y.field(name).cloned().unwrap_or(Type::Never);
                    self.unify(ft, &other)
                })
            }
            _ => false,
        }
    }

    fn bind_variable(&mut self, variable: u64, t: &Type) -> bool {
        if self.occurs(variable, t) {
            return false;
        }
        let shape = self.shape(variable).cloned();
        let other = self.resolve(t);
        if let Type::Variable(other_id) = other {
            let other_shape = self.shape(other_id).cloned();
            let Some(shape) = shape else {
                self.assign(variable, other);
                return true;
            };
            let Some(other_shape) = other_shape else {
                if self.occurs(other_id, &Type::Variable(variable)) {
                    return false;
                }
                self.assign(other_id, Type::Variable(variable));
                return true;
            };
            return match (shape, other_shape) {
                (Shape::Brace, Shape::Brace) => {
                    self.assign(variable, other);
                    true
                }
                (Shape::Brace, _) | (_, Shape::Brace) => false,
                (
                    Shape::Bracket { element, count },
                    Shape::Bracket { element: other_element, count: other_count },
                ) => {
                    self.assign(variable, other);
                    if count != other_count {
                        self.assign(other_id, list(other_element.clone()));
                    }
                    self.unify(&element, &other_element)
                }
            };
        }
        match shape {
            None => {
                self.assign(variable, t.clone());
                true
            }
            Some(Shape::Brace) => {
                let fits = match &other {
                    Type::Map(_) => true,
                    Type::Record(record) => record.fields.is_empty(),
                    _ => false,
                };
                if fits {
                    self.assign(variable, other);
                }
                fits
            }
            Some(Shape::Bracket { element, count }) => match &other {
                Type::List(item) => {
                    let item = (**item).clone();
                    self.assign(variable, other);
                    self.unify(&element, &item)
                }
                Type::Tuple(items) if items.len() == count => {
                    let items = items.clone();
                    self.assign(variable, other);
                    items.iter().all(|item| self.unify(&element, item))
                }
                _ => false,
            },
        }
    }

    fn trial(&mut self, a: &Type, b: &Type) -> bool {
        let mark = self.snapshot();
        if self.unify(a, b) {
            return true;
        }
        self.rollback(mark);
        false
    }

    fn nullary(enum_type: &EnumType) -> bool {
        enum_type.variants.iter().all(|(_, payload)| payload.is_none())
    }

    fn convert(&mut self, actual: &Type, expected: &Type, joining: bool) -> (bool, Option<Conversion>) {
        let mark = self.snapshot();
        if self.unify(actual, expected) {
            return (true, None);
        }
        self.rollback(mark);
        let (a, e) = (self.resolve(actual), self.resolve(expected));
        if joining && e == STRING && matches!(&a, Type::Enum(en) if Checker::nullary(en)) {
            return (true, Some(Conversion::VariantName));
        }
        match (&a, &e) {
            (_, Type::Option(inner)) if !matches!(a, Type::Option(_) | Type::Never) && !self.is_open(&a) => {
                let (ok, conversion) = self.convert(&a, inner, joining);
                if ok {
                    return (true, Some(Conversion::ToSome(conversion.map(Box::new))));
                }
            }
            (Type::Option(_), _) | (_, Type::Option(_)) => {}
            (Type::Record(ra), Type::Record(re)) if record_names(ra) == record_names(re) => {
                let mut conversions = Vec::new();
                let mut complete = true;
                for (name, ft) in &re.fields {
                    let source = ra.field(name).cloned().unwrap_or(Type::Never);
                    let (ok, conversion) = self.convert(&source, ft, joining);
                    if !ok {
                        complete = false;
                        break;
                    }
                    conversions.push((name.clone(), conversion));
                }
                if complete {
                    return (true, Some(Conversion::EachField(conversions)));
                }
            }
            (Type::Record(ra), Type::Record(re))
                if joining && {
                    let (na, ne) = (record_names(ra), record_names(re));
                    na.len() < ne.len() && na.is_subset(&ne)
                } && re
                    .fields
                    .iter()
                    .filter(|(name, _)| ra.field(name).is_none())
                    .all(|(_, ft)| matches!(self.resolve(ft), Type::Option(_))) =>
            {
                let mut widened = Vec::new();
                let mut complete = true;
                for (name, ft) in &re.fields {
                    let Some(source) = ra.field(name) else {
                        widened.push(WidenedField {
                            name: name.clone(),
                            present: false,
                            conversion: None,
                            field_type: ft.clone(),
                        });
                        continue;
                    };
                    let (ok, conversion) = self.convert(source, ft, joining);
                    if !ok {
                        complete = false;
                        break;
                    }
                    widened.push(WidenedField { name: name.clone(), present: true, conversion, field_type: ft.clone() });
                }
                if complete {
                    return (true, Some(Conversion::WidenRecord(widened)));
                }
            }
            (Type::List(x), Type::List(y)) => {
                if let (true, Some(conversion)) = self.convert(x, y, joining) {
                    return (true, Some(Conversion::EachElement(Box::new(conversion))));
                }
            }
            (Type::Tuple(xs), Type::Tuple(ys)) if xs.len() == ys.len() => {
                let items: Vec<(bool, Option<Conversion>)> =
                    xs.iter().zip(ys).map(|(x, y)| self.convert(x, y, joining)).collect();
                if items.iter().all(|(ok, _)| *ok) {
                    return (true, Some(Conversion::EachItem(items.into_iter().map(|(_, c)| c).collect())));
                }
            }
            _ => {}
        }
        self.rollback(mark);
        (false, None)
    }

    fn coerce(&mut self, value: ir::Node, actual: &Type, expected: Option<&Type>, span: Span) -> Checked {
        self.coerce_with(value, actual, expected, span, Coercion::default())
    }

    fn coerce_joining(&mut self, value: ir::Node, actual: &Type, expected: &Type, span: Span) -> ir::Node {
        self.coerce_with(value, actual, Some(expected), span, Coercion { joining: true, ..Coercion::default() }).0
    }

    fn coerce_with(
        &mut self,
        value: ir::Node,
        actual: &Type,
        expected: Option<&Type>,
        span: Span,
        options: Coercion<'_>,
    ) -> Checked {
        let Some(expected) = expected else {
            return (value, actual.clone());
        };
        let (ok, conversion) = self.convert(actual, expected, options.joining);
        if ok {
            let value = match conversion {
                None => value,
                Some(conversion) => node(Ir::Coerce { value: Box::new(value), conversion }, span),
            };
            return (value, expected.clone());
        }
        if self.dialect == Dialect::JavaScript
            && let Some(promoted) = self.promote_numbers(&value, actual, expected, span)
        {
            return (promoted, expected.clone());
        }
        let (found, wanted) = (self.zonk(actual), self.zonk(expected));
        let mut detail = format!("Expected: {}\nFound: {}", show_type(&wanted), show_type(&found));
        let mut detail_hint = options.detail_hint;
        let mut help = options.help;
        let mut fix = None;
        let mut code = "type-mismatch";
        if detail_hint.is_none()
            && let Type::Option(inner) = &found
            && self.trial(inner, &wanted)
        {
            match options.optional_read.or_else(|| options.source.and_then(|read| self.optional_read_of(read))) {
                Some((hint, optional_fix)) => {
                    detail_hint = Some(hint);
                    fix = Some(optional_fix);
                }
                None => detail_hint = Some(OPTIONAL_HINT.to_string()),
            }
        }
        if found == INT && wanted == FLOAT {
            detail_hint = detail_hint.or_else(|| Some(INT_TO_FLOAT_HINT.to_string()));
            fix = float_fix(options.source);
            help = help.or_else(|| Some("float".to_string()));
        } else if found == FLOAT && wanted == INT {
            if self.dialect == Dialect::JavaScript {
                code = "js-float-as-int";
                fix = Some("Math.floor(x)".to_string());
            } else {
                detail_hint = detail_hint.or_else(|| Some(FLOAT_TO_INT_HINT.to_string()));
                help = help.or_else(|| Some("round".to_string()));
            }
        }
        if wanted == BYTES {
            detail_hint = detail_hint.or_else(|| Some(BYTES_HINT.to_string()));
        }
        if let Some(hint) = detail_hint {
            detail.push('\n');
            detail.push_str(&hint);
        }
        let message = options
            .message
            .unwrap_or_else(|| format!("This is {}, but {} is needed here.", describe(&found), describe(&wanted)));
        self.report(Diagnostic::new(code, span, message).detail(detail).maybe_fix(fix).maybe_help(help));
        (value, expected.clone())
    }

    fn join(&mut self, branches: Vec<(ir::Node, Type, Span)>, expected: Option<&Type>) -> (Vec<ir::Node>, Type) {
        if let Some(expected) = expected {
            let nodes = branches.into_iter().map(|(n, t, span)| self.coerce_joining(n, &t, expected, span)).collect();
            return (nodes, expected.clone());
        }
        let mut target = Type::Never;
        for (_, t, span) in &branches {
            let mark = self.snapshot();
            match self.least_upper_bound(&target, t) {
                Some(joined) => target = joined,
                None => {
                    self.rollback(mark);
                    let message = format!(
                        "The branches return different types: {} and {}.",
                        show_type(&self.zonk(&target)),
                        show_type(&self.zonk(t))
                    );
                    let calls = branches.iter().any(|(node, _, _)| matches!(node.kind, Ir::Call { .. }));
                    let detail = if calls {
                        "Every branch of an if or match must return the same type. To call a tool only when a test \
                         holds, write (when test (call ...)), which is (some result) or none."
                    } else {
                        "Every branch of an if or match must return the same type."
                    };
                    self.report(Diagnostic::new("branch-mismatch", *span, message).detail(detail));
                    return (branches.into_iter().map(|(n, _, _)| n).collect(), target);
                }
            }
        }
        let nodes = branches.into_iter().map(|(n, t, span)| self.coerce_joining(n, &t, &target, span)).collect();
        (nodes, target)
    }

    fn least_upper_bound(&mut self, a: &Type, b: &Type) -> Option<Type> {
        let (ra, rb) = (self.resolve(a), self.resolve(b));
        if matches!(ra, Type::Never) {
            return Some(b.clone());
        }
        if matches!(rb, Type::Never) {
            return Some(a.clone());
        }
        let mark = self.snapshot();
        if self.unify(a, b) {
            return Some(a.clone());
        }
        self.rollback(mark);
        match (&ra, &rb) {
            (Type::Primitive(Primitive::Int), Type::Primitive(Primitive::Float))
            | (Type::Primitive(Primitive::Float), Type::Primitive(Primitive::Int))
                if self.dialect == Dialect::JavaScript =>
            {
                Some(FLOAT)
            }
            (Type::Option(inner), Type::Primitive(Primitive::Bool)) | (Type::Primitive(Primitive::Bool), Type::Option(inner))
                if self.dialect == Dialect::JavaScript
                    && matches!(self.resolve(inner), Type::Primitive(Primitive::Bool) | Type::Never | Type::Variable(_)) =>
            {
                None
            }
            (Type::Option(x), Type::Option(y)) => self.least_upper_bound(x, y).map(option),
            (Type::Option(x), _) if !self.is_open(&rb) => self.least_upper_bound(x, &rb).map(option),
            (_, Type::Option(y)) if !self.is_open(&ra) => self.least_upper_bound(&ra, y).map(option),
            (Type::Record(x), Type::Record(y)) => {
                let mut fields = Vec::new();
                for (name, field_type) in &x.fields {
                    let joined = match y.field(name) {
                        None if self.dialect == Dialect::JavaScript => None,
                        None => self.optional_field(field_type),
                        Some(other) => self.least_upper_bound(field_type, other),
                    }?;
                    fields.push((name.clone(), joined));
                }
                for (name, field_type) in &y.fields {
                    if x.field(name).is_some() {
                        continue;
                    }
                    if self.dialect == Dialect::JavaScript {
                        return None;
                    }
                    fields.push((name.clone(), self.optional_field(field_type)?));
                }
                Some(Type::Record(RecordType::plain(fields)))
            }
            (Type::List(x), Type::List(y)) => self.least_upper_bound(x, y).map(list),
            (Type::Map(x), Type::Map(y)) => self.least_upper_bound(x, y).map(map_of),
            (Type::Tuple(xs), Type::Tuple(ys)) if xs.len() == ys.len() => {
                let mut items = Vec::new();
                for (left, right) in xs.iter().zip(ys) {
                    items.push(self.least_upper_bound(left, right)?);
                }
                Some(Type::Tuple(items))
            }
            (Type::Enum(e), _) if rb == STRING && Checker::nullary(e) => Some(STRING),
            (_, Type::Enum(e)) if ra == STRING && Checker::nullary(e) => Some(STRING),
            _ => None,
        }
    }

    fn optional_field(&self, t: &Type) -> Option<Type> {
        let resolved = self.resolve(t);
        if matches!(resolved, Type::Option(_)) {
            return Some(t.clone());
        }
        if self.is_open(&resolved) { None } else { Some(option(t.clone())) }
    }

    fn is_sort_key(&self, t: &Type) -> bool {
        match self.resolve(t) {
            Type::Primitive(p) => matches!(p, Primitive::Int | Primitive::Float | Primitive::String | Primitive::Bool),
            Type::Tuple(items) => items.iter().all(|item| self.is_sort_key(item)),
            Type::List(element) => self.is_sort_key(&element),
            Type::Nominal(nominal) => nominal.validation.is_none() && self.is_sort_key(&nominal.base),
            Type::Variable(id) => !matches!(self.shape(id), Some(Shape::Brace)),
            Type::Never => true,
            _ => false,
        }
    }

    fn validated_nominal_in(&self, t: &Type) -> Option<Arc<NominalType>> {
        match self.resolve(t) {
            Type::Nominal(nominal) if nominal.validation.is_some() => Some(nominal),
            Type::Tuple(items) => items.iter().find_map(|item| self.validated_nominal_in(item)),
            Type::List(element) => self.validated_nominal_in(&element),
            _ => None,
        }
    }

    fn sort_key_detail(&self, key: &Type) -> String {
        let mut detail = format!("Expected: a sort key\nFound: {}", show_type(&self.zonk(key)));
        if let Some(nominal) = self.validated_nominal_in(key)
            && let Some(validation) = &nominal.validation
        {
            detail.push_str(&format!(
                "\n{} is not a sort key, because its text does not order like its value. Compare two with {}.",
                nominal.name, validation.compare
            ));
        }
        detail
    }

    fn collecting<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (R, Effects<'r>) {
        self.effects.push(Effects::default());
        let result = f(self);
        let effects = self.effects.pop().unwrap_or_default();
        (result, effects)
    }

    fn repeated<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.repeat_depth += 1;
        let result = f(self);
        self.repeat_depth -= 1;
        result
    }

    fn outside_tail<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.tail.take();
        let result = f(self);
        self.tail = saved;
        result
    }
}

impl<'r> Checker<'r> {
    fn check<'a>(&mut self, syntax: &'a Node, expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if self.nesting >= NESTING_LIMIT || crate::stack::exhausted() {
            self.overflowed.set(true);
            return self.placeholder(syntax.span);
        }
        self.nesting += 1;
        let checked = self.check_node(syntax, expected, scope);
        self.nesting -= 1;
        checked
    }

    fn check_node<'a>(&mut self, syntax: &'a Node, expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let span = syntax.span;
        match &syntax.kind {
            NodeKind::Int(value) => {
                let options = Coercion { source: Some(syntax), ..Coercion::default() };
                self.coerce_with(literal(Value::Int(*value), INT, span), &INT, expected, span, options)
            }
            NodeKind::Float(value) => {
                let options = Coercion { source: Some(syntax), ..Coercion::default() };
                self.coerce_with(literal(Value::float(*value), FLOAT, span), &FLOAT, expected, span, options)
            }
            NodeKind::String(value) => match self.expected_enum(expected) {
                Some(enum_type) => self.variant_literal(value, &enum_type, expected, span),
                None => self.coerce(literal(Value::string(value), STRING, span), &STRING, expected, span),
            },
            NodeKind::Bool(value) => self.coerce(literal(Value::Bool(*value), BOOL, span), &BOOL, expected, span),
            NodeKind::Duration(_) => {
                self.report(Diagnostic::new(
                    "bad-literal",
                    span,
                    "A duration only appears in retry and wait-until options, and as the argument of sleep.",
                ));
                self.placeholder(span)
            }
            NodeKind::Keyword(name) => {
                self.report(
                    habit("keyword-value", span, "Keywords only name record fields. Use a string for a label.")
                        .fix(format!("\"{name}\"")),
                );
                self.placeholder(span)
            }
            NodeKind::Symbol(name) => self.symbol(name, span, expected, scope),
            NodeKind::List(items) => self.outside_tail(|c| c.bracket(items, span, expected, scope)),
            NodeKind::Record(_) => self.outside_tail(|c| c.record(syntax, expected, scope, None)),
            NodeKind::Form(items) => self.form(syntax, items, expected, scope),
        }
    }

    fn peel(&self, expected: Option<&Type>) -> Option<Type> {
        let expected = expected?;
        match self.resolve(expected) {
            Type::Option(inner) => Some(*inner),
            _ => Some(expected.clone()),
        }
    }

    fn hint(&self, expected: Option<&Type>) -> Type {
        self.resolve(&self.peel(expected).unwrap_or(Type::Never))
    }

    fn symbol<'a>(&mut self, name: &str, span: Span, expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        match scope.get(name) {
            Some(Binding::Value(t)) => {
                let t = t.clone();
                self.inputs_used.insert(name.to_string());
                return self.coerce(node(Ir::Variable { name: name.to_string() }, span), &t, expected, span);
            }
            Some(Binding::Function(_)) => {
                let message = format!(
                    "{name} is a local function. Call it, (({name} ...)), or pass it to map, filter, and the like."
                )
                .replace("((", "(")
                .replace("))", ")");
                self.report(Diagnostic::new("fn-position", span, message));
                return self.placeholder(span);
            }
            None => {}
        }
        if name == "none" {
            let inner = match self.peel(expected) {
                Some(inner) => inner,
                None => self.fresh(),
            };
            let t = option(inner);
            return self.coerce(literal(Value::None, t.clone(), span), &t, expected, span);
        }
        if name == "unit" {
            return self.coerce(literal(Value::Unit, UNIT, span), &UNIT, expected, span);
        }
        if name == "_" {
            self.report(Diagnostic::new("bad-pattern", span, "_ discards a value in a pattern. It is not a value itself."));
            return self.placeholder(span);
        }
        if first_upper(name) {
            return self.variant(name, span, expected);
        }
        if is_prelude(name) || self.registry.function(name).is_some() {
            self.report(Diagnostic::new(
                "fn-position",
                span,
                format!("{name} is a function. Call it, or pass it to map, filter, and the like."),
            ));
            return self.placeholder(span);
        }
        if self.registry.capability(name).is_some() {
            self.report(Diagnostic::new(
                "fn-position",
                span,
                format!("{name} is a tool. Call it with call: (call {name} {{...}})."),
            ));
            return self.placeholder(span);
        }
        let suggestion = closest(name, &scope.visible(false));
        self.report(Diagnostic::new("unknown-name", span, format!("{name} is not defined here.")).maybe_fix(suggestion));
        self.placeholder(span)
    }

    fn expected_enum(&self, expected: Option<&Type>) -> Option<Arc<EnumType>> {
        expected?;
        match self.hint(expected) {
            Type::Enum(e) => Some(e),
            _ => None,
        }
    }

    fn variant_literal(&mut self, text: &str, enum_type: &Arc<EnumType>, expected: Option<&Type>, span: Span) -> Checked {
        let Some(payload) = enum_type.variant(text) else {
            let variants: Vec<&str> = enum_type.variants.iter().map(|(name, _)| name.as_str()).collect();
            let suggestion = closest(text, &variants);
            self.report(
                Diagnostic::new("unknown-variant", span, format!("\"{text}\" is not a variant of {}.", enum_type.name))
                    .detail(format!("Variants: {}", variants.join(", ")))
                    .maybe_fix(suggestion.map(|s| format!("\"{s}\""))),
            );
            return self.placeholder(span);
        };
        if payload.is_some() {
            self.report(Diagnostic::new(
                "unknown-variant",
                span,
                format!("{text} carries a value, so only a core function constructs it. Match on it with ({text} x)."),
            ));
            return self.placeholder(span);
        }
        let t = Type::Enum(enum_type.clone());
        self.coerce(literal(Value::variant(&enum_type.name, text, None), t.clone(), span), &t, expected, span)
    }

    fn enums(&self) -> Vec<Arc<EnumType>> {
        let mut found = vec![call_error_kind()];
        for (_, t) in self.registry.types() {
            if let Type::Enum(e) = t {
                found.push(e.clone());
            }
        }
        found
    }

    fn variant_spelled_otherwise(&self, name: &str, expected: Option<&Type>) -> Option<(String, String)> {
        let lowered = name.to_lowercase();
        let hinted = match self.hint(expected) {
            Type::Enum(hint) => vec![hint],
            _ => Vec::new(),
        };
        hinted.into_iter().chain(self.enums()).find_map(|e| {
            e.variants
                .iter()
                .find(|(variant, payload)| payload.is_none() && variant.to_lowercase() == lowered && variant != name)
                .map(|(variant, _)| (e.name.clone(), variant.clone()))
        })
    }

    fn variant(&mut self, name: &str, span: Span, expected: Option<&Type>) -> Checked {
        let candidates: Vec<Arc<EnumType>> = match self.hint(expected) {
            Type::Enum(hint) if hint.has(name) => vec![hint],
            _ => self.enums().into_iter().filter(|e| e.has(name)).collect(),
        };
        let enum_type = match candidates.as_slice() {
            [] => {
                let diagnostic =
                    Diagnostic::new("unknown-variant", span, format!("{name} is not a variant of any enum in scope."));
                let diagnostic = match self.variant_spelled_otherwise(name, expected) {
                    Some((enum_name, spelled)) if first_upper(&spelled) => diagnostic
                        .detail(format!("{enum_name} spells it {spelled}."))
                        .fix(spelled),
                    Some((enum_name, spelled)) => diagnostic
                        .detail(format!(
                            "{enum_name} spells it \"{spelled}\", which is not a name, so write it as a string."
                        ))
                        .fix(format!("\"{spelled}\"")),
                    None => diagnostic,
                };
                self.report(diagnostic);
                return self.placeholder(span);
            }
            [one] => one.clone(),
            several => {
                let names: Vec<&str> = several.iter().map(|e| e.name.as_str()).collect();
                self.report(Diagnostic::new(
                    "unresolved-type",
                    span,
                    format!(
                        "{name} is a variant of several enums ({}), and the checker cannot tell which.",
                        names.join(", ")
                    ),
                ));
                return self.placeholder(span);
            }
        };
        if enum_type.variant(name).is_some_and(Option::is_some) {
            self.report(Diagnostic::new(
                "unknown-variant",
                span,
                format!("{name} carries a value, so only a core function constructs it. Match on it with ({name} x)."),
            ));
            return self.placeholder(span);
        }
        let t = Type::Enum(enum_type.clone());
        self.coerce(literal(Value::variant(&enum_type.name, name, None), t.clone(), span), &t, expected, span)
    }

    fn bracket<'a>(&mut self, items: &'a [Node], span: Span, expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let hint = self.hint(expected);
        if let Type::List(element) = &hint {
            let checked: Vec<ir::Node> = items.iter().map(|item| self.check(item, Some(element), scope).0).collect();
            let made = node(Ir::MakeList { items: checked, element: (**element).clone() }, span);
            return self.coerce(made, &hint, expected, span);
        }
        if let Type::Tuple(types) = &hint
            && types.len() == items.len()
        {
            let checked: Vec<ir::Node> =
                items.iter().zip(types).map(|(item, t)| self.check(item, Some(t), scope).0).collect();
            return self.coerce(node(Ir::MakeTuple { items: checked }, span), &hint, expected, span);
        }
        let checked: Vec<Checked> = items.iter().map(|item| self.check(item, None, scope)).collect();
        let Some((_, element)) = checked.first() else {
            let element = self.fresh();
            let made = node(Ir::MakeList { items: Vec::new(), element: element.clone() }, span);
            return self.coerce(made, &list(element), expected, span);
        };
        if self.dialect == Dialect::JavaScript
            && let Some(numbers) = self.js_number_list(&checked, span)
        {
            return self.coerce(numbers, &list(FLOAT), expected, span);
        }
        let element = element.clone();
        let shared = checked.iter().skip(1).all(|(_, t)| self.trial(t, &element));
        let count = checked.len();
        let (nodes, types): (Vec<ir::Node>, Vec<Type>) = checked.into_iter().unzip();
        if shared {
            let shape = self.fresh();
            if let Type::Variable(id) = shape {
                self.shapes.insert(variable_index(id), Shape::Bracket { element, count });
            }
            let made = node(Ir::MakeBracket { items: nodes, shape: shape.clone() }, span);
            return self.coerce(made, &shape, expected, span);
        }
        self.coerce(node(Ir::MakeTuple { items: nodes }, span), &Type::Tuple(types), expected, span)
    }

    fn record<'a>(&mut self, syntax: &'a Node, expected: Option<&Type>, scope: &Scope<'a>, owner: Option<&str>) -> Checked {
        let span = syntax.span;
        let NodeKind::Record(record_fields) = &syntax.kind else {
            return self.placeholder(span);
        };
        let hint = self.hint(expected);
        let place = |owner: Option<&str>| match owner {
            Some(owner) => format!("The input to {owner}"),
            None => "This record".to_string(),
        };
        if let Type::Record(hint_record) = &hint {
            let mut fields: Vec<(String, ir::Node)> = Vec::new();
            let given: BTreeSet<&str> = record_fields.iter().map(|f| f.key.name.as_str()).collect();
            for f in record_fields.iter() {
                let Some(wanted) = hint_record.field(&f.key.name) else {
                    let names: Vec<&str> = hint_record.fields.iter().map(|(n, _)| n.as_str()).collect();
                    let shown: Vec<String> = names.iter().map(|n| format!(":{n}")).collect();
                    let suggestion = closest(&f.key.name, &names);
                    self.report(
                        Diagnostic::new(
                            "unknown-field",
                            f.key.span,
                            format!("{} has no field :{}.", place(owner), f.key.name),
                        )
                        .detail(format!("Fields: {}", shown.join(", ")))
                        .maybe_fix(suggestion.map(|s| format!(":{s}"))),
                    );
                    self.check(&f.value, None, scope);
                    continue;
                };
                let wanted = wanted.clone();
                fields.push((f.key.name.clone(), self.check(&f.value, Some(&wanted), scope).0));
            }
            for (name, field_type) in &hint_record.fields {
                if given.contains(name.as_str()) {
                    continue;
                }
                if matches!(self.resolve(field_type), Type::Option(_)) {
                    fields.push((name.clone(), literal(Value::None, field_type.clone(), span)));
                    continue;
                }
                let detail = format!("Expected: {}", show_type(&self.zonk(&hint)));
                self.report(
                    Diagnostic::new("missing-field", span, format!("{} is missing the field :{name}.", place(owner)))
                        .detail(detail),
                );
            }
            let order: Vec<&str> = hint_record.fields.iter().map(|(n, _)| n.as_str()).collect();
            fields.sort_by_key(|(name, _)| order.iter().position(|n| n == name));
            return self.coerce(node(Ir::MakeRecord { fields }, span), &hint, expected, span);
        }
        if self.dialect == Dialect::JavaScript
            && !record_fields.is_empty()
            && let Type::Map(value_type) = &hint
        {
            let value_type = (**value_type).clone();
            let pair_type = Type::Tuple(vec![STRING, value_type.clone()]);
            let pairs = record_fields
                .iter()
                .map(|f| {
                    let key = literal(Value::string(&f.key.name), STRING, f.key.span);
                    let (value, _) = self.check(&f.value, Some(&value_type), scope);
                    node(Ir::MakeTuple { items: vec![key, value] }, f.key.span)
                })
                .collect();
            let list = node(Ir::MakeList { items: pairs, element: pair_type }, span);
            return self.coerce(prelude(PreludeOp::IntoMap, vec![list], span), &hint, expected, span);
        }
        if record_fields.is_empty() {
            if let Type::Map(_) = &hint {
                return self.coerce(literal(Value::Map(MapValue::from_pairs([])), hint.clone(), span), &hint, expected, span);
            }
            let shape = self.fresh();
            if let Type::Variable(id) = shape {
                self.shapes.insert(variable_index(id), Shape::Brace);
            }
            return self.coerce(node(Ir::MakeEmptyBrace { shape: shape.clone() }, span), &shape, expected, span);
        }
        let mut built = Vec::new();
        let mut types = Vec::new();
        for f in record_fields.iter() {
            let (value, t) = self.check(&f.value, None, scope);
            built.push((f.key.name.clone(), value));
            types.push((f.key.name.clone(), t));
        }
        self.record_literals.push((span, types.iter().map(|(name, _)| name.clone()).collect()));
        let t = Type::Record(RecordType::plain(types));
        let made = node(Ir::MakeRecord { fields: built }, span);
        if let Type::Map(_) = hint {
            let options = Coercion {
                detail_hint: Some("A record literal is never a map. Build one with assoc or into-map.".to_string()),
                help: Some("assoc".to_string()),
                ..Coercion::default()
            };
            return self.coerce_with(made, &t, expected, span, options);
        }
        self.coerce(made, &t, expected, span)
    }

    fn form<'a>(&mut self, syntax: &'a Node, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let span = syntax.span;
        let Some(head) = items.first() else {
            self.report(Diagnostic::new("bad-literal", span, "() is not an expression. Sx has no empty form."));
            return self.placeholder(span);
        };
        if let NodeKind::Keyword(name) = &head.kind {
            let target = items.get(1).map_or_else(|| "r".to_string(), show);
            self.report(
                habit("keyword-call", head.span, "A keyword is not a function. Read a field with get.")
                    .fix(format!("(get {target} :{name})"))
                    .help("get"),
            );
            for item in &items[1..] {
                self.check(item, None, scope);
            }
            return self.placeholder(span);
        }
        let Some(name) = symbol(head) else {
            self.report(Diagnostic::new("bad-literal", head.span, "A form starts with the name of a form or function."));
            return self.placeholder(span);
        };
        if matches!(name, "let" | "if" | "cond" | "match" | "loop") {
            return self.special_form(name, syntax, items, expected, scope);
        }
        if name == "recur" {
            return self.form_recur(span, items, scope);
        }
        if matches!(name, "fail" | "log") && !matches!(scope.get(name), Some(Binding::Function(_))) {
            return self.outside_tail(|c| {
                if name == "fail" { c.form_fail(span, items, expected, scope) } else { c.form_log(span, items, expected, scope) }
            });
        }
        if self.dialect == Dialect::JavaScript
            && matches!(name, "truthy" | "present" | "typeof" | "record-spread")
            && scope.get(name).is_none()
        {
            return self.outside_tail(|c| match name {
                "truthy" => c.form_truthy(span, items, expected, scope),
                "present" => c.form_present(span, items, expected, scope),
                "typeof" => c.form_typeof(span, items, expected, scope),
                _ => c.form_record_spread(span, items, expected, scope),
            });
        }
        if self.dialect == Dialect::JavaScript && name.starts_with("js.") && scope.get(name).is_none() {
            return self.outside_tail(|c| c.js_function(span, name, items, expected, scope));
        }
        if matches!(name, "when" | "do") && !matches!(scope.get(name), Some(Binding::Function(_))) {
            return if name == "when" {
                self.outside_tail(|c| c.form_when(span, items, expected, scope))
            } else {
                self.form_do(span, items, expected, scope)
            };
        }
        self.outside_tail(|c| {
            if is_special_form(name) {
                return c.special_form(name, syntax, items, expected, scope);
            }
            match scope.get(name) {
                Some(Binding::Function(binding)) => {
                    let binding = binding.clone();
                    return c.apply_local(span, items, &binding, expected, scope);
                }
                Some(Binding::Value(t)) => {
                    let found = describe(&c.zonk(t));
                    if is_prelude(name) {
                        c.report(
                            Diagnostic::new(
                                "type-mismatch",
                                head.span,
                                format!("{name} is {found} here, which hides the function {name}."),
                            )
                            .detail(
                                "A name bound by let, loop, fn, or match hides the function of that name in the rest \
                                 of its scope. Rename the local.",
                            )
                            .help(name),
                        );
                    } else {
                        c.report(Diagnostic::new("type-mismatch", head.span, format!("{name} is {found}, not a function.")));
                    }
                    return c.placeholder(span);
                }
                None => {}
            }
            if is_prelude(name) {
                return c.prelude(syntax, items, expected, scope);
            }
            if let Some(function) = c.registry.function(name) {
                return c.core(span, items, function, expected, scope);
            }
            c.unknown_head(span, items, scope)
        })
    }

    fn unknown_head<'a>(&mut self, span: Span, items: &'a [Node], scope: &Scope<'a>) -> Checked {
        let head = &items[0];
        let name = symbol(head).unwrap_or("?");
        let found = habit_head(name);
        let construct = found.map(|(c, _)| c);
        if name == script::SCRIPT {
            self.report(script::position_diagnostic(head.span));
            return self.placeholder(span);
        }
        if self.instantiating.iter().any(|n| n == name) {
            self.report(
                Diagnostic::new(
                    "unknown-function",
                    head.span,
                    format!("{name} cannot call itself. Sx has no recursion."),
                )
                .detail("Repeat with loop and :max, or map over a list that already exists.")
                .help("loop"),
            );
            for item in &items[1..] {
                self.collecting(|c| c.check(item, None, scope));
            }
            return self.placeholder(span);
        }
        for item in &items[1..] {
            if construct == Some("defn") || (construct == Some("assoc") && keyword(item).is_some()) {
                continue;
            }
            self.collecting(|c| c.check(item, None, scope));
        }
        if let Some((construct, message)) = found {
            let diagnostic = habit(construct, head.span, message);
            self.report(if construct == "defn" { diagnostic.help("let") } else { diagnostic });
        } else if self.registry.capability(name).is_some() {
            let rest: Vec<String> = items[1..].iter().map(show).collect();
            self.report(
                Diagnostic::new("unknown-function", head.span, format!("{name} is a tool, so call it with call."))
                    .fix(format!("(call {name} {})", rest.join(" "))),
            );
        } else if let Some(hint) = missing_function(name) {
            self.report(Diagnostic::new("unknown-function", head.span, format!("Sx has no {name}.")).detail(hint));
        } else {
            let mut candidates: Vec<String> = PRELUDE.iter().map(|s| (*s).to_string()).collect();
            candidates.extend(self.registry.functions().map(|f| f.name.clone()));
            candidates.extend(scope.visible(true));
            let suggestion = closest(name, &candidates);
            self.report(
                Diagnostic::new("unknown-function", head.span, format!("{name} is not a function or form Sx knows."))
                    .maybe_fix(suggestion),
            );
        }
        self.placeholder(span)
    }

    fn arity(&mut self, span: Span, items: &[Node], low: usize, high: Option<usize>, signature: &str) -> bool {
        let given = items.len() - 1;
        if given >= low && high.is_none_or(|high| given <= high) {
            return true;
        }
        let name = items.first().and_then(symbol).unwrap_or("?");
        let wanted = match high {
            Some(high) if high == low => low.to_string(),
            None => format!("at least {low}"),
            Some(high) => format!("{low} or {high}"),
        };
        let plural = if high.filter(|h| *h != 0).unwrap_or(low) != 1 { "s" } else { "" };
        self.report(
            Diagnostic::new("arity", span, format!("{name} takes {wanted} argument{plural}, but this call has {given}."))
                .detail(format!("Signature: ({name} {signature})").trim_end().to_string())
                .maybe_help((!UNDOCUMENTED_FORMS.contains(&name)).then(|| name.to_string())),
        );
        false
    }

    fn special_form<'a>(
        &mut self,
        name: &str,
        syntax: &'a Node,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let span = syntax.span;
        match name {
            "let" => self.form_let(span, items, expected, scope),
            "if" => self.form_if(span, items, expected, scope),
            "cond" => self.form_cond(span, items, expected, scope),
            "match" => self.form_match(span, items, expected, scope),
            "loop" => self.form_loop(span, items, expected, scope),
            "fn" => {
                self.report(
                    Diagnostic::new(
                        "fn-position",
                        span,
                        "A fn is only allowed as an argument to map, filter, and the like, or as the value of a let binding.",
                    )
                    .help("fn"),
                );
                self.placeholder(span)
            }
            "get" => self.form_get(span, items, expected, scope),
            "call" => self.form_call(span, items, expected, scope),
            "attempt" => self.form_attempt(span, items, expected, scope),
            "retry" => self.form_retry(span, items, expected, scope),
            "wait-until" => self.form_wait_until(span, items, expected, scope),
            "sleep" => self.form_sleep(span, items, expected),
            "par" => self.form_par(span, items, expected, scope),
            "par-map" => self.form_par_map(span, items, expected, scope),
            "some" => self.form_some(span, items, expected, scope),
            "ok" => self.wrap_result(span, items, expected, scope, WrapKind::Ok),
            "err" => self.wrap_result(span, items, expected, scope, WrapKind::Err),
            _ => self.placeholder(span),
        }
    }

    fn form_let<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let Some(bindings) = items.get(1).and_then(list_items) else {
            self.report(Diagnostic::new("bad-pattern", span, "let needs a binding vector: (let [name value] body).").help("let"));
            return self.placeholder(span);
        };
        if bindings.len() % 2 == 1 {
            self.report(
                Diagnostic::new(
                    "bad-pattern",
                    items[1].span,
                    "A let binding vector pairs each name with a value, but this one has an odd count.",
                )
                .help("let"),
            );
            return self.placeholder(span);
        }
        enum Step {
            Function(String, Span),
            Value(Binder, Box<ir::Node>, Span),
        }
        let mut inner = scope.clone();
        let mut steps = Vec::new();
        for pair in bindings.chunks(2) {
            let (pattern, value) = (&pair[0], &pair[1]);
            if is_function_form(value) {
                let Some(name) = symbol(pattern).filter(|n| *n != "_") else {
                    self.report(Diagnostic::new("bad-pattern", pattern.span, "A local function needs a name."));
                    continue;
                };
                self.reserved_function(name, pattern.span);
                self.check_fn_shape(value);
                let binding = FunctionBinding { form: value, scope: inner.clone(), name: name.to_string() };
                inner = inner.with(name, Binding::Function(Rc::new(binding)));
                steps.push(Step::Function(name.to_string(), pattern.span));
                continue;
            }
            let (value_ir, value_type) = self.outside_tail(|c| c.check(value, None, &inner));
            let (binder, next) = self.bind(pattern, &value_type, &inner, "let", None);
            inner = next;
            steps.push(Step::Value(binder, Box::new(value_ir), pattern.span));
        }
        let body_nodes = &items[2..];
        let Some(last) = body_nodes.last() else {
            self.report(Diagnostic::new("arity", span, "A let needs a body expression after its bindings.").help("let"));
            return self.placeholder(span);
        };
        if body_nodes.len() > 1 {
            self.report(
                habit(
                    "let-body",
                    body_nodes[1].span,
                    format!("A let has one body expression, but this one has {}.", body_nodes.len()),
                )
                .detail("Bind a step that runs for its effect to _ among the bindings: [_ (map ...)].")
                .help("let"),
            );
            for extra in &body_nodes[..body_nodes.len() - 1] {
                self.outside_tail(|c| c.check(extra, None, &inner));
            }
        }
        let (mut body, body_type) = self.check(last, expected, &inner);
        for step in steps.into_iter().rev() {
            body = match step {
                Step::Function(name, step_span) => node(Ir::LocalFunction { name, body: Box::new(body) }, step_span),
                Step::Value(binder, value, step_span) => {
                    node(Ir::Let(ir::Let { binder, value, body: Box::new(body) }), step_span)
                }
            };
        }
        (body, body_type)
    }

    fn fn_shape(form: &Node) -> Option<(&[Node], &Node)> {
        let items = form_items(form)?;
        Some((list_items(items.get(1)?)?, items.get(2)?))
    }

    fn check_fn_shape(&mut self, form: &Node) -> bool {
        let items = form_items(form).unwrap_or(&[]);
        if items.len() < 3 || list_items(&items[1]).is_none() {
            self.report(
                Diagnostic::new("bad-pattern", form.span, "fn needs a parameter vector and a body: (fn [x] body).").help("fn"),
            );
            return false;
        }
        if items.len() > 3 {
            self.report(Diagnostic::new("arity", items[3].span, "A fn has one body expression.").help("fn"));
            return false;
        }
        true
    }

    fn reserved_function(&mut self, name: &str, span: Span) {
        if !is_reserved(name) {
            return;
        }
        let diagnostic = if name == "none" {
            Diagnostic::new("reserved-name", span, "none is the absent value, so it cannot name a function.")
        } else {
            Diagnostic::new("reserved-name", span, format!("{name} is the name of a form, so it cannot name a function."))
                .detail(format!(
                    "({name} ...) always means the form, so a function with this name could never be called. A value may be named {name}; a function may not."
                ))
        };
        self.report(diagnostic.fix(suggested_name(name)).help(name));
    }

    fn bind<'a>(
        &mut self,
        pattern: &Node,
        t: &Type,
        scope: &Scope<'a>,
        role: &str,
        fix: Option<String>,
    ) -> (Binder, Scope<'a>) {
        match &pattern.kind {
            NodeKind::Symbol(name) if name == "_" => (Binder::Discard, scope.clone()),
            NodeKind::Symbol(name) if !first_upper(name) || role != "match" => {
                if first_upper(name)
                    && let Some(owner) = self.enums().into_iter().find(|e| e.has(name))
                {
                    let mut letters = name.chars();
                    let lowered: String = letters.next().into_iter().flat_map(char::to_lowercase).chain(letters).collect();
                    self.report(
                        Diagnostic::new(
                            "reserved-name",
                            pattern.span,
                            format!("{name} is a variant of {}, so it cannot name a value.", owner.name),
                        )
                        .detail("A capitalized name is a variant when an enum in scope has a variant of that name. Name a value with a lowercase name.")
                        .fix(lowered),
                    );
                    return (Binder::Discard, scope.value(name, Type::Never));
                }
                if name == "none" {
                    self.report(
                        Diagnostic::new("reserved-name", pattern.span, "none is the absent value, so it cannot name a value.")
                            .detail("In a pattern, none matches an absent value, and as an expression it is that value.")
                            .fix(suggested_name(name))
                            .help(name),
                    );
                    return (Binder::Discard, scope.clone());
                }
                (Binder::Name(name.clone()), scope.value(name, t.clone()))
            }
            NodeKind::List(items) => {
                let mut resolved = self.resolve(t);
                if let Type::Variable(_) = resolved {
                    let candidate = Type::Tuple(items.iter().map(|_| self.fresh()).collect());
                    if self.unify(t, &candidate) {
                        resolved = candidate;
                    }
                }
                if let Type::Never = resolved {
                    resolved = Type::Tuple(items.iter().map(|_| Type::Never).collect());
                }
                let mut inner = scope.clone();
                if let Type::Record(_) = resolved {
                    let in_binding = role == "let" || role == "loop";
                    let message = if in_binding {
                        "This pattern takes a tuple apart, but the value is a record.".to_string()
                    } else {
                        format!("This parameter takes a tuple apart, but {role} passes it a record.")
                    };
                    let detail = format!(
                        "Expected: {}\nFound: a tuple pattern {}",
                        show_type(&self.zonk(&resolved)),
                        show(pattern)
                    );
                    self.report(
                        Diagnostic::new("type-mismatch", pattern.span, message)
                            .detail(detail)
                            .maybe_fix(fix)
                            .maybe_help((!in_binding).then(|| role.to_string())),
                    );
                    for item in items.iter() {
                        inner = self.bind(item, &Type::Never, &inner, role, None).1;
                    }
                    return (Binder::Discard, inner);
                }
                let types = match &resolved {
                    Type::Tuple(types) if types.len() == items.len() => types.clone(),
                    _ => {
                        let zonked = self.zonk(&resolved);
                        self.report(
                            Diagnostic::new(
                                "type-mismatch",
                                pattern.span,
                                format!(
                                    "This pattern takes apart a tuple of {}, but the value is {}.",
                                    items.len(),
                                    describe(&zonked)
                                ),
                            )
                            .detail(format!("Found: {}", show_type(&zonked))),
                        );
                        for item in items.iter() {
                            inner = self.bind(item, &Type::Never, &inner, role, None).1;
                        }
                        return (Binder::Discard, inner);
                    }
                };
                let mut binders = Vec::new();
                for (item, item_type) in items.iter().zip(&types) {
                    let (binder, next) = self.bind(item, item_type, &inner, role, None);
                    binders.push(binder);
                    inner = next;
                }
                (Binder::Tuple(binders), inner)
            }
            _ => {
                let constructor = head_name(pattern);
                let detail = constructor.filter(|c| matches!(*c, "some" | "ok" | "err") || first_upper(c)).map(|_| {
                    format!("To take a value apart, use match: (match value {} ... _ ...).", show(pattern))
                });
                self.report(
                    Diagnostic::new(
                        "bad-pattern",
                        pattern.span,
                        format!("{} is not a pattern. A pattern is a name, _, or [a b ...].", show(pattern)),
                    )
                    .maybe_detail(detail),
                );
                let mut inner = scope.clone();
                for name in pattern_names(pattern) {
                    inner = inner.value(&name, Type::Never);
                }
                (Binder::Discard, inner)
            }
        }
    }

    fn check_test<'a>(&mut self, condition_node: &'a Node, scope: &Scope<'a>, who: &'static str) -> Test {
        if let Some(operand) = self.present_operand(condition_node, scope) {
            return self.check_present_test(operand, condition_node, scope, who);
        }
        let (condition, condition_type) = self.outside_tail(|c| c.check(condition_node, None, scope));
        let resolved = self.resolve(&condition_type);
        if let Type::Option(inner_type) = &resolved {
            let inner = self.resolve(inner_type);
            if inner == BOOL && self.dialect == Dialect::Sx {
                let message = format!(
                    "{} must be true or false, but this is {}.",
                    test_subject(who),
                    describe(&self.zonk(&condition_type))
                );
                self.report(
                    Diagnostic::new("condition-not-bool", condition_node.span, message)
                        .detail(format!(
                            "Presence and truth would disagree: (some false) is present but false.\n\
                             Match on it instead: (match {} (some v) ... none ...).",
                            show(condition_node)
                        ))
                        .help(test_help(who)),
                );
                return Test::plain(condition);
            }
            if !matches!(inner, Type::Never) {
                return self.presence_test(condition, inner_type, condition_node, scope, who);
            }
        }
        if !self.trial(&condition_type, &BOOL) {
            let hint = matches!(resolved, Type::Option(_))
                .then(|| "To test an optional value, match on it: (match x (some v) ... none ...).".to_string());
            let message = format!(
                "{} must be true or false, but this is {}.",
                test_subject(who),
                describe(&self.zonk(&condition_type))
            );
            self.report(
                Diagnostic::new("condition-not-bool", condition_node.span, message).maybe_detail(hint).help(test_help(who)),
            );
        }
        self.with_narrowings(Test::plain(condition), condition_node, scope)
    }

    fn presence_test(&mut self, condition: ir::Node, inner_type: &Type, operand: &Node, scope: &Scope<'_>, who: &'static str) -> Test {
        if self.is_open(&self.resolve(inner_type)) {
            self.presence_tests.push(PresenceTest {
                inner: inner_type.clone(),
                span: operand.span,
                shown: show(operand),
                who,
            });
        }
        let name = symbol(operand).filter(|n| scope.is_value(n)).map(str::to_string);
        Test {
            condition,
            presence: true,
            name,
            inner: Some(inner_type.clone()),
            narrow_true: Vec::new(),
            narrow_false: Vec::new(),
        }
    }

    fn check_presence_tests(&mut self) {
        let tests = std::mem::take(&mut self.presence_tests);
        for test in &tests {
            if self.resolve(&test.inner) == BOOL && self.dialect == Dialect::Sx {
                self.report(
                    Diagnostic::new(
                        "condition-not-bool",
                        test.span,
                        format!("{} must be true or false, but this is an Option<Bool>.", test_subject(test.who)),
                    )
                    .detail(format!(
                        "Presence and truth would disagree: (some false) is present but false.\n\
                         Match on it instead: (match {} (some v) ... none ...).",
                        test.shown
                    ))
                    .help(test_help(test.who)),
                );
            }
        }
    }

    fn check_late_json_values(&mut self) {
        let values = std::mem::take(&mut self.late_json_values);
        for (t, span, who) in &values {
            let t = self.zonk(t);
            if self.dialect == Dialect::JavaScript && *who == "js.stringify" && holds_map(&t) {
                self.report(
                    Diagnostic::new("ts-map-order", *span, "The order of a Map's keys is not preserved")
                        .detail("JavaScript writes the keys of an object in insertion order, and a Map here is in ascending key order.")
                        .fix("Object.keys(m).sort()"),
                );
                continue;
            }
            if let Some(problem) = encodable_problem(&t) {
                let reason = problem.split_once(" has no JSON encoding").map_or("", |(_, reason)| reason);
                self.report(
                    Diagnostic::new(
                        "no-encoding",
                        *span,
                        format!("{who} cannot write {}, which has no JSON encoding{reason}.", describe(&t)),
                    )
                    .help(*who),
                );
            }
        }
    }

    fn check_late_sort_keys(&mut self) {
        let keys = std::mem::take(&mut self.late_sort_keys);
        for (key, span) in &keys {
            if !self.is_sort_key(key) {
                self.report(
                    Diagnostic::new(
                        "type-mismatch",
                        *span,
                        "sort-by needs a key that is an Int, Float, String, Bool, or a tuple of them.",
                    )
                    .detail(self.sort_key_detail(key))
                    .help("sort-by"),
                );
            }
        }
    }

    fn narrowed<'a>(test: &Test, scope: &Scope<'a>) -> Scope<'a> {
        let narrowed = match (&test.name, &test.inner) {
            (Some(name), Some(inner)) => scope.value(name, inner.clone()),
            _ => scope.clone(),
        };
        narrowed.narrow(&test.narrow_true)
    }

    fn narrowed_else<'a>(test: &Test, scope: &Scope<'a>) -> Scope<'a> {
        scope.narrow(&test.narrow_false)
    }

    fn lower_test(test: Test, then: ir::Node, otherwise: ir::Node, span: Span, result: &Type) -> ir::Node {
        let then = js::guard_present(&test.narrow_true, then, result, span);
        let otherwise = js::guard_present(&test.narrow_false, otherwise, result, span);
        if !test.presence {
            return node(
                Ir::If(ir::If {
                    condition: Box::new(test.condition),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                }),
                span,
            );
        }
        let binder = test.name.map_or(Binder::Discard, Binder::Name);
        node(
            Ir::Match(ir::Match {
                scrutinee: Box::new(test.condition),
                clauses: vec![
                    (MatchPattern::Some(Box::new(MatchPattern::Any(binder))), then),
                    (MatchPattern::None, otherwise),
                ],
            }),
            span,
        )
    }

    fn form_if<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 3, Some(3), "condition then else") {
            return self.placeholder(span);
        }
        let (condition_node, then_node, else_node) = (&items[1], &items[2], &items[3]);
        let test = self.check_test(condition_node, scope, "if");
        let (then, then_type) = self.check(then_node, expected, &Checker::narrowed(&test, scope));
        let (otherwise, else_type) = self.check(else_node, expected, &Checker::narrowed_else(&test, scope));
        let (nodes, t) = self.join(vec![(then, then_type, then_node.span), (otherwise, else_type, else_node.span)], expected);
        let mut nodes = nodes.into_iter();
        let (Some(then), Some(otherwise)) = (nodes.next(), nodes.next()) else {
            return self.placeholder(span);
        };
        let result = self.zonk(&t);
        (Checker::lower_test(test, then, otherwise, span, &result), t)
    }

    fn form_when<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if items.len() > 3 {
            self.report(
                habit("when-body", items[3].span, format!("A when has one expression, but this one has {}.", items.len() - 2))
                    .detail("Run several steps in order with do: (when test (do step1 step2)).")
                    .fix(format!("(when {} (do {}))", show(&items[1]), items[2..].iter().map(show).collect::<Vec<_>>().join(" "))),
            );
            self.check_test(&items[1], scope, "when");
            for item in &items[2..] {
                self.check(item, None, scope);
            }
            return self.placeholder(span);
        }
        if !self.arity(span, items, 2, Some(2), "condition expression") {
            return self.placeholder(span);
        }
        let test = self.check_test(&items[1], scope, "when");
        let inner_expected = match expected.map(|e| self.resolve(e)) {
            Some(Type::Option(inner)) => Some(*inner),
            _ => None,
        };
        let (value, t) = self.check(&items[2], inner_expected.as_ref(), &Checker::narrowed(&test, scope));
        let wrapped = option(t);
        let some = node(Ir::Wrap { kind: WrapKind::Some, value: Box::new(value), wrapped_type: wrapped.clone() }, span);
        let none = literal(Value::None, wrapped.clone(), span);
        let made = Checker::lower_test(test, some, none, span, &self.zonk(&wrapped));
        let options = Coercion { detail_hint: Some(WHEN_HINT.to_string()), ..Coercion::default() };
        self.coerce_with(made, &wrapped, expected, span, options)
    }

    fn form_do<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 1, None, "expression ...") {
            return self.placeholder(span);
        }
        let Some((last, steps)) = items[1..].split_last() else {
            return self.placeholder(span);
        };
        let values: Vec<(ir::Node, Span)> =
            steps.iter().map(|step| (self.outside_tail(|c| c.check(step, None, scope)).0, step.span)).collect();
        let (mut body, t) = self.check(last, expected, scope);
        for (value, step_span) in values.into_iter().rev() {
            let step = ir::Let { binder: Binder::Discard, value: Box::new(value), body: Box::new(body) };
            body = node(Ir::Let(step), step_span);
        }
        (body, t)
    }

    fn form_cond<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let args = &items[1..];
        let ends_with_else = args.len() >= 2 && args.len().is_multiple_of(2) && is_else(&args[args.len() - 2]);
        if !ends_with_else {
            if let Some(last) = args.last().filter(|last| is_else(last)) {
                self.report(
                    Diagnostic::new("arity", last.span, ":else needs a value after it.")
                        .detail("End a cond with :else and a value: (cond test result ... :else value).")
                        .help("cond"),
                );
            } else {
                self.report(
                    Diagnostic::new("missing-else", span, "A cond must end with :else and a value for when no test is true.")
                        .detail("End it like this: (cond test result ... :else value).")
                        .help("cond"),
                );
            }
            self.outside_tail(|c| {
                for item in args {
                    if keyword(item).is_none() {
                        c.check(item, None, scope);
                    }
                }
            });
            return self.placeholder(span);
        }
        let mut tests = Vec::new();
        let mut branches = Vec::new();
        for pair in args[..args.len() - 2].chunks(2) {
            let (test_node, result_node) = (&pair[0], &pair[1]);
            if is_else(test_node) {
                self.report(
                    Diagnostic::new(
                        "unreachable-clause",
                        test_node.span,
                        "This :else comes before the last clause, so the clauses after it can never run.",
                    )
                    .help("cond"),
                );
                self.outside_tail(|c| c.check(result_node, None, scope));
                continue;
            }
            let test = self.check_test(test_node, scope, "cond");
            let (result, result_type) = self.check(result_node, expected, &Checker::narrowed(&test, scope));
            tests.push(test);
            branches.push((result, result_type, result_node.span));
        }
        let last = &args[args.len() - 1];
        let (final_node, final_type) = self.check(last, expected, scope);
        branches.push((final_node, final_type, last.span));
        let (mut nodes, t) = self.join(branches, expected);
        let Some(mut body) = nodes.pop() else {
            return self.placeholder(span);
        };
        let result = self.zonk(&t);
        for (test, branch) in tests.into_iter().rev().zip(nodes.into_iter().rev()) {
            body = Checker::lower_test(test, branch, body, span, &result);
        }
        (body, t)
    }

    fn form_match<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if items.len() < 4 {
            self.report(Diagnostic::new("arity", span, "match takes a value and at least one pattern and result.").help("match"));
            return self.placeholder(span);
        }
        let subject = &items[1];
        let (scrutinee, scrutinee_type) = self.outside_tail(|c| c.check(subject, None, scope));
        let clause_nodes = &items[2..];
        let optional_patterns = matches!(head_name(&clause_nodes[0]), Some("some")) || symbol(&clause_nodes[0]) == Some("none");
        let (scrutinee, scrutinee_type) = if self.dialect == Dialect::JavaScript && optional_patterns {
            self.present_when_not_optional(scrutinee, scrutinee_type, subject.span)
        } else {
            (scrutinee, scrutinee_type)
        };
        if clause_nodes.len() % 2 == 1 {
            self.report(
                Diagnostic::new(
                    "bad-pattern",
                    clause_nodes[clause_nodes.len() - 1].span,
                    "This match pairs each pattern with a result, but the last pattern has none.",
                )
                .help("match"),
            );
            return self.placeholder(span);
        }
        let mut covered: BTreeSet<String> = BTreeSet::new();
        let mut complete = false;
        let mut clauses: Vec<(MatchPattern, ir::Node, Type, Span)> = Vec::new();
        let scrutinee_option = self.resolve(&scrutinee_type);
        let narrowing = match (&scrutinee_option, symbol(subject)) {
            (Type::Option(inner), Some(name)) if scope.is_value(name) => Some((name, (**inner).clone())),
            _ => None,
        };
        let count = clause_nodes.len() / 2;
        for (index, pair) in clause_nodes.chunks(2).enumerate() {
            let (pattern_node, body_node) = (&pair[0], &pair[1]);
            let (mut pattern, mut inner, keys, total) = self.match_pattern(pattern_node, &scrutinee_type, scope);
            let last = index == count - 1;
            let wildcard = matches!(pattern, MatchPattern::Any(_));
            if last && wildcard && !complete && !covered.is_empty() && self.missing_cases(&scrutinee_type, &covered).is_empty() {
                self.check(body_node, None, &inner);
                complete = true;
                break;
            }
            if let Some((name, inner_type)) = &narrowing
                && last
                && covered.contains("none")
                && symbol(pattern_node) == Some("_")
            {
                pattern = MatchPattern::Some(Box::new(MatchPattern::Any(Binder::Name((*name).to_string()))));
                inner = scope.value(name, inner_type.clone());
            }
            if complete && !(last && wildcard) {
                self.report(Diagnostic::new(
                    "unreachable-clause",
                    pattern_node.span,
                    "This clause can never match, because the clauses before it cover every case.",
                ));
            } else if !keys.is_empty() && keys.is_subset(&covered) && !wildcard && total {
                self.report(Diagnostic::new(
                    "unreachable-clause",
                    pattern_node.span,
                    "This clause can never match, because an earlier clause covers the same case.",
                ));
            }
            if total {
                covered.extend(keys);
            }
            if wildcard {
                complete = true;
            }
            let (body, body_type) = self.check(body_node, expected, &inner);
            clauses.push((pattern, body, body_type, body_node.span));
        }
        if !complete {
            let missing = self.missing_cases(&scrutinee_type, &covered);
            if !missing.is_empty() {
                self.report(
                    Diagnostic::new("non-exhaustive", span, format!("This match does not handle {}.", missing.join(", ")))
                        .detail("Add a clause for each missing case, or end with _ to match anything else.")
                        .help("match"),
                );
            }
        }
        let mut patterns = Vec::new();
        let mut branches = Vec::new();
        for (pattern, body, body_type, body_span) in clauses {
            patterns.push(pattern);
            branches.push((body, body_type, body_span));
        }
        let (bodies, t) = self.join(branches, expected);
        let clauses = patterns.into_iter().zip(bodies).collect();
        (node(Ir::Match(ir::Match { scrutinee: Box::new(scrutinee), clauses }), span), t)
    }

    fn missing_cases(&self, t: &Type, covered: &BTreeSet<String>) -> Vec<String> {
        let pick = |cases: [(&str, &str); 2]| {
            cases.iter().filter(|(key, _)| !covered.contains(*key)).map(|(_, case)| (*case).to_string()).collect()
        };
        match self.resolve(t) {
            Type::Option(_) => pick([("some", "(some x)"), ("none", "none")]),
            Type::Result(_, _) => pick([("ok", "(ok x)"), ("err", "(err e)")]),
            Type::Enum(e) => e
                .variants
                .iter()
                .filter(|(name, _)| !covered.contains(name))
                .map(|(name, payload)| if payload.is_none() { name.clone() } else { format!("({name} x)") })
                .collect(),
            Type::Never => Vec::new(),
            resolved if resolved == BOOL => ["true", "false"]
                .iter()
                .filter(|name| !covered.contains(&format!("Bool:{name}")))
                .map(|name| (*name).to_string())
                .collect(),
            _ => vec!["every other value; end with _".to_string()],
        }
    }
}

type PatternResult<'a> = (MatchPattern, Scope<'a>, BTreeSet<String>, bool);

fn keys(key: &str) -> BTreeSet<String> {
    BTreeSet::from([key.to_string()])
}

impl<'r> Checker<'r> {
    fn match_pattern<'a>(&mut self, pattern: &Node, t: &Type, scope: &Scope<'a>) -> PatternResult<'a> {
        let mut resolved = self.resolve(t);
        let span = pattern.span;
        match &pattern.kind {
            NodeKind::Symbol(name) if name == "_" => {
                return (MatchPattern::Any(Binder::Discard), scope.clone(), BTreeSet::new(), true);
            }
            NodeKind::Float(_) => {
                self.report(
                    Diagnostic::new(
                        "bad-pattern",
                        span,
                        format!("{} cannot be a match pattern, because Floats are rarely equal.", show(pattern)),
                    )
                    .detail("Compare with (= x 1.5), (< x 1.5), or (>= x 1.5) in an if or cond.")
                    .help("match"),
                );
                return (MatchPattern::Any(Binder::Discard), scope.clone(), BTreeSet::new(), false);
            }
            NodeKind::Int(_) | NodeKind::String(_) | NodeKind::Bool(_) => {
                return self.literal_pattern(pattern, t, scope);
            }
            NodeKind::Symbol(name) if name == "none" => {
                if self.is_open(&resolved) {
                    resolved = option(self.fresh());
                    self.unify(t, &resolved);
                }
                if !matches!(resolved, Type::Option(_) | Type::Never) {
                    return self.pattern_mismatch(pattern, t, scope);
                }
                return (MatchPattern::None, scope.clone(), keys("none"), true);
            }
            NodeKind::Symbol(name) if first_upper(name) => {
                let variant = MatchPattern::Variant { variant: name.clone(), inner: None };
                let enum_type = match &resolved {
                    Type::Never => return (variant, scope.clone(), keys(name), true),
                    Type::Enum(e) => e.clone(),
                    _ => return self.pattern_mismatch(pattern, t, scope),
                };
                let Some(payload) = enum_type.variant(name) else {
                    let names: Vec<&str> = enum_type.variants.iter().map(|(v, _)| v.as_str()).collect();
                    self.report(
                        Diagnostic::new("unknown-variant", span, format!("{} has no variant {name}.", enum_type.name))
                            .detail(format!("Variants: {}", names.join(", "))),
                    );
                    return (variant, scope.clone(), BTreeSet::new(), false);
                };
                if payload.is_some() {
                    self.report(
                        Diagnostic::new("bad-pattern", span, format!("{name} carries a value, so match it as ({name} x)."))
                            .help("match"),
                    );
                }
                return (variant, scope.clone(), keys(name), true);
            }
            NodeKind::Symbol(name) if is_plain_name(name) => {
                return (MatchPattern::Any(Binder::Name(name.clone())), scope.value(name, t.clone()), BTreeSet::new(), true);
            }
            NodeKind::Symbol(name) => {
                self.report(
                    Diagnostic::new(
                        "bad-pattern",
                        span,
                        format!("{name} is not a match pattern. Use a name or _ to match anything."),
                    )
                    .help("match"),
                );
                return (MatchPattern::Any(Binder::Discard), scope.value(name, Type::Never), BTreeSet::new(), false);
            }
            NodeKind::Form(items) if items.len() == 2 && symbol(&items[0]).is_some() => {
                let constructor = symbol(&items[0]).unwrap_or_default();
                let inner_node = &items[1];
                if matches!(constructor, "some" | "ok" | "err") {
                    if self.is_open(&resolved) {
                        resolved = if constructor == "some" {
                            option(self.fresh())
                        } else {
                            let ok = self.fresh();
                            let err = self.fresh();
                            result_of(ok, err)
                        };
                        self.unify(t, &resolved);
                    }
                    let inner_type = match (&resolved, constructor) {
                        (Type::Never, _) => Type::Never,
                        (Type::Option(inner), "some") => (**inner).clone(),
                        (Type::Result(ok, _), "ok") => (**ok).clone(),
                        (Type::Result(_, err), "err") => (**err).clone(),
                        _ => return self.pattern_mismatch(pattern, t, scope),
                    };
                    let (inner_pattern, inner_scope, total) = self.sub_pattern(inner_node, &inner_type, scope);
                    let inner_pattern = Box::new(inner_pattern);
                    let wrapped = match constructor {
                        "some" => MatchPattern::Some(inner_pattern),
                        "ok" => MatchPattern::Ok(inner_pattern),
                        _ => MatchPattern::Err(inner_pattern),
                    };
                    return (wrapped, inner_scope, keys(constructor), total);
                }
                if first_upper(constructor) {
                    if let Type::Enum(enum_type) = &resolved {
                        if let Some(payload) = enum_type.variant(constructor) {
                            let Some(payload) = payload.clone() else {
                                self.report(
                                    Diagnostic::new(
                                        "bad-pattern",
                                        span,
                                        format!("{constructor} carries no value, so match it as {constructor}."),
                                    )
                                    .help("match"),
                                );
                                let variant = MatchPattern::Variant { variant: constructor.to_string(), inner: None };
                                return (variant, scope.clone(), keys(constructor), true);
                            };
                            let (inner_pattern, inner_scope, total) = self.sub_pattern(inner_node, &payload, scope);
                            let variant = MatchPattern::Variant {
                                variant: constructor.to_string(),
                                inner: Some(Box::new(inner_pattern)),
                            };
                            return (variant, inner_scope, keys(constructor), total);
                        }
                        let names: Vec<&str> = enum_type.variants.iter().map(|(v, _)| v.as_str()).collect();
                        self.report(
                            Diagnostic::new(
                                "unknown-variant",
                                items[0].span,
                                format!("{} has no variant {constructor}.", enum_type.name),
                            )
                            .detail(format!("Variants: {}", names.join(", "))),
                        );
                        let variant = MatchPattern::Variant { variant: constructor.to_string(), inner: None };
                        return (variant, scope.clone(), BTreeSet::new(), false);
                    }
                    return self.pattern_mismatch(pattern, t, scope);
                }
            }
            _ => {}
        }
        self.report(
            Diagnostic::new("bad-pattern", span, format!("{} is not a match pattern.", show(pattern)))
                .detail("Patterns: (some x) none (ok x) (err e) Variant (Variant x) 1 \"text\" true _")
                .help("match"),
        );
        (MatchPattern::Any(Binder::Discard), scope.clone(), BTreeSet::new(), false)
    }

    fn literal_pattern<'a>(&mut self, pattern: &Node, t: &Type, scope: &Scope<'a>) -> PatternResult<'a> {
        let (value, literal_type) = match &pattern.kind {
            NodeKind::Int(n) => (Value::Int(*n), INT),
            NodeKind::String(s) => (Value::string(s), STRING),
            NodeKind::Bool(b) => (Value::Bool(*b), BOOL),
            _ => return self.pattern_mismatch(pattern, t, scope),
        };
        let name = match &literal_type {
            Type::Primitive(p) => p.name(),
            _ => "?",
        };
        let key = format!("{name}:{}", show_value(&value));
        if matches!(self.resolve(t), Type::Never) || self.trial(t, &literal_type) {
            return (MatchPattern::Literal(value), scope.clone(), keys(&key), true);
        }
        let fix = match (&pattern.kind, self.resolve(t)) {
            (NodeKind::String(text), Type::Enum(e)) if e.has(text) => Some(text.clone()),
            _ => None,
        };
        let zonked = self.zonk(t);
        self.report(
            Diagnostic::new("bad-pattern", pattern.span, format!("{} cannot match {}.", show(pattern), describe(&zonked)))
                .detail(format!("The value is {}.", show_type(&zonked)))
                .maybe_fix(fix)
                .help("match"),
        );
        (MatchPattern::Any(Binder::Discard), scope.clone(), BTreeSet::new(), false)
    }

    fn sub_pattern<'a>(&mut self, pattern: &Node, t: &Type, scope: &Scope<'a>) -> (MatchPattern, Scope<'a>, bool) {
        let binds = match &pattern.kind {
            NodeKind::Symbol(name) => (is_plain_name(name) && name != "none") || name == "_",
            NodeKind::List(_) => true,
            _ => false,
        };
        if binds {
            let (binder, inner) = self.bind(pattern, t, scope, "match", None);
            return (MatchPattern::Any(binder), inner, true);
        }
        let (matched, inner, _, _) = self.match_pattern(pattern, t, scope);
        (matched, inner, false)
    }

    fn pattern_mismatch<'a>(&mut self, pattern: &Node, t: &Type, scope: &Scope<'a>) -> PatternResult<'a> {
        let zonked = self.zonk(t);
        self.report(
            Diagnostic::new("bad-pattern", pattern.span, format!("{} cannot match {}.", show(pattern), describe(&zonked)))
                .detail(format!("The value is {}.", show_type(&zonked)))
                .help("match"),
        );
        (MatchPattern::Any(Binder::Discard), scope.clone(), BTreeSet::new(), false)
    }

    fn form_get<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 2, Some(3), "record :field [default]") {
            return self.placeholder(span);
        }
        let (record_node, key) = (&items[1], &items[2]);
        let default_node = items.get(3);
        if self.dialect == Dialect::JavaScript
            && default_node.is_none()
            && let Some(narrowed) = self.narrowed_read(record_node, key, scope)
            && let Some(path) = js::path_of(record_node, keyword(key).unwrap_or_default())
        {
            let read = node(Ir::Variable { name: path }, span);
            return self.coerce(read, &narrowed, expected, span);
        }
        let (record_ir, record_type) = self.check(record_node, None, scope);
        if self.dialect == Dialect::JavaScript
            && let Some(shaped) = self.shaped_enum(&record_type)
            && let Some(field) = keyword(key)
            && default_node.is_none()
        {
            let (read, t) = self.shaped_get(record_ir, &shaped, field, span, key.span);
            return self.coerce(read, &t, expected, span);
        }
        let mut resolved = self.resolve(&record_type);
        if self.dialect == Dialect::JavaScript
            && let Type::Enum(found) = &resolved
            && !found.name.starts_with(crate::values::SHAPED_PREFIX)
            && !Checker::nullary(found)
            && default_node.is_none()
            && let Some(field) = keyword(key).filter(|field| matches!(*field, "tag" | "value"))
        {
            let found = found.clone();
            let narrowed = symbol(record_node).and_then(|name| scope.variants_of(name)).map(<[String]>::to_vec);
            if let Some(read) = self.enum_read(record_ir.clone(), &found, field, narrowed.as_deref(), span, expected) {
                return read;
            }
            let example = found.variants.iter().find(|(_, payload)| payload.is_some()).map_or("Name", |(name, _)| name.as_str());
            self.report(Diagnostic::new(
                "not-a-record",
                record_node.span,
                format!(
                    "This is {}, which has several variants, so .{field} can be read only after a test such as x.tag === \"{example}\" has picked one.",
                    describe(&self.zonk(&resolved))
                ),
            ));
            return self.placeholder(span);
        }
        if self.dialect == Dialect::JavaScript
            && matches!(resolved, Type::Result(..))
            && default_node.is_none()
            && let Some(read) = keyword(key).and_then(|field| match field {
                "status" => Some("js.settled-status"),
                "value" => Some("js.settled-value"),
                "reason" => Some("js.settled-reason"),
                _ => None,
            })
        {
            return self.settled_read(read, span, (record_ir, record_type), record_node.span, expected);
        }
        let Some(key_name) = keyword(key) else {
            if let Some(value_type) = self.map_value_of(&record_type) {
                return self.map_get(span, record_ir, &value_type, key, default_node, expected, scope);
            }
            let fix = match &key.kind {
                NodeKind::String(text) => Some(format!("(get {} :{text})", show(record_node))),
                _ => None,
            };
            self.report(
                Diagnostic::new("bad-literal", key.span, "get takes a field name as a keyword, such as :name.")
                    .maybe_fix(fix)
                    .help("get"),
            );
            return self.placeholder(span);
        };
        if self.is_brace(&resolved) {
            self.trial(&resolved, &empty_record());
            resolved = self.resolve(&resolved);
        }
        let record = match &resolved {
            Type::Map(value_type) if self.dialect == Dialect::JavaScript => {
                let value_type = (**value_type).clone();
                let named = Node { kind: NodeKind::String(key_name.to_string()), span: key.span };
                return self.map_get(span, record_ir, &value_type, &named, default_node, expected, scope);
            }
            Type::Map(_) => {
                self.report(
                    habit("keyword-value", key.span, "A Map is keyed by strings. Keywords only name record fields.")
                        .fix(format!("\"{key_name}\""))
                        .help("get"),
                );
                return self.placeholder(span);
            }
            Type::Never => return self.placeholder(span),
            other if self.is_open(other) => {
                let diagnostic = Diagnostic::new(
                    "unresolved-type",
                    record_node.span,
                    "The checker cannot tell which record this is, so it cannot read a field from it.",
                );
                let diagnostic = if self.unfilled_loops > 0 {
                    diagnostic.detail(UNFILLED_LOOP_HINT)
                } else if let Some(open) = self.open_arguments.first() {
                    diagnostic.detail(self.open_argument_detail(open))
                } else {
                    diagnostic
                };
                self.implicate_open_arguments();
                self.report(diagnostic);
                return self.placeholder(span);
            }
            Type::Record(record) => record.clone(),
            other => {
                let optional = matches!(other, Type::Option(_));
                let read = optional.then(|| self.optional_read_of(record_node)).flatten();
                let hint = optional.then(|| match &read {
                    Some((hint, _)) => hint.clone(),
                    None => "Match on an optional value before reading its fields: (match x (some r) (get r :field) none ...)."
                        .to_string(),
                });
                let message = format!("get reads a field from a record, but this is {}.", describe(&self.zonk(other)));
                self.report(
                    Diagnostic::new("not-a-record", record_node.span, message)
                        .maybe_detail(hint)
                        .maybe_fix(read.map(|(_, fix)| fix))
                        .help("get"),
                );
                return self.placeholder(span);
            }
        };
        let Some(field_type) = record.field(key_name).cloned() else {
            let names: Vec<&str> = record.fields.iter().map(|(n, _)| n.as_str()).collect();
            let shown: Vec<String> = names.iter().map(|n| format!(":{n}")).collect();
            let suggestion = closest(key_name, &names);
            self.report(
                Diagnostic::new("unknown-field", key.span, format!("This record has no field :{key_name}."))
                    .detail(format!("Fields: {}", shown.join(", ")))
                    .maybe_fix(suggestion.map(|s| format!("(get {} :{s})", show(record_node))))
                    .help("get"),
            );
            return self.placeholder(span);
        };
        let getter = node(Ir::Get { record: Box::new(record_ir), field: key_name.to_string() }, span);
        let Some(default_node) = default_node else {
            let optional_read =
                matches!(self.resolve(&field_type), Type::Option(_)).then(|| self.optional_field_read(record_node, key_name));
            let options = Coercion { optional_read, ..Coercion::default() };
            return self.coerce_with(getter, &field_type, expected, span, options);
        };
        let Type::Option(inner) = self.resolve(&field_type) else {
            let ((default, default_type), effects) = self.collecting(|c| c.check(default_node, None, scope));
            if !effects.calls.is_empty() {
                self.report(
                    Diagnostic::new(
                        "arity",
                        default_node.span,
                        format!("The field :{key_name} is always present, so get takes no default that calls a tool."),
                    )
                    .detail("The default would never run. Leave the call out, or use a plain value as the default.")
                    .fix(format!("(get {} :{key_name})", show(record_node)))
                    .help("get"),
                );
            } else {
                self.coerce(default, &default_type, Some(&field_type), default_node.span);
            }
            return self.coerce(getter, &field_type, expected, span);
        };
        self.or_default(getter, &inner, default_node, expected, scope, span)
    }

    fn optional_read_of(&self, read: &Node) -> Option<(String, String)> {
        let items = form_items(read)?;
        if items.len() != 3 || symbol(&items[0]) != Some("get") {
            return None;
        }
        Some(self.optional_field_read(&items[1], keyword(&items[2])?))
    }

    fn record_lacking(&self, field: &str) -> Option<Span> {
        let has = |names: &[String]| names.iter().any(|name| name == field);
        self.record_literals.iter().find_map(|(span, names)| {
            let shares = |other: &[String]| other.iter().any(|name| names.contains(name));
            let lacks = !has(names)
                && self.record_literals.iter().any(|(other_span, other)| other_span != span && has(other) && shares(other));
            lacks.then_some(*span)
        })
    }

    fn optional_field_read(&self, record: &Node, field: &str) -> (String, String) {
        let shown = show(record);
        let why = match self.record_lacking(field) {
            Some(at) => format!(
                "The records that reach this point do not all have :{field}: the record at {}:{} has none.",
                at.start.line, at.start.column
            ),
            None => format!(
                "The field :{field} is optional: a tool declares it optional, or the records that reach this point do not all have it."
            ),
        };
        let hint = format!(
            "{why}\nSx does not narrow a field by a flag such as :known, so testing a flag does not make :{field} present.\n\
             Match on it, or give get a default: (get {shown} :{field} DEFAULT)."
        );
        let binder = if is_reserved(field) { "value" } else { field };
        (hint, format!("(match (get {shown} :{field}) (some {binder}) ... none ...)"))
    }

    fn or_default<'a>(
        &mut self,
        lookup: ir::Node,
        inner: &Type,
        default_node: &'a Node,
        expected: Option<&Type>,
        scope: &Scope<'a>,
        span: Span,
    ) -> Checked {
        let (default, default_type) = self.check(default_node, None, scope);
        let present = node(Ir::Variable { name: DEFAULT_BINDER.to_string() }, span);
        let binder = MatchPattern::Some(Box::new(MatchPattern::Any(Binder::Name(DEFAULT_BINDER.to_string()))));
        let choice = |lookup: ir::Node, present: ir::Node, default: ir::Node| {
            node(
                Ir::Match(ir::Match {
                    scrutinee: Box::new(lookup),
                    clauses: vec![(binder.clone(), present), (MatchPattern::None, default)],
                }),
                span,
            )
        };
        if let Type::Option(_) = self.resolve(&default_type) {
            let (nodes, t) =
                self.join(vec![(present, inner.clone(), span), (default, default_type, default_node.span)], expected);
            let mut nodes = nodes.into_iter();
            let (Some(present), Some(default)) = (nodes.next(), nodes.next()) else {
                return self.placeholder(span);
            };
            return (choice(lookup, present, default), t);
        }
        let (default, _) = self.coerce(default, &default_type, Some(inner), default_node.span);
        let made = choice(lookup, present, default);
        self.coerce(made, inner, expected, span)
    }

    fn is_brace(&self, t: &Type) -> bool {
        matches!(self.resolve(t), Type::Variable(id) if matches!(self.shape(id), Some(Shape::Brace)))
    }

    fn map_value_of(&mut self, t: &Type) -> Option<Type> {
        match self.resolve(t) {
            Type::Map(value) => Some(*value),
            resolved @ Type::Variable(_) => {
                let value = self.fresh();
                self.trial(&resolved, &map_of(value.clone())).then_some(value)
            }
            _ => None,
        }
    }

    fn map_value(&mut self, t: &Type, at: &Node, who: &str) -> Type {
        if let Some(value) = self.map_value_of(t) {
            return value;
        }
        let resolved = self.resolve(t);
        if !matches!(resolved, Type::Never) {
            let hint = matches!(resolved, Type::Record(_)).then_some("A record has fixed fields. Read one with get: (get r :field).");
            let zonked = self.zonk(&resolved);
            let mut detail = format!("Expected: Map<String, ...>\nFound: {}", show_type(&zonked));
            if let Some(hint) = hint {
                detail.push('\n');
                detail.push_str(hint);
            }
            self.report(
                Diagnostic::new("type-mismatch", at.span, format!("{who} takes a Map here, but this is {}.", describe(&zonked)))
                    .detail(detail)
                    .help(who),
            );
        }
        Type::Never
    }

    #[allow(clippy::too_many_arguments)]
    fn map_get<'a>(
        &mut self,
        span: Span,
        map_ir: ir::Node,
        value_type: &Type,
        key: &'a Node,
        default_node: Option<&'a Node>,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let (key_ir, _) = self.check(key, Some(&STRING), scope);
        let lookup = prelude(PreludeOp::MapGet, vec![map_ir, key_ir], span);
        match default_node {
            None => self.coerce(lookup, &option(value_type.clone()), expected, span),
            Some(default_node) => self.or_default(lookup, value_type, default_node, expected, scope, span),
        }
    }

    fn form_call<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let Some(name_node) = items.get(1).filter(|n| symbol(n).is_some()) else {
            self.report(
                Diagnostic::new("arity", span, "call takes a tool name and an input record: (call tool.name {...}).")
                    .help("call"),
            );
            return self.placeholder(span);
        };
        let name = symbol(name_node).unwrap_or_default();
        let Some(capability) = self.registry.capability(name) else {
            let ids: Vec<&str> = self.registry.capabilities().map(|c| c.id.as_str()).collect();
            let suggestion = closest(name, &ids);
            self.report(
                Diagnostic::new("unknown-capability", name_node.span, format!("There is no tool named {name}."))
                    .maybe_fix(suggestion),
            );
            for item in &items[2..] {
                self.check(item, None, scope);
            }
            return self.placeholder(span);
        };
        if items.len() != 3 {
            self.report(
                Diagnostic::new(
                    "arity",
                    span,
                    format!("call takes a tool name and one input, but this call has {} inputs.", items.len() - 2),
                )
                .detail(format!("Input: {}", show_type(&capability.input)))
                .fix(format!("(call {} {{...}})", capability.id))
                .help(capability.id.clone()),
            );
            return self.placeholder(span);
        }
        if let Authority::Granted(grants) = self.authority
            && !grants.contains(&capability.id)
        {
            let allowed: Vec<&str> = grants.ids().collect();
            let allowed = if allowed.is_empty() { "nothing".to_string() } else { allowed.join(", ") };
            self.report(
                Diagnostic::new("unauthorized", name_node.span, format!("This program may not call {}.", capability.id))
                    .detail(format!("The host allows: {allowed}")),
            );
        }
        let argument_node = &items[2];
        let (argument, _) = if let NodeKind::Record(_) = argument_node.kind {
            self.record(argument_node, Some(&capability.input), scope, Some(&capability.id))
        } else {
            self.check(argument_node, Some(&capability.input), scope)
        };
        self.check_scope(capability, &argument, argument_node.span);
        let depth = self.repeat_depth;
        for effects in &mut self.effects {
            effects.calls.push(EffectCall { capability, span, depth });
        }
        self.capabilities_used.insert(capability.id.clone());
        let call = node(Ir::Call { capability: capability.id.clone(), argument: Box::new(argument) }, span);
        let call = if self.dialect == Dialect::JavaScript {
            self.safe_decoded(call, &capability.output, span)
        } else {
            call
        };
        self.coerce(call, &capability.output, expected, span)
    }

    fn check_scope(&mut self, capability: &Capability, argument: &ir::Node, span: Span) {
        let Authority::Granted(grants) = self.authority else {
            return;
        };
        let Some(input) = Literal::of(argument) else {
            return;
        };
        let Screen::Verdict(Verdict::Denied(reason)) = grants.screen(&capability.id, &input.value()) else {
            return;
        };
        let granted = match grants.get(&capability.id) {
            Some(Grant::Scoped { scope, check }) => {
                let shown = check.as_ref().map_or_else(|| scope.clone(), |check| check.shown(scope));
                Some(format!("Granted scope: {}", show_value(&shown)))
            }
            _ => None,
        };
        self.report(
            Diagnostic::new(
                "out-of-scope",
                span,
                format!("The host's grant for {} does not cover this input: {reason}.", capability.id),
            )
            .maybe_detail(granted)
            .help(capability.id.clone()),
        );
    }

    fn form_some<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 1, Some(1), "value") {
            return self.placeholder(span);
        }
        let inner_expected = match expected.map(|e| self.resolve(e)) {
            Some(Type::Option(inner)) => Some(*inner),
            _ => None,
        };
        let (value, t) = self.check(&items[1], inner_expected.as_ref(), scope);
        let wrapped = option(t);
        let made = node(Ir::Wrap { kind: WrapKind::Some, value: Box::new(value), wrapped_type: wrapped.clone() }, span);
        self.coerce(made, &wrapped, expected, span)
    }

    fn wrap_result<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
        kind: WrapKind,
    ) -> Checked {
        if !self.arity(span, items, 1, Some(1), "value") {
            return self.placeholder(span);
        }
        let result = match expected.map(|e| self.resolve(e)) {
            Some(hint @ Type::Result(_, _)) => hint,
            _ => {
                let ok = self.fresh();
                let err = self.fresh();
                result_of(ok, err)
            }
        };
        let Type::Result(ok, err) = &result else {
            return self.placeholder(span);
        };
        let inner = if kind == WrapKind::Ok { (**ok).clone() } else { (**err).clone() };
        let (value, _) = self.check(&items[1], Some(&inner), scope);
        let made = node(Ir::Wrap { kind, value: Box::new(value), wrapped_type: result.clone() }, span);
        self.coerce(made, &result, expected, span)
    }

    fn form_attempt<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 1, Some(1), "expression") {
            return self.placeholder(span);
        }
        let hint = match expected.map(|e| self.resolve(e)) {
            Some(Type::Result(ok, _)) => Some(*ok),
            _ => None,
        };
        let (body, t) = self.check(&items[1], hint.as_ref(), scope);
        self.coerce(node(Ir::Attempt { body: Box::new(body) }, span), &result_of(t, call_error()), expected, span)
    }

    fn options<'a>(&mut self, syntax: &'a Node, form: &str, allowed: &[(&str, &str)]) -> Option<BTreeMap<String, &'a Node>> {
        let NodeKind::Record(fields) = &syntax.kind else {
            let example = allowed.first().map_or("", |(_, example)| example);
            self.report(
                Diagnostic::new("bad-option", syntax.span, format!("{form} takes its options as a record first, such as {example}."))
                    .help(form),
            );
            return None;
        };
        let mut found = BTreeMap::new();
        for f in fields.iter() {
            if !allowed.iter().any(|(name, _)| *name == f.key.name) {
                let names: Vec<String> = allowed.iter().map(|(name, _)| format!(":{name}")).collect();
                self.report(
                    Diagnostic::new("bad-option", f.key.span, format!("{form} has no option :{}.", f.key.name))
                        .detail(format!("Options: {}", names.join(", ")))
                        .help(form),
                );
                return None;
            }
            found.insert(f.key.name.clone(), &f.value);
        }
        Some(found)
    }

    fn durations(&mut self, syntax: &Node, form: &str, option_name: &str, count: Option<usize>) -> Option<Vec<i64>> {
        if let (NodeKind::Duration(_), None) = (&syntax.kind, count) {
            let shown = show(syntax);
            self.report(
                Diagnostic::new("bad-option", syntax.span, format!(":{option_name} takes a list of durations, one per retry."))
                    .detail(format!("A list states how many times to retry; [{shown}] retries once."))
                    .fix(format!("[{shown}]"))
                    .help(form),
            );
            return None;
        }
        if let Some(items) = list_items(syntax) {
            let found: Vec<i64> = items
                .iter()
                .filter_map(|i| match i.kind {
                    NodeKind::Duration(ms) => Some(ms),
                    _ => None,
                })
                .collect();
            if found.len() == items.len() && count.is_none_or(|c| c == items.len()) {
                return Some(found);
            }
        }
        let shape = match count {
            None => "a list of durations such as [1s 2s 4s]".to_string(),
            Some(count) => format!("a list of {count} durations such as [100ms 5s]"),
        };
        self.report(Diagnostic::new("bad-option", syntax.span, format!(":{option_name} takes {shape}.")).help(form));
        None
    }

    fn single_duration(&mut self, syntax: &Node, form: &str, option_name: &str) -> Option<i64> {
        if let NodeKind::Duration(ms) = syntax.kind {
            return Some(ms);
        }
        self.report(
            Diagnostic::new("bad-option", syntax.span, format!(":{option_name} takes a duration such as 30s.")).help(form),
        );
        None
    }

    fn backoff(first: i64, cap: i64, within: i64) -> Vec<i64> {
        let mut delays = Vec::new();
        let (mut delay, mut total) = (i128::from(first), 0i128);
        while total + delay <= i128::from(within) && delays.len() < MAX_REPEATS {
            delays.push(i64::try_from(delay).unwrap_or(i64::MAX));
            total += delay;
            delay = (delay * 2).min(i128::from(cap));
        }
        delays
    }

    fn form_retry<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 2, Some(2), "{:delays [1s 2s]} expression") {
            return self.placeholder(span);
        }
        let options = self.options(
            &items[1],
            "retry",
            &[("delays", "{:delays [1s 2s 4s]}"), ("backoff", ""), ("within", ""), ("on", "")],
        );
        let mut delays = Vec::new();
        let mut kinds: Vec<CallErrorKind> = CallErrorKind::TRANSIENT.to_vec();
        if let Some(options) = options {
            let has = |name: &str| options.contains_key(name);
            if has("delays") == has("backoff") {
                self.report(
                    Diagnostic::new("bad-option", items[1].span, "retry takes exactly one of :delays and :backoff.")
                        .help("retry"),
                );
            } else if let Some(delay_node) = options.get("delays") {
                if has("within") {
                    self.report(
                        Diagnostic::new("bad-option", items[1].span, ":within goes with :backoff, not :delays.").help("retry"),
                    );
                }
                delays = self.durations(delay_node, "retry", "delays", None).unwrap_or_default();
            } else if let Some(backoff_node) = options.get("backoff") {
                let pair = self.durations(backoff_node, "retry", "backoff", Some(2));
                let within = match options.get("within") {
                    Some(within_node) => self.single_duration(within_node, "retry", "within"),
                    None => None,
                };
                if !has("within") {
                    self.report(
                        Diagnostic::new("bad-option", items[1].span, ":backoff needs :within to bound the total delay.")
                            .fix("{:backoff [100ms 5s] :within 30s}")
                            .help("retry"),
                    );
                }
                if let (Some(pair), Some(within)) = (pair, within) {
                    if pair[0] < 1 || pair[1] < pair[0] {
                        self.report(
                            Diagnostic::new(
                                "bad-option",
                                backoff_node.span,
                                ":backoff takes a first delay of at least 1ms and a cap no smaller than it.",
                            )
                            .help("retry"),
                        );
                    } else {
                        delays = Checker::backoff(pair[0], pair[1], within);
                    }
                }
            }
            if let Some(on_node) = options.get("on")
                && let Some(found) = self.kinds(on_node)
            {
                kinds = found;
            }
        }
        let depth = self.repeat_depth;
        let ((body, t), effects) = self.collecting(|c| c.check(&items[2], expected, scope));
        self.check_retried_writes(&effects, &kinds, depth);
        kinds.sort_by_key(|k| k.name());
        (node(Ir::Retry(ir::Retry { delays, kinds, body: Box::new(body) }), span), t)
    }

    fn check_retried_writes(&mut self, effects: &Effects<'r>, kinds: &[CallErrorKind], depth: usize) {
        let unknown: Vec<&str> = CallErrorKind::ALL
            .iter()
            .filter(|k| kinds.contains(k) && !CallErrorKind::REFUSAL.contains(k))
            .map(|k| k.name())
            .collect();
        let refusals: Vec<&str> = CallErrorKind::REFUSAL.iter().map(|k| k.name()).collect();
        for (index, call) in effects.calls.iter().enumerate() {
            let capability = call.capability;
            if capability.effect != Effect::Write || capability.idempotent {
                continue;
            }
            let owner = format!("retry may repeat {}, which is a write that is not idempotent", capability.id);
            if !unknown.is_empty() {
                let allowed: Vec<&str> = CallErrorKind::ALL
                    .iter()
                    .filter(|k| kinds.contains(k) && CallErrorKind::REFUSAL.contains(k))
                    .map(|k| k.name())
                    .collect();
                let mut detail =
                    format!("Only the kinds {} mean the service did not perform the operation.", name_list(&refusals));
                let mut fix = None;
                if !allowed.is_empty() {
                    let text = format!(":on [{}]", allowed.join(" "));
                    detail.push_str(&format!("\nHere {text} alone would be allowed."));
                    fix = Some(text);
                }
                let verb = if unknown.len() == 1 { "leaves" } else { "leave" };
                self.report(
                    Diagnostic::new(
                        "retry-write",
                        call.span,
                        format!("{owner}, and {} {verb} it unknown whether the write happened.", name_list(&unknown)),
                    )
                    .detail(detail)
                    .maybe_fix(fix)
                    .help("retry"),
                );
            } else if call.depth > depth {
                self.report(
                    Diagnostic::new("retry-write", call.span, format!("{owner}, because it can run more than once inside this retry."))
                        .detail("Put the retry inside the fn, around the single write.")
                        .help("retry"),
                );
            } else if index + 1 < effects.calls.len() {
                self.report(
                    Diagnostic::new(
                        "retry-write",
                        call.span,
                        format!("{owner}, because a call after it can be refused once the write has taken effect."),
                    )
                    .detail("Retry the write alone, or make it the last call in the retried expression.")
                    .help("retry"),
                );
            }
        }
    }

    fn kinds(&mut self, syntax: &Node) -> Option<Vec<CallErrorKind>> {
        if let Some(items) = list_items(syntax)
            && !items.is_empty()
        {
            let found: Vec<CallErrorKind> =
                items.iter().filter_map(|i| symbol(i).and_then(CallErrorKind::from_name)).collect();
            if found.len() == items.len() {
                let mut distinct = found;
                distinct.sort();
                distinct.dedup();
                return Some(distinct);
            }
        }
        let names: Vec<&str> = CallErrorKind::ALL.iter().map(|k| k.name()).collect();
        self.report(
            Diagnostic::new("bad-option", syntax.span, ":on takes a list of error kinds, such as [Timeout Unavailable].")
                .detail(format!("Kinds: {}", names.join(" ")))
                .help("retry"),
        );
        None
    }

    fn form_wait_until<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 3, Some(3), "{:within 2m :every 5s} (fn [v] ...) expression") {
            return self.placeholder(span);
        }
        let options =
            self.options(&items[1], "wait-until", &[("within", "{:within 2m :every 5s}"), ("every", ""), ("backoff", "")]);
        let mut intervals = Vec::new();
        if let Some(options) = options {
            let within = match options.get("within") {
                Some(within_node) => self.single_duration(within_node, "wait-until", "within"),
                None => None,
            };
            if !options.contains_key("within") {
                self.report(
                    Diagnostic::new("bad-option", items[1].span, "wait-until needs :within to bound the total wait.")
                        .fix("{:within 2m :every 5s}")
                        .help("wait-until"),
                );
            }
            if options.contains_key("every") == options.contains_key("backoff") {
                self.report(
                    Diagnostic::new("bad-option", items[1].span, "wait-until takes exactly one of :every and :backoff.")
                        .help("wait-until"),
                );
            } else if let (Some(within), Some(every_node)) = (within, options.get("every")) {
                match self.single_duration(every_node, "wait-until", "every") {
                    Some(every) if every < 1 => {
                        self.report(
                            Diagnostic::new("bad-option", every_node.span, ":every must be at least 1ms.").help("wait-until"),
                        );
                    }
                    Some(every) => {
                        let count = usize::try_from(within.div_euclid(every)).unwrap_or(0).min(MAX_REPEATS);
                        intervals = vec![every; count];
                    }
                    None => {}
                }
            } else if let (Some(within), Some(backoff_node)) = (within, options.get("backoff")) {
                match self.durations(backoff_node, "wait-until", "backoff", Some(2)) {
                    Some(pair) if pair[0] < 1 || pair[1] < pair[0] => {
                        self.report(
                            Diagnostic::new(
                                "bad-option",
                                backoff_node.span,
                                ":backoff takes a first interval of at least 1ms and a cap no smaller than it.",
                            )
                            .help("wait-until"),
                        );
                    }
                    Some(pair) => intervals = Checker::backoff(pair[0], pair[1], within),
                    None => {}
                }
            }
        }
        let ((body, t), effects) = self.collecting(|c| c.check(&items[3], expected, scope));
        for call in &effects.calls {
            if call.capability.effect == Effect::Write {
                self.report(
                    Diagnostic::new(
                        "wait-write",
                        call.span,
                        format!(
                            "wait-until repeats its expression, so it cannot call {}, which writes.",
                            call.capability.id
                        ),
                    )
                    .help("wait-until"),
                );
            }
        }
        let (predicate, _) = self.function_argument(&items[2], vec![t.clone()], Some(&BOOL), "wait-until", scope);
        (node(Ir::WaitUntil(ir::WaitUntil { intervals, predicate, body: Box::new(body) }), span), t)
    }

    fn form_fail<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 1, Some(1), "message") {
            return self.placeholder(span);
        }
        let (message, t) = self.check(&items[1], None, scope);
        let resolved = self.resolve(&t);
        let accepted = match &resolved {
            Type::Never => true,
            Type::Variable(_) => self.trial(&resolved, &STRING),
            other => *other == STRING || other.is_call_error(),
        };
        if !accepted {
            let found = self.zonk(&t);
            self.report(
                Diagnostic::new(
                    "type-mismatch",
                    items[1].span,
                    format!("fail takes a String message, or a caught error to raise again, but this is {}.", describe(&found)),
                )
                .detail(format!("Expected: String or CallError\nFound: {}", show_type(&found)))
                .help("fail"),
            );
            return self.placeholder(span);
        }
        self.coerce(node(Ir::Fail { message: Box::new(message) }, span), &Type::Never, expected, span)
    }

    fn form_log<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let mut args = Vec::with_capacity(items.len() - 1);
        for item in &items[1..] {
            let (value, t) = self.check(item, None, scope);
            if self.dialect == Dialect::JavaScript {
                args.push(self.js_displayed(value, &t, item.span));
                continue;
            }
            self.late_json_values.push((t, item.span, "log"));
            args.push(value);
        }
        self.coerce(node(Ir::Log { args }, span), &UNIT, expected, span)
    }

    fn form_range<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 2, Some(3), signature("range")) {
            return self.placeholder(span);
        }
        let mut operands = vec![self.check(&items[1], Some(&INT), scope).0, self.check(&items[2], Some(&INT), scope).0];
        if let Some(step) = items.get(3) {
            if matches!(step.kind, NodeKind::Int(n) if n != 0) {
                operands.push(self.check(step, Some(&INT), scope).0);
            } else {
                self.report(
                    Diagnostic::new("bad-literal", step.span, "The step of range is a nonzero integer literal, such as 2 or -1.")
                        .detail("A step that is computed, zero, or not an integer would make the length of the list unknown.")
                        .help("range"),
                );
                return self.placeholder(span);
            }
        }
        self.coerce(prelude(PreludeOp::Range, operands, span), &list(INT), expected, span)
    }

    fn form_put<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let (target, key, value) = (&items[1], &items[2], &items[3]);
        let (target_ir, target_type) = self.check(target, None, scope);
        let mut resolved = self.resolve(&target_type);
        if self.is_brace(&resolved) {
            if self.dialect == Dialect::JavaScript && !matches!(key.kind, NodeKind::String(_)) {
                let value_type = self.fresh();
                self.trial(&resolved, &map_of(value_type));
            } else {
                self.trial(&resolved, &empty_record());
            }
            resolved = self.resolve(&resolved);
        }
        if let Some(value_type) = self.map_value_of(&target_type) {
            let (key_ir, _) = self.check(key, Some(&STRING), scope);
            let (value_ir, _) = self.check(value, Some(&value_type), scope);
            let operands = vec![target_ir, key_ir, value_ir];
            return self.coerce(prelude(PreludeOp::Assoc, operands, span), &map_of(value_type), expected, span);
        }
        let record = match resolved {
            Type::Never => return self.placeholder(span),
            Type::Record(record) if !record.open => record,
            Type::Record(_) => {
                self.report(
                    Diagnostic::new(
                        "type-mismatch",
                        target.span,
                        "put cannot replace a field of an open record, because the program does not see the fields its type leaves out.",
                    )
                    .help("put"),
                );
                return self.placeholder(span);
            }
            other if self.is_open(&other) => {
                self.report(Diagnostic::new(
                    "unresolved-type",
                    target.span,
                    "The checker cannot tell which record or Map this is, so it cannot put into it.",
                ));
                return self.placeholder(span);
            }
            other => {
                let found = self.zonk(&other);
                self.report(
                    Diagnostic::new("type-mismatch", target.span, format!("put takes a record or a Map, but this is {}.", describe(&found)))
                        .detail(format!("Expected: Record or Map<String, ...>\nFound: {}", show_type(&found)))
                        .help("put"),
                );
                return self.placeholder(span);
            }
        };
        let field = match &key.kind {
            NodeKind::String(field) => field.clone(),
            NodeKind::Keyword(field) => {
                self.report(
                    habit("keyword-value", key.span, "put names a record field with a string, such as \"name\".")
                        .fix(format!("\"{field}\""))
                        .help("put"),
                );
                return self.placeholder(span);
            }
            _ => {
                self.report(
                    Diagnostic::new("bad-literal", key.span, "put on a record names the field with a string literal, such as \"name\".")
                        .detail("A computed key is only for a Map.")
                        .help("put"),
                );
                return self.placeholder(span);
            }
        };
        let Some(field_type) = record.field(&field).cloned() else {
            let names: Vec<&str> = record.fields.iter().map(|(n, _)| n.as_str()).collect();
            let shown: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
            let suggestion = closest(&field, &names);
            self.report(
                Diagnostic::new("unknown-field", key.span, format!("This record has no field \"{field}\"."))
                    .detail(format!("Fields: {}", shown.join(", ")))
                    .maybe_fix(suggestion.map(|s| format!("\"{s}\"")))
                    .help("put"),
            );
            return self.placeholder(span);
        };
        let (value_ir, _) = self.check(value, Some(&field_type), scope);
        let held = node(Ir::Variable { name: PUT_BINDER.to_string() }, span);
        let fields = record
            .fields
            .iter()
            .map(|(name, _)| {
                let part = if *name == field {
                    value_ir.clone()
                } else {
                    node(Ir::Get { record: Box::new(held.clone()), field: name.clone() }, span)
                };
                (name.clone(), part)
            })
            .collect();
        let rebuilt = node(
            Ir::Let(ir::Let {
                binder: Binder::Name(PUT_BINDER.to_string()),
                value: Box::new(target_ir),
                body: Box::new(node(Ir::MakeRecord { fields }, span)),
            }),
            span,
        );
        self.coerce(rebuilt, &Type::Record(record), expected, span)
    }

    fn form_sleep(&mut self, span: Span, items: &[Node], expected: Option<&Type>) -> Checked {
        if !self.arity(span, items, 1, Some(1), "duration") {
            return self.placeholder(span);
        }
        let NodeKind::Duration(milliseconds) = items[1].kind else {
            self.report(
                Diagnostic::new("bad-option", items[1].span, "sleep takes a duration literal such as 10s.")
                    .detail("A duration is written with a unit: 250ms, 5s, 2m, or 1h.")
                    .help("sleep"),
            );
            return self.placeholder(span);
        };
        self.coerce(node(Ir::Sleep { milliseconds }, span), &UNIT, expected, span)
    }

    fn form_loop<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if let Some(bindings) = items.get(1).filter(|n| list_items(n).is_some()) {
            self.report(
                habit("loop-without-max", bindings.span, "A loop needs a bound on its repetitions.")
                    .fix(format!("(loop {{:max 100}} {} ...)", show(bindings)))
                    .help("loop"),
            );
            return self.placeholder(span);
        }
        if !self.arity(span, items, 3, Some(3), "{:max 100} [name value ...] body") {
            return self.placeholder(span);
        }
        let mut maximum = 1;
        if let Some(options) = self.options(&items[1], "loop", &[("max", "{:max 100}")]) {
            match options.get("max").map(|n| &n.kind) {
                Some(NodeKind::Int(value)) if *value >= 1 => maximum = *value,
                _ => {
                    self.report(
                        Diagnostic::new(
                            "bad-option",
                            items[1].span,
                            "loop takes {:max N}, where N is an integer literal of at least 1.",
                        )
                        .help("loop"),
                    );
                }
            }
        }
        let bindings_node = &items[2];
        let Some(bindings) = list_items(bindings_node).filter(|b| b.len() % 2 == 0) else {
            self.report(
                Diagnostic::new("bad-pattern", bindings_node.span, "loop needs a binding vector that pairs each name with a value.")
                    .help("loop"),
            );
            return self.placeholder(span);
        };
        let mut inner = scope.clone();
        let mut binders = Vec::new();
        let mut initial = Vec::new();
        let mut types = Vec::new();
        self.outside_tail(|c| {
            for pair in bindings.chunks(2) {
                let (value_ir, value_type) = c.check(&pair[1], None, scope);
                let (binder, next) = c.bind(&pair[0], &value_type, &inner, "loop", None);
                inner = next;
                binders.push(binder);
                initial.push(value_ir);
                types.push(value_type);
            }
        });
        let (body, t) = if types.iter().any(|t| self.unsettled(t)) {
            self.check_accumulating_loop(&items[3], expected, &inner, types)
        } else {
            self.check_loop_body(&items[3], expected, &inner, types)
        };
        if always_recurs(&body) {
            self.report(
                Diagnostic::new(
                    "loop-no-result",
                    items[3].span,
                    "This loop body always ends in recur, so the loop has no result and can only fail with loop-limit.",
                )
                .detail("Give the body a branch that does not recur, such as (if done? result (recur ...)).")
                .help("loop"),
            );
        }
        (node(Ir::Loop(ir::Loop { maximum, binders, initial, body: Box::new(body) }), span), t)
    }

    fn unsettled(&self, t: &Type) -> bool {
        match self.resolve(t) {
            Type::Variable(id) => match self.shape(id) {
                Some(Shape::Bracket { element, .. }) => self.unsettled(element),
                Some(Shape::Brace) | None => true,
            },
            Type::List(inner) | Type::Map(inner) | Type::Option(inner) => self.unsettled(&inner),
            Type::Tuple(items) => items.iter().any(|item| self.unsettled(item)),
            Type::Result(ok, err) => self.unsettled(&ok) || self.unsettled(&err),
            Type::Record(record) => record.fields.iter().any(|(_, field_type)| self.unsettled(field_type)),
            _ => false,
        }
    }

    fn check_loop_body<'a>(&mut self, body: &'a Node, expected: Option<&Type>, scope: &Scope<'a>, types: Vec<Type>) -> Checked {
        let saved = self.tail.replace(LoopContext { types });
        let checked = self.repeated(|c| c.check(body, expected, scope));
        self.tail = saved;
        checked
    }

    fn check_accumulating_loop<'a>(
        &mut self,
        body: &'a Node,
        expected: Option<&Type>,
        scope: &Scope<'a>,
        types: Vec<Type>,
    ) -> Checked {
        let before = Speculation::start(self);
        let (checked, effects) = self.collecting(|c| c.check_loop_body(body, expected, scope, types.clone()));
        if !self.diagnostics[before.diagnostics..].iter().any(|d| d.code == "unresolved-type") {
            if let Some(outer) = self.effects.last_mut() {
                outer.calls.extend(effects.calls);
            }
            return checked;
        }
        before.undo(self);
        self.collecting(|c| c.check_loop_body(body, expected, scope, types.clone()));
        before.forget(self);
        let unfilled = types.iter().any(|t| self.unsettled(t));
        self.unfilled_loops += usize::from(unfilled);
        let checked = self.check_loop_body(body, expected, scope, types);
        self.unfilled_loops -= usize::from(unfilled);
        checked
    }

    fn form_recur<'a>(&mut self, span: Span, items: &'a [Node], scope: &Scope<'a>) -> Checked {
        let Some(types) = self.tail.as_ref().map(|context| context.types.clone()) else {
            self.report(
                Diagnostic::new("recur-position", span, "recur must be the last step of a loop body, and never inside a fn.")
                    .help("loop"),
            );
            self.outside_tail(|c| {
                for item in &items[1..] {
                    c.check(item, None, scope);
                }
            });
            return self.placeholder(span);
        };
        let args = &items[1..];
        let count = types.len();
        if args.len() != count {
            let plural = if count != 1 { "s" } else { "" };
            self.report(
                Diagnostic::new(
                    "recur-arity",
                    span,
                    format!(
                        "This loop binds {count} name{plural}, so recur takes {count} value{plural}, but this one has {}.",
                        args.len()
                    ),
                )
                .help("loop"),
            );
            return self.placeholder(span);
        }
        let checked = self.outside_tail(|c| args.iter().zip(&types).map(|(arg, t)| c.check(arg, Some(t), scope).0).collect());
        (node(Ir::Recur { args: checked }, span), Type::Never)
    }

    fn form_par<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 1, None, "expression ...") {
            return self.placeholder(span);
        }
        let count = items.len() - 1;
        let hints: Vec<Option<Type>> = match expected.map(|e| self.resolve(e)) {
            Some(Type::Tuple(types)) if types.len() == count => types.into_iter().map(Some).collect(),
            _ => vec![None; count],
        };
        let mut branches = Vec::new();
        let mut types = Vec::new();
        for (branch_node, hint) in items[1..].iter().zip(&hints) {
            let ((branch, t), effects) = self.collecting(|c| c.check(branch_node, hint.as_ref(), scope));
            self.read_only(&effects, "par");
            branches.push(branch);
            types.push(t);
        }
        self.coerce(node(Ir::Par { branches }, span), &Type::Tuple(types), expected, span)
    }

    fn read_only(&mut self, effects: &Effects<'r>, form: &str) {
        for call in &effects.calls {
            if call.capability.effect == Effect::Write {
                self.report(
                    Diagnostic::new(
                        "parallel-write",
                        call.span,
                        format!("{form} runs in parallel, so it cannot call {}, which writes.", call.capability.id),
                    )
                    .detail("Call writes outside par and par-map, one at a time.")
                    .help(form),
                );
            }
        }
    }

    fn form_par_map<'a>(&mut self, span: Span, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        if !self.arity(span, items, 3, Some(3), "{:limit 8} (fn [x] ...) xs") {
            return self.placeholder(span);
        }
        let mut limit = 1;
        if let Some(options) = self.options(&items[1], "par-map", &[("limit", "{:limit 8}")]) {
            let concurrency = self.budgets.concurrency;
            match options.get("limit").map(|n| &n.kind) {
                Some(NodeKind::Int(value)) if *value >= 1 && u64::try_from(*value).is_ok_and(|v| v <= concurrency) => {
                    limit = *value;
                }
                _ => {
                    self.report(
                        Diagnostic::new(
                            "par-map-limit",
                            items[1].span,
                            format!("par-map takes {{:limit N}}, where N is an integer literal from 1 to {concurrency}."),
                        )
                        .help("par-map"),
                    );
                }
            }
        }
        let (items_ir, items_type) = self.check(&items[3], None, scope);
        let element = self.list_element(&items_type, &items[3], "par-map");
        let ((function, result), effects) =
            self.collecting(|c| c.function_argument(&items[2], vec![element], None, "par-map", scope));
        self.read_only(&effects, "par-map");
        let made = node(Ir::ParMap { limit, function, items: Box::new(items_ir) }, span);
        self.coerce(made, &list(result), expected, span)
    }

    fn tuple_item(
        &mut self,
        items_ir: ir::Node,
        t: &Type,
        index: i64,
        at: &Node,
        span: Span,
        expected: Option<&Type>,
    ) -> Checked {
        let Type::Tuple(types) = self.resolve(t) else {
            return self.placeholder(span);
        };
        let size = types.len();
        let Some(element) = usize::try_from(index).ok().and_then(|i| types.get(i)) else {
            self.report(
                Diagnostic::new(
                    "type-mismatch",
                    at.span,
                    format!("This tuple has {size} elements, so its indexes are 0 to {}, not {index}.", size as i64 - 1),
                )
                .help("nth"),
            );
            return self.placeholder(span);
        };
        let element = element.clone();
        let made = prelude(PreludeOp::TupleItem, vec![items_ir, literal(Value::Int(index), INT, span)], span);
        self.coerce(made, &element, expected, span)
    }

    fn list_element(&mut self, t: &Type, at: &Node, who: &str) -> Type {
        let resolved = self.resolve(t);
        if let Type::List(element) = resolved {
            return *element;
        }
        if let Type::Variable(_) = resolved {
            let element = self.fresh();
            if self.trial(t, &list(element.clone())) {
                return element;
            }
        }
        if !matches!(resolved, Type::Never) {
            let hint = (resolved == STRING)
                .then_some("text functions take strings; split one into a list with text.split or text.chars.");
            let zonked = self.zonk(&resolved);
            let mut detail = format!("Expected: List<...>\nFound: {}", show_type(&zonked));
            if let Some(hint) = hint {
                detail.push('\n');
                detail.push_str(hint);
            }
            self.report(
                Diagnostic::new("type-mismatch", at.span, format!("{who} takes a list here, but this is {}.", describe(&zonked)))
                    .detail(detail)
                    .help(who),
            );
        }
        Type::Never
    }

    fn function_argument<'a>(
        &mut self,
        syntax: &'a Node,
        params: Vec<Type>,
        expected_return: Option<&Type>,
        who: &str,
        scope: &Scope<'a>,
    ) -> (Lambda, Type) {
        let span = syntax.span;
        if is_function_form(syntax) {
            if !self.check_fn_shape(syntax) {
                return (empty_lambda(span, None), Type::Never);
            }
            return self.repeated(|c| c.instantiate(syntax, params, expected_return, who, scope, None));
        }
        if let NodeKind::Symbol(name) = &syntax.kind {
            match scope.get(name) {
                Some(Binding::Function(binding)) => {
                    if self.instantiating.contains(name) {
                        self.report(Diagnostic::new(
                            "unknown-function",
                            span,
                            format!("{name} cannot call itself. Sx has no recursion."),
                        ));
                        return (empty_lambda(span, None), Type::Never);
                    }
                    let binding = binding.clone();
                    return self.repeated(|c| {
                        c.instantiate(binding.form, params, expected_return, who, &binding.scope, Some(name))
                    });
                }
                None if is_prelude(name) || self.registry.function(name).is_some() => {
                    return self.named_function(syntax, name, params, expected_return, scope);
                }
                _ => {}
            }
        }
        if let NodeKind::Keyword(name) = &syntax.kind {
            self.report(
                habit("keyword-call", span, "A keyword is not a function.")
                    .fix(format!("(fn [r] (get r :{name}))"))
                    .help("get"),
            );
            return (empty_lambda(span, None), Type::Never);
        }
        self.report(
            Diagnostic::new(
                "type-mismatch",
                span,
                format!("{who} takes a function here, such as (fn [x] ...), but this is {}.", show(syntax)),
            )
            .help(who),
        );
        self.check(syntax, None, scope);
        (empty_lambda(span, None), Type::Never)
    }

    fn named_function<'a>(
        &mut self,
        syntax: &Node,
        name: &str,
        params: Vec<Type>,
        expected_return: Option<&Type>,
        scope: &Scope<'a>,
    ) -> (Lambda, Type) {
        let span = syntax.span;
        let names: Vec<String> = (0..params.len()).map(|i| format!("#{i}")).collect();
        let mut inner = scope.clone();
        for (param, t) in names.iter().zip(&params) {
            inner = inner.value(param, t.clone());
        }
        let mut parts = vec![Node { kind: NodeKind::Symbol(name.to_string()), span }];
        parts.extend(names.iter().map(|param| Node { kind: NodeKind::Symbol(param.clone()), span }));
        let call = Node { kind: NodeKind::Form(Items::new(parts)), span };
        let (body, t) = self.outside_tail(|c| c.check(&call, expected_return, &inner));
        let lambda = Lambda {
            params: names.into_iter().map(Binder::Name).collect(),
            param_types: params,
            result: t.clone(),
            body: Box::new(body),
            captured: None,
            span,
        };
        (lambda, t)
    }

    fn instantiate<'a>(
        &mut self,
        form: &'a Node,
        params: Vec<Type>,
        expected_return: Option<&Type>,
        who: &str,
        scope: &Scope<'a>,
        local: Option<&str>,
    ) -> (Lambda, Type) {
        let span = form.span;
        let captured = local.map(str::to_string);
        self.instantiations += 1;
        if self.instantiations > self.budgets.instantiations {
            if self.instantiations == self.budgets.instantiations + 1 {
                self.report(Diagnostic::new(
                    "too-large",
                    span,
                    "Local functions expand into more instantiations than the budget allows.",
                ));
            }
            return (empty_lambda(span, captured), Type::Never);
        }
        let Some((param_nodes, body_node)) = Checker::fn_shape(form) else {
            return (empty_lambda(span, captured), Type::Never);
        };
        if param_nodes.len() != params.len() {
            let given = param_nodes.len();
            let plural = if given != 1 { "s" } else { "" };
            let params_span = form_items(form).and_then(|items| items.get(1)).map_or(span, |n| n.span);
            self.report(
                Diagnostic::new(
                    "arity",
                    params_span,
                    format!("This function takes {given} parameter{plural}, but {who} passes {}.", params.len()),
                )
                .maybe_help(local.is_none().then(|| who.to_string())),
            );
            return (empty_lambda(span, captured), Type::Never);
        }
        let key = std::ptr::from_ref(form).addr();
        let open = self.open_arguments.len();
        if let Some(local) = local {
            self.open_parameters(key, local, param_nodes, &params);
        }
        let mut inner = scope.clone();
        let mut binders = Vec::new();
        for (pattern, t) in param_nodes.iter().zip(&params) {
            let fix = self.record_fix(pattern, t, body_node);
            let (binder, next) = self.bind(pattern, t, &inner, who, fix);
            binders.push(binder);
            inner = next;
        }
        if let Some(local) = local {
            self.instantiating.push(local.to_string());
        }
        let saved = self.tail.take();
        let (body, t) = self.check(body_node, expected_return, &inner);
        self.tail = saved;
        if local.is_some() {
            self.instantiating.pop();
            self.open_arguments.truncate(open);
            self.observe_parameters(key, &params);
        }
        let lambda =
            Lambda { params: binders, param_types: params, result: t.clone(), body: Box::new(body), captured, span };
        (lambda, t)
    }

    fn open_parameters(&mut self, key: usize, function: &str, patterns: &[Node], params: &[Type]) {
        for (index, (pattern, t)) in patterns.iter().zip(params).enumerate() {
            if !self.unsettled(t) {
                continue;
            }
            if let Some(agreed) = self.earlier.agreed((key, index)).cloned() {
                self.trial(t, &agreed);
            }
            if self.unsettled(t) {
                self.open_arguments.push(OpenArgument {
                    key: (key, index),
                    function: function.to_string(),
                    parameter: symbol(pattern).map_or_else(|| show(pattern), str::to_string),
                });
            }
        }
    }

    fn observe_parameters(&mut self, key: usize, params: &[Type]) {
        for (index, t) in params.iter().enumerate() {
            if self.unsettled(t) {
                continue;
            }
            let known = self.zonk(t);
            if mentions_never(&known) {
                continue;
            }
            let seen = self.arguments.observed.entry((key, index)).or_default();
            if !seen.contains(&known) {
                seen.push(known);
            }
        }
    }

    fn open_argument_detail(&self, open: &OpenArgument) -> String {
        let (function, parameter) = (&open.function, &open.parameter);
        let others = match self.earlier.observed.get(&open.key).map(Vec::as_slice) {
            None | Some([]) => format!("No other call of {function} passes {parameter} a value with a known type."),
            Some([only]) => {
                format!("Other calls of {function} pass {parameter} as {}, which does not fit this call.", show_type(only))
            }
            Some(several) => {
                let shown: Vec<String> = several.iter().map(show_type).collect();
                format!("The other calls of {function} pass {parameter} as different types: {}.", shown.join("; "))
            }
        };
        format!(
            "The parameter {parameter} of {function} is passed [], {{}}, or none at a call, and nothing gives that value a type. {others}\n\
             Pass {function} a value of the right shape at this call, or pass the same type for {parameter} at every call."
        )
    }

    fn implicate_open_arguments(&mut self) {
        let keys: Vec<ArgumentKey> = self.open_arguments.iter().map(|open| open.key).collect();
        self.arguments.implicated.extend(keys);
    }

    fn record_fix(&self, pattern: &Node, t: &Type, body: &Node) -> Option<String> {
        let items = list_items(pattern)?;
        let Type::Record(record) = self.resolve(t) else {
            return None;
        };
        let body_name = symbol(body)?;
        let index = items.iter().position(|item| symbol(item) == Some(body_name))?;
        let (field, _) = record.fields.get(index)?;
        Some(format!("(fn [r] (get r :{field}))"))
    }

    fn apply_local<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        binding: &FunctionBinding<'a>,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let name = binding.name.as_str();
        if self.instantiating.iter().any(|n| n == name) {
            self.report(Diagnostic::new(
                "unknown-function",
                items[0].span,
                format!("{name} cannot call itself. Sx has no recursion."),
            ));
            return self.placeholder(span);
        }
        let mut args = Vec::new();
        let mut types = Vec::new();
        for arg in &items[1..] {
            let (value, t) = self.check(arg, None, scope);
            args.push(value);
            types.push(t);
        }
        let (function, t) = self.instantiate(binding.form, types, expected, name, &binding.scope, Some(name));
        let made = node(Ir::ApplyLocal { name: name.to_string(), function, args }, span);
        self.coerce(made, &t, expected, span)
    }

    fn core<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        function: &'r Function,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let params: Vec<String> = function.params.iter().map(show_type).collect();
        let signature = format!("{} -> {}", params.join(" "), show_type(&function.output));
        let count = function.params.len();
        if !self.arity(span, items, count, Some(count), &signature) {
            return self.placeholder(span);
        }
        let mut args = Vec::new();
        for (index, (arg, param)) in items[1..].iter().zip(&function.params).enumerate() {
            if let NodeKind::String(text) = &arg.kind {
                for literal in function.literal_checks.iter().filter(|literal| literal.argument == index) {
                    if let Err(message) = (literal.check)(text) {
                        self.report(
                            Diagnostic::new("bad-literal", arg.span, format!("{} cannot use this literal: {message}", function.name))
                                .help(function.name.clone()),
                        );
                    }
                }
            }
            let names_a_variant = matches!(arg.kind, NodeKind::String(_)) && matches!(param, Type::Enum(_));
            let (value, t) = self.check(arg, names_a_variant.then_some(param), scope);
            let message = format!(
                "{} takes {} here, but this is {}.",
                function.name,
                describe(param),
                describe(&self.zonk(&t))
            );
            let options = Coercion {
                message: Some(message),
                help: Some(function.name.clone()),
                source: Some(arg),
                ..Coercion::default()
            };
            args.push(self.coerce_with(value, &t, Some(param), arg.span, options).0);
        }
        self.functions_used.insert(function.name.clone());
        let made = node(Ir::CoreCall { function: function.name.clone(), args }, span);
        self.coerce(made, &function.output, expected, span)
    }
}

impl<'r> Checker<'r> {
    fn prelude<'a>(&mut self, syntax: &'a Node, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let span = syntax.span;
        let op = symbol(&items[0]).unwrap_or("?");
        let args = &items[1..];
        if is_higher_order(op)
            && args.len() >= 2
            && !self.looks_like_function(&args[0], scope)
            && self.looks_like_function(&args[args.len() - 1], scope)
        {
            let mut order: Vec<String> = vec![show(&args[args.len() - 1])];
            order.extend(args[..args.len() - 1].iter().map(show));
            self.report(
                habit("argument-order", span, format!("{op} takes the function first, as in Clojure."))
                    .fix(format!("({op} {})", order.join(" ")))
                    .help(op),
            );
            return self.placeholder(span);
        }
        if self.dialect == Dialect::JavaScript
            && let Some(checked) = self.js_operator(syntax, items, op, expected, scope)
        {
            return checked;
        }
        if op == "-" {
            if !self.arity(span, items, 1, Some(2), "a b") {
                return self.placeholder(span);
            }
            let (operands, number) = self.number_operands(op, args, scope, Vec::new());
            return self.coerce(prelude(PreludeOp::Subtract, operands, span), &number, expected, span);
        }
        if op == "str" {
            if !self.arity(span, items, 1, None, "String ...") {
                return self.placeholder(span);
            }
            return self.prelude_str(syntax, items, expected, scope);
        }
        if matches!(op, "assoc" | "dissoc") && args.len() >= 2 && keyword(&args[1]).is_some() {
            return self.unknown_head(span, items, scope);
        }
        if op == "range" {
            return self.form_range(span, items, expected, scope);
        }
        if let Some(count) = fixed_arity(op)
            && !self.arity(span, items, count, Some(count), signature(op))
        {
            return self.placeholder(span);
        }
        if op == "put" {
            return self.form_put(span, items, expected, scope);
        }
        if op == "identity" {
            return self.check(&args[0], expected, scope);
        }
        let Some(prelude_op) = PreludeOp::from_name(op) else {
            return self.placeholder(span);
        };
        match op {
            "quot" | "rem" => {
                let checked = args.iter().map(|arg| self.check(arg, Some(&INT), scope).0).collect();
                self.coerce(prelude(prelude_op, checked, span), &INT, expected, span)
            }
            "+" | "*" | "/" => {
                let (operands, number) = self.number_operands(op, args, scope, Vec::new());
                if op == "/" && number == INT {
                    self.report(
                        Diagnostic::new("type-mismatch", span, "/ divides Floats. Int division is quot, which truncates toward zero.")
                            .detail("For a Float quotient of Ints, convert first: (/ (float a) (float b)).")
                            .fix(format!("(quot {} {})", show(&args[0]), show(&args[1])))
                            .help("quot"),
                    );
                }
                self.coerce(prelude(prelude_op, operands, span), &number, expected, span)
            }
            "float" => {
                let (value, _) = self.check(&args[0], Some(&INT), scope);
                self.coerce(prelude(prelude_op, vec![value], span), &FLOAT, expected, span)
            }
            "round" | "floor" | "ceil" => {
                let (value, _) = self.check(&args[0], Some(&FLOAT), scope);
                self.coerce(prelude(prelude_op, vec![value], span), &INT, expected, span)
            }
            "=" | "not=" => {
                let variant_first = symbol(&args[0]).is_some_and(|name| first_upper(name) && scope.get(name).is_none());
                let string_first =
                    matches!(args[0].kind, NodeKind::String(_)) && !matches!(args[1].kind, NodeKind::String(_));
                let (one, two) = if variant_first || string_first { (1, 0) } else { (0, 1) };
                let (first, first_type) = self.check(&args[one], None, scope);
                if matches!(self.resolve(&first_type), Type::Primitive(Primitive::Int | Primitive::Float)) {
                    let ordered = [&args[one], &args[two]];
                    let (pair, _) = self.number_operands_of(op, &ordered, scope, vec![(first, first_type)]);
                    let pair = if one == 0 { pair } else { pair.into_iter().rev().collect() };
                    return self.coerce(prelude(prelude_op, pair, span), &BOOL, expected, span);
                }
                let (second, _) = self.check(&args[two], Some(&first_type), scope);
                let pair = if one == 0 { vec![first, second] } else { vec![second, first] };
                self.coerce(prelude(prelude_op, pair, span), &BOOL, expected, span)
            }
            "<" | "<=" | ">" | ">=" => {
                let (first, first_type) = self.check(&args[0], None, scope);
                if matches!(self.resolve(&first_type), Type::Primitive(Primitive::Int | Primitive::Float)) {
                    let (pair, _) = self.number_operands(op, args, scope, vec![(first, first_type)]);
                    return self.coerce(prelude(prelude_op, pair, span), &BOOL, expected, span);
                }
                let (second, _) = self.check(&args[1], Some(&first_type), scope);
                let resolved = self.resolve(&first_type);
                if !matches!(resolved, Type::Never) && !self.is_open(&resolved) && resolved != STRING {
                    let hint = match &resolved {
                        Type::Option(_) => Some(OPTIONAL_HINT.to_string()),
                        Type::Nominal(nominal) => nominal
                            .validation
                            .as_ref()
                            .map(|validation| format!("Compare two {} with {}.", nominal.name, validation.compare)),
                        _ => None,
                    };
                    let message =
                        format!("{op} compares integers, floats, or strings, but this is {}.", describe(&self.zonk(&resolved)));
                    self.report(Diagnostic::new("type-mismatch", args[0].span, message).maybe_detail(hint).help(op));
                }
                self.coerce(prelude(prelude_op, vec![first, second], span), &BOOL, expected, span)
            }
            "not" => {
                let value = self.check(&args[0], Some(&BOOL), scope).0;
                self.coerce(prelude(prelude_op, vec![value], span), &BOOL, expected, span)
            }
            "and" | "or" => {
                let checked = args.iter().map(|arg| self.check(arg, Some(&BOOL), scope).0).collect();
                self.coerce(prelude(prelude_op, checked, span), &BOOL, expected, span)
            }
            "count" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                if self.resolve(&t) == BYTES {
                    return self.coerce(prelude(prelude_op, vec![items_ir], span), &INT, expected, span);
                }
                if self.is_map(&t) {
                    self.map_value(&t, &args[0], "count");
                } else {
                    self.list_element(&t, &args[0], "count");
                }
                self.coerce(prelude(prelude_op, vec![items_ir], span), &INT, expected, span)
            }
            "first" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                if let Type::Tuple(_) = self.resolve(&t) {
                    return self.tuple_item(items_ir, &t, 0, &args[0], span, expected);
                }
                let element = self.list_element(&t, &args[0], "first");
                self.coerce(prelude(prelude_op, vec![items_ir], span), &element, expected, span)
            }
            "rest" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let element = self.list_element(&t, &args[0], "rest");
                self.coerce(prelude(prelude_op, vec![items_ir], span), &list(element), expected, span)
            }
            "nth" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                if let Type::Tuple(types) = self.resolve(&t) {
                    let NodeKind::Int(index) = args[1].kind else {
                        self.report(
                            Diagnostic::new(
                                "type-mismatch",
                                args[1].span,
                                "A tuple's elements have their own types, so nth on a tuple needs a literal index.",
                            )
                            .detail(format!("Take it apart instead: (let [{}] ...).", tuple_names(types.len())))
                            .help("nth"),
                        );
                        return self.placeholder(span);
                    };
                    return self.tuple_item(items_ir, &t, index, &args[1], span, expected);
                }
                let element = self.list_element(&t, &args[0], "nth");
                let (index, _) = self.check(&args[1], Some(&INT), scope);
                self.coerce(prelude(prelude_op, vec![items_ir, index], span), &element, expected, span)
            }
            "take" => {
                let (count, _) = self.check(&args[0], Some(&INT), scope);
                let (items_ir, t) = self.check(&args[1], None, scope);
                let element = self.list_element(&t, &args[1], "take");
                self.coerce(prelude(prelude_op, vec![count, items_ir], span), &list(element), expected, span)
            }
            "to-json" => {
                let (value, t) = self.check(&args[0], None, scope);
                self.late_json_values.push((t, args[0].span, "to-json"));
                self.coerce(prelude(prelude_op, vec![value], span), &STRING, expected, span)
            }
            "enumerate" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let element = self.list_element(&t, &args[0], "enumerate");
                let pair = Type::Tuple(vec![INT, element]);
                self.coerce(prelude(prelude_op, vec![items_ir], span), &list(pair), expected, span)
            }
            "safe-int" => {
                let (value, _) = self.check(&args[0], Some(&INT), scope);
                self.coerce(prelude(prelude_op, vec![value], span), &INT, expected, span)
            }
            "concat" => self.form_concat(prelude_op, span, args, expected, scope),
            "flatten" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let outer = self.list_element(&t, &args[0], "flatten");
                let inner = if matches!(self.resolve(&outer), Type::Never) {
                    Type::Never
                } else {
                    self.list_element(&outer, &args[0], "flatten")
                };
                self.coerce(prelude(prelude_op, vec![items_ir], span), &list(inner), expected, span)
            }
            "contains?" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let element = if self.is_map(&t) {
                    self.map_value(&t, &args[0], "contains?");
                    STRING
                } else {
                    self.list_element(&t, &args[0], "contains?")
                };
                let (value, _) = self.check(&args[1], Some(&element), scope);
                self.coerce(prelude(prelude_op, vec![items_ir, value], span), &BOOL, expected, span)
            }
            "assoc" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let value_type = self.map_value(&t, &args[0], "assoc");
                let (key, _) = self.check(&args[1], Some(&STRING), scope);
                let (value, _) = self.check(&args[2], Some(&value_type), scope);
                self.coerce(prelude(prelude_op, vec![items_ir, key, value], span), &map_of(value_type), expected, span)
            }
            "dissoc" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let value_type = self.map_value(&t, &args[0], "dissoc");
                let (key, _) = self.check(&args[1], Some(&STRING), scope);
                self.coerce(prelude(prelude_op, vec![items_ir, key], span), &map_of(value_type), expected, span)
            }
            "keys" | "vals" | "entries" => {
                let (items_ir, t) = self.check(&args[0], None, scope);
                let value_type = self.map_value(&t, &args[0], op);
                let result = match op {
                    "keys" => list(STRING),
                    "vals" => list(value_type),
                    _ => list(Type::Tuple(vec![STRING, value_type])),
                };
                self.coerce(prelude(prelude_op, vec![items_ir], span), &result, expected, span)
            }
            "into-map" => {
                let (pairs, t) = self.check(&args[0], None, scope);
                let mut value_type = self.fresh();
                if !self.trial(&t, &list(Type::Tuple(vec![STRING, value_type.clone()]))) {
                    let found = self.zonk(&t);
                    self.report(
                        Diagnostic::new(
                            "type-mismatch",
                            args[0].span,
                            format!("into-map takes a list of [String value] pairs, but this is {}.", describe(&found)),
                        )
                        .detail(format!("Expected: List<Tuple<String, ...>>\nFound: {}", show_type(&found)))
                        .help("into-map"),
                    );
                    value_type = Type::Never;
                }
                self.coerce(prelude(prelude_op, vec![pairs], span), &map_of(value_type), expected, span)
            }
            "reduce" => {
                let state_hint = self.state_tuple_hint(&args[0], &args[1]);
                let (mut initial, mut accumulator) = self.check(&args[1], state_hint.as_ref(), scope);
                let (items_ir, t) = self.check(&args[2], None, scope);
                let element = self.list_element(&t, &args[2], "reduce");
                let widens = self.dialect == Dialect::JavaScript && self.resolve(&accumulator) == INT;
                let attempt = Speculation::start(self);
                let (mut function, mut result) = self.function_argument(
                    &args[0],
                    vec![accumulator.clone(), element.clone()],
                    Some(&accumulator),
                    "reduce",
                    scope,
                );
                if widens && self.diagnostics.len() > attempt.diagnostics {
                    attempt.undo(self);
                    let widened = self.coerce_joining(initial.clone(), &INT, &FLOAT, span);
                    let again = Speculation::start(self);
                    let (retried, retried_result) =
                        self.function_argument(&args[0], vec![FLOAT, element.clone()], Some(&FLOAT), "reduce", scope);
                    if self.diagnostics.len() > again.diagnostics {
                        again.undo(self);
                        let (reported, reported_result) = self.function_argument(
                            &args[0],
                            vec![accumulator.clone(), element],
                            Some(&accumulator),
                            "reduce",
                            scope,
                        );
                        function = reported;
                        result = reported_result;
                    } else {
                        initial = widened;
                        accumulator = FLOAT;
                        function = retried;
                        result = retried_result;
                    }
                }
                let _ = result;
                let function = lambda_node(function);
                self.coerce(prelude(prelude_op, vec![function, initial, items_ir], span), &accumulator, expected, span)
            }
            _ => self.higher_order(syntax, items, op, expected, scope),
        }
    }

    fn state_tuple_hint(&mut self, function: &Node, initial: &Node) -> Option<Type> {
        if self.dialect != Dialect::JavaScript {
            return None;
        }
        let state = list_items(form_items(function).filter(|_| is_function_form(function))?.get(1).and_then(list_items)?.first()?)?;
        let initial_items = list_items(initial)?;
        (state.len() > 1 && state.len() == initial_items.len()).then(|| Type::Tuple(state.iter().map(|_| self.fresh()).collect()))
    }

    fn keep_effects(&mut self, effects: Effects<'r>) {
        if let Some(outer) = self.effects.last_mut() {
            outer.calls.extend(effects.calls);
        }
    }

    fn form_concat<'a>(
        &mut self,
        op: PreludeOp,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let (first, t) = self.check(&args[0], None, scope);
        let element = self.list_element(&t, &args[0], "concat");
        let wanted = list(element);
        let before = Speculation::start(self);
        let ((second, _), effects) = self.collecting(|c| c.check(&args[1], Some(&wanted), scope));
        if self.diagnostics.len() == before.diagnostics {
            self.keep_effects(effects);
            return self.coerce(prelude(op, vec![first, second], span), &wanted, expected, span);
        }
        before.undo(self);
        if let Some((joined, effects)) = self.join_concat(&first, &wanted, args, scope) {
            self.keep_effects(effects);
            let (first, second) = joined.operands;
            return self.coerce(prelude(op, vec![first, second], span), &joined.list, expected, span);
        }
        let (second, _) = self.check(&args[1], Some(&wanted), scope);
        self.coerce(prelude(op, vec![first, second], span), &wanted, expected, span)
    }

    fn join_concat<'a>(
        &mut self,
        first: &ir::Node,
        wanted: &Type,
        args: &'a [Node],
        scope: &Scope<'a>,
    ) -> Option<(JoinedConcat, Effects<'r>)> {
        let before = Speculation::start(self);
        let ((second, second_type), effects) = self.collecting(|c| c.check(&args[1], None, scope));
        let found = self.zonk(&second_type);
        let held = self.zonk(wanted);
        let joined = match (&held, &found) {
            (Type::List(held_element), Type::List(found_element))
                if matches!((&**held_element, &**found_element), (Type::Record(_), Type::Record(_)))
                    && self.diagnostics.len() == before.diagnostics =>
            {
                self.least_upper_bound(&held, &found)
            }
            _ => None,
        };
        let Some(target) = joined else {
            before.undo(self);
            return None;
        };
        let first = self.coerce_joining(first.clone(), &held, &target, args[0].span);
        let second = self.coerce_joining(second, &found, &target, args[1].span);
        if self.diagnostics.len() != before.diagnostics {
            before.undo(self);
            return None;
        }
        Some((JoinedConcat { operands: (first, second), list: target }, effects))
    }

    fn number_operands<'a>(
        &mut self,
        op: &str,
        args: &'a [Node],
        scope: &Scope<'a>,
        checked: Vec<Checked>,
    ) -> (Vec<ir::Node>, Type) {
        let refs: Vec<&'a Node> = args.iter().collect();
        self.number_operands_of(op, &refs, scope, checked)
    }

    fn number_operands_of<'a>(
        &mut self,
        op: &str,
        args: &[&'a Node],
        scope: &Scope<'a>,
        checked: Vec<Checked>,
    ) -> (Vec<ir::Node>, Type) {
        let mut done = checked;
        for arg in &args[done.len()..] {
            let result = self.check(arg, None, scope);
            done.push(result);
        }
        let number = if done.iter().any(|(_, t)| self.resolve(t) == FLOAT) { FLOAT } else { INT };
        let mut operands = Vec::new();
        for (arg, (value, t)) in args.iter().zip(done) {
            if number == FLOAT && self.resolve(&t) == INT {
                self.report(
                    Diagnostic::new(
                        "type-mismatch",
                        arg.span,
                        format!("{op} takes two Ints or two Floats, but this is an Int and the other operand is a Float."),
                    )
                    .detail(format!("Expected: Float\nFound: Int\n{INT_TO_FLOAT_HINT}"))
                    .maybe_fix(float_fix(Some(arg)))
                    .help("float"),
                );
                operands.push(value);
                continue;
            }
            let options = Coercion { source: Some(arg), ..Coercion::default() };
            operands.push(self.coerce_with(value, &t, Some(&number), arg.span, options).0);
        }
        (operands, number)
    }

    fn is_map(&self, t: &Type) -> bool {
        matches!(self.resolve(t), Type::Map(_)) || self.is_brace(t)
    }

    fn looks_like_function(&self, syntax: &Node, scope: &Scope<'_>) -> bool {
        if is_function_form(syntax) {
            return true;
        }
        let Some(name) = symbol(syntax) else {
            return false;
        };
        match scope.get(name) {
            Some(Binding::Function(_)) => true,
            Some(Binding::Value(_)) => false,
            None => is_prelude(name) || self.registry.function(name).is_some(),
        }
    }

    fn higher_order<'a>(
        &mut self,
        syntax: &'a Node,
        items: &'a [Node],
        op: &str,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let span = syntax.span;
        let Some(kind) = higher(op) else {
            return self.placeholder(span);
        };
        let op_code = PreludeOp::HigherOrder(kind);
        let (function_node, items_node) = (&items[1], &items[2]);
        let (items_ir, items_type) = self.check(items_node, None, scope);
        let element = self.list_element(&items_type, items_node, op);
        let lambda = lambda_node;
        match kind {
            HigherOrder::Map => {
                let (function, result) = self.function_argument(function_node, vec![element], None, op, scope);
                self.coerce(prelude(op_code, vec![lambda(function), items_ir], span), &list(result), expected, span)
            }
            HigherOrder::Filter | HigherOrder::Any | HigherOrder::All => {
                let (function, _) = self.function_argument(function_node, vec![element.clone()], Some(&BOOL), op, scope);
                let t = if kind == HigherOrder::Filter { list(element) } else { BOOL };
                self.coerce(prelude(op_code, vec![lambda(function), items_ir], span), &t, expected, span)
            }
            HigherOrder::Keep => {
                let inner = self.fresh();
                let (function, _) =
                    self.function_argument(function_node, vec![element], Some(&option(inner.clone())), op, scope);
                self.coerce(prelude(op_code, vec![lambda(function), items_ir], span), &list(inner), expected, span)
            }
            HigherOrder::SortBy => {
                self.sort_key_owner = match form_items(function_node) {
                    Some(parts) if parts.len() == 3 => {
                        Some(SortKeyOwner { body: &parts[2] as *const Node, params: show(&parts[1]) })
                    }
                    _ => None,
                };
                let (mut function, key) = self.function_argument(function_node, vec![element.clone()], None, op, scope);
                self.sort_key_owner = None;
                if self.dialect == Dialect::JavaScript && self.resolve(&key) == STRING {
                    function = self.js_ordered_key(function);
                }
                if self.dialect == Dialect::JavaScript
                    && symbol(function_node) == Some("identity")
                    && !self.is_open(&key)
                    && self.resolve(&key) != STRING
                {
                    self.report(
                        Diagnostic::new(
                            "ts-sort-comparator",
                            function_node.span,
                            "sort() without a comparator orders values as text",
                        )
                        .detail(format!("The elements are {}, not Strings.", describe(&self.zonk(&key))))
                        .fix("(a, b) => k(a) - k(b)"),
                    );
                }
                if self.is_open(&key) {
                    self.late_sort_keys.push((key.clone(), function_node.span));
                }
                if !self.is_sort_key(&key) {
                    let mut hint = None;
                    if head_name(items_node) == Some("filter")
                        && let Some(filter) = form_items(items_node)
                        && filter.len() == 3
                        && show(&filter[1]).contains("none")
                    {
                        hint = Some(format!(
                            "To drop none values and unwrap the rest, use (keep (fn [x] x) {}).",
                            show(&filter[2])
                        ));
                    }
                    let mut detail = self.sort_key_detail(&key);
                    if let Some(hint) = &hint {
                        detail.push('\n');
                        detail.push_str(hint);
                    }
                    self.report(
                        Diagnostic::new(
                            "type-mismatch",
                            function_node.span,
                            "sort-by needs a key that is an Int, Float, String, Bool, or a tuple of them.",
                        )
                        .detail(detail)
                        .help(if hint.is_some() { "keep" } else { "sort-by" }),
                    );
                }
                self.coerce(prelude(op_code, vec![lambda(function), items_ir], span), &list(element), expected, span)
            }
            HigherOrder::DistinctBy => {
                let (function, _) = self.function_argument(function_node, vec![element.clone()], None, op, scope);
                self.coerce(prelude(op_code, vec![lambda(function), items_ir], span), &list(element), expected, span)
            }
            HigherOrder::GroupBy => {
                let (function, key) = self.function_argument(function_node, vec![element.clone()], None, op, scope);
                let t = list(Type::Tuple(vec![key, list(element)]));
                self.coerce(prelude(op_code, vec![lambda(function), items_ir], span), &t, expected, span)
            }
            HigherOrder::Reduce => self.placeholder(span),
        }
    }

    fn prelude_str<'a>(&mut self, syntax: &'a Node, items: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let span = syntax.span;
        let hint = match &self.sort_key_owner {
            Some(owner) if std::ptr::eq(owner.body, syntax) => {
                let parts: Vec<String> =
                    items[1..].iter().filter(|arg| !matches!(arg.kind, NodeKind::String(_))).map(show).collect();
                Some(format!("To sort by several keys, return a tuple: (fn {} [{}])", owner.params, parts.join(" ")))
            }
            _ => None,
        };
        let mut checked = Vec::new();
        for arg in &items[1..] {
            let (value, t) = self.check(arg, None, scope);
            let resolved = self.resolve(&t);
            let plain = matches!(resolved, Type::Primitive(Primitive::Int | Primitive::Float | Primitive::Bool))
                || matches!(&resolved, Type::Enum(e) if Checker::nullary(e));
            if hint.is_none() && plain {
                checked.push(value);
                continue;
            }
            let found = describe(&self.zonk(&t));
            let message = if hint.is_some() {
                format!("A sort key built with str joins strings only, but this argument is {found}.")
            } else {
                format!("str joins strings, integers, floats, booleans, and enum values, but this is {found}.")
            };
            let options = Coercion {
                message: Some(message),
                detail_hint: hint.clone(),
                help: Some(if hint.is_some() { "sort-by" } else { "str" }.to_string()),
                ..Coercion::default()
            };
            checked.push(self.coerce_with(value, &t, Some(&STRING), arg.span, options).0);
        }
        self.coerce(prelude(PreludeOp::Str, checked, span), &STRING, expected, span)
    }

    fn check_size(&mut self, program: &Node) {
        let mut nodes: u64 = 0;
        let mut deepest: u64 = 0;
        let mut stack: Vec<(&Node, u64)> = vec![(program, 1)];
        while let Some((at, depth)) = stack.pop() {
            nodes += 1;
            deepest = deepest.max(depth);
            match &at.kind {
                NodeKind::Form(items) | NodeKind::List(items) => stack.extend(items.iter().map(|c| (c, depth + 1))),
                NodeKind::Record(fields) => stack.extend(fields.iter().map(|f| (&f.value, depth + 1))),
                _ => {}
            }
        }
        if nodes > self.budgets.nodes {
            self.report(Diagnostic::new(
                "too-large",
                program.span,
                format!("The program has {nodes} syntax nodes, more than the nodes budget of {}.", self.budgets.nodes),
            ));
        }
        if deepest > self.budgets.depth {
            self.report(Diagnostic::new(
                "too-large",
                program.span,
                format!("The program nests {deepest} levels deep, more than the depth budget of {}.", self.budgets.depth),
            ));
        }
    }
}

impl Checker<'_> {
    fn settle_type(&self, t: &mut Type) {
        *t = self.zonk(t);
        if type_depth(t) > DEPTH_LIMIT {
            self.overflowed.set(true);
        }
    }

    fn settle_conversion(&self, conversion: &mut Conversion) {
        match conversion {
            Conversion::ToSome(inner) => {
                if let Some(inner) = inner {
                    self.settle_conversion(inner);
                }
            }
            Conversion::EachField(fields) => {
                for (_, c) in fields.iter_mut().filter_map(|(n, c)| c.as_mut().map(|c| (n, c))) {
                    self.settle_conversion(c);
                }
            }
            Conversion::WidenRecord(fields) => {
                for field in fields {
                    if let Some(c) = &mut field.conversion {
                        self.settle_conversion(c);
                    }
                    self.settle_type(&mut field.field_type);
                }
            }
            Conversion::EachItem(items) => {
                for c in items.iter_mut().flatten() {
                    self.settle_conversion(c);
                }
            }
            Conversion::EachElement(element) => self.settle_conversion(element),
            Conversion::VariantName => {}
        }
    }

    fn settle_lambda(&self, lambda: &mut Lambda, depth: usize) {
        for t in &mut lambda.param_types {
            self.settle_type(t);
        }
        self.settle_type(&mut lambda.result);
        self.settle_at(&mut lambda.body, depth + 1);
    }

    fn settle_all(&self, nodes: &mut [ir::Node], depth: usize) {
        for n in nodes {
            self.settle_at(n, depth);
        }
    }

    fn settle(&self, at: &mut ir::Node) {
        self.settle_at(at, 1);
    }

    fn settle_at(&self, at: &mut ir::Node, depth: usize) {
        if depth > DEPTH_LIMIT {
            self.overflowed.set(true);
            return;
        }
        self.settle_kind(at, depth);
    }

    fn settle_kind(&self, at: &mut ir::Node, depth: usize) {
        let replacement = match &mut at.kind {
            Ir::MakeEmptyBrace { shape } => Some(match self.zonk(shape) {
                shape @ Type::Map(_) => Ir::Literal { value: Value::Map(MapValue::from_pairs([])), value_type: shape },
                _ => Ir::MakeRecord { fields: Vec::new() },
            }),
            Ir::MakeBracket { items, shape } => {
                self.settle_all(items, depth + 1);
                let items = std::mem::take(items);
                Some(match self.zonk(shape) {
                    Type::List(element) => Ir::MakeList { items, element: *element },
                    _ => Ir::MakeTuple { items },
                })
            }
            Ir::Literal { value_type, .. } => {
                self.settle_type(value_type);
                None
            }
            Ir::Variable { .. } | Ir::Sleep { .. } => None,
            Ir::Let(ir::Let { value, body, .. }) => {
                self.settle_at(value, depth + 1);
                self.settle_at(body, depth + 1);
                None
            }
            Ir::LocalFunction { body, .. } | Ir::Attempt { body } | Ir::Fail { message: body } => {
                self.settle_at(body, depth + 1);
                None
            }
            Ir::If(ir::If { condition, then, otherwise }) => {
                self.settle_at(condition, depth + 1);
                self.settle_at(then, depth + 1);
                self.settle_at(otherwise, depth + 1);
                None
            }
            Ir::Match(ir::Match { scrutinee, clauses }) => {
                self.settle_at(scrutinee, depth + 1);
                for (_, body) in clauses {
                    self.settle_at(body, depth + 1);
                }
                None
            }
            Ir::Lambda(lambda) => {
                self.settle_lambda(lambda, depth);
                None
            }
            Ir::ApplyLocal { function, args, .. } => {
                self.settle_lambda(function, depth + 1);
                self.settle_all(args, depth + 1);
                None
            }
            Ir::Prelude { args, .. } | Ir::CoreCall { args, .. } | Ir::Recur { args } | Ir::Log { args } => {
                self.settle_all(args, depth + 1);
                None
            }
            Ir::Call { argument, .. } => {
                self.settle_at(argument, depth + 1);
                None
            }
            Ir::Get { record, .. } => {
                self.settle_at(record, depth + 1);
                None
            }
            Ir::MakeRecord { fields } => {
                for (_, value) in fields {
                    self.settle_at(value, depth + 1);
                }
                None
            }
            Ir::MakeList { items, element } => {
                self.settle_all(items, depth + 1);
                self.settle_type(element);
                None
            }
            Ir::MakeTuple { items } | Ir::Par { branches: items } => {
                self.settle_all(items, depth + 1);
                None
            }
            Ir::Wrap { value, wrapped_type, .. } => {
                self.settle_at(value, depth + 1);
                self.settle_type(wrapped_type);
                None
            }
            Ir::MakeVariant { payload, .. } => {
                if let Some(payload) = payload {
                    self.settle_at(payload, depth + 1);
                }
                None
            }
            Ir::Coerce { value, conversion } => {
                self.settle_at(value, depth + 1);
                self.settle_conversion(conversion);
                None
            }
            Ir::Retry(retry) => {
                self.settle_at(&mut retry.body, depth + 1);
                None
            }
            Ir::WaitUntil(wait) => {
                self.settle_lambda(&mut wait.predicate, depth + 1);
                self.settle_at(&mut wait.body, depth + 1);
                None
            }
            Ir::Loop(repeat) => {
                self.settle_all(&mut repeat.initial, depth + 1);
                self.settle_at(&mut repeat.body, depth + 1);
                None
            }
            Ir::ParMap { function, items, .. } => {
                self.settle_lambda(function, depth + 1);
                self.settle_at(items, depth + 1);
                None
            }
        };
        if let Some(kind) = replacement {
            at.kind = kind;
        }
    }
}

const NESTING_LIMIT: usize = 500;
const DEPTH_LIMIT: usize = 1000;

pub fn check(
    program: &Node,
    registry: &Registry,
    authority: Authority<'_>,
    inputs: &BTreeMap<String, Type>,
    budgets: Budgets,
    dialect: Dialect,
) -> CheckResult {
    let setting = Setting { registry, authority, inputs, budgets, dialect };
    let style = crate::types::current_style();
    let checked = crate::stack::on_sized_stack(|| {
        let _style = crate::types::use_style(style);
        let mut arguments = ArgumentTypes::default();
        let first = elaborate(program, &setting, ArgumentTypes::default(), &mut arguments);
        let unresolved = first.diagnostics.iter().any(|d| d.code == "unresolved-type");
        if !unresolved || arguments.implicated.is_empty() {
            return first;
        }
        elaborate(program, &setting, arguments, &mut ArgumentTypes::default())
    });
    match checked {
        Ok(result) => result,
        Err(error) => {
            let message = format!("The checker could not start a thread for this program: {error}.");
            CheckResult::rejected(vec![Diagnostic::new("too-large", program.span, message)])
        }
    }
}

struct Setting<'s> {
    registry: &'s Registry,
    authority: Authority<'s>,
    inputs: &'s BTreeMap<String, Type>,
    budgets: Budgets,
    dialect: Dialect,
}

fn elaborate(program: &Node, setting: &Setting<'_>, earlier: ArgumentTypes, found: &mut ArgumentTypes) -> CheckResult {
    let Setting { registry, authority, inputs, budgets, dialect } = *setting;
    let mut checker = Checker {
        dialect,
        registry,
        authority,
        budgets,
        substitution: Vec::new(),
        trail: Vec::new(),
        shapes: HashMap::new(),
        presence_tests: Vec::new(),
        late_sort_keys: Vec::new(),
        late_json_values: Vec::new(),
        record_literals: Vec::new(),
        unfilled_loops: 0,
        earlier,
        arguments: ArgumentTypes::default(),
        open_arguments: Vec::new(),
        diagnostics: Vec::new(),
        effects: Vec::new(),
        capabilities_used: BTreeSet::new(),
        functions_used: BTreeSet::new(),
        inputs_used: BTreeSet::new(),
        tail: None,
        instantiating: Vec::new(),
        instantiations: 0,
        sort_key_owner: None,
        repeat_depth: 0,
        nesting: 0,
        overflowed: std::cell::Cell::new(false),
    };
    let mut declared: BTreeMap<String, Type> = inputs.clone();
    let mut header_inputs = None;
    let mut body_node = program;
    let mut unusable: Vec<String> = Vec::new();
    if script::is_script(program) {
        let read = script::read(program, registry);
        checker.diagnostics.extend(read.diagnostics);
        let Some(body) = read.body else {
            return CheckResult::rejected(checker.diagnostics);
        };
        body_node = body;
        declared = read.inputs.iter().cloned().collect();
        header_inputs = Some(declared.clone());
        unusable = read.unusable;
    }
    let mut scope = Scope::default();
    for (name, t) in &declared {
        scope = scope.value(name, t.clone());
    }
    for name in &unusable {
        scope = scope.value(name, Type::Never);
    }
    let (mut body, mut result) = checker.check(body_node, None, &scope);
    drop(scope);
    if dialect == Dialect::JavaScript {
        let resolved = checker.zonk(&result);
        if let Some(shaped) = checker.settled_shape(body.clone(), &resolved, program.span) {
            (body, result) = shaped;
        }
    }
    *found = std::mem::take(&mut checker.arguments);
    checker.check_presence_tests();
    checker.check_late_sort_keys();
    checker.check_late_json_values();
    checker.check_size(program);
    checker.settle(&mut body);
    let result = checker.zonk(&result);
    if checker.overflowed.get() || type_depth(&result) > DEPTH_LIMIT {
        let mut diagnostics: Vec<Diagnostic> =
            checker.diagnostics.into_iter().filter(|d| d.code == "too-large" && d.span == program.span).collect();
        if diagnostics.is_empty() {
            let message = format!(
                "The program nests more than {NESTING_LIMIT} forms or builds a value more than {DEPTH_LIMIT} levels deep, \
                 more than the checker follows."
            );
            diagnostics.push(Diagnostic::new("too-large", program.span, message));
        }
        return CheckResult::rejected(diagnostics);
    }
    if let Some(problem) = encodable_problem(&result) {
        let reason = problem.split_once(" has no JSON encoding").map_or("", |(_, reason)| reason);
        let message = format!("The program's result is {}, which has no JSON encoding{reason}.", describe(&result));
        checker.report(Diagnostic::new("no-encoding", program.span, message));
    }
    let mut seen: HashSet<(&'static str, Span, String)> = HashSet::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for diagnostic in std::mem::take(&mut checker.diagnostics) {
        if seen.insert((diagnostic.code, diagnostic.span, diagnostic.message.clone())) {
            diagnostics.push(diagnostic);
        }
    }
    diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.column));
    if !diagnostics.is_empty() {
        return CheckResult::rejected(diagnostics);
    }
    let used_inputs: BTreeMap<String, Type> = declared
        .iter()
        .filter(|(name, _)| checker.inputs_used.contains(*name))
        .map(|(name, t)| (name.clone(), t.clone()))
        .collect();
    let signatures = ir::Signatures {
        capabilities: checker
            .capabilities_used
            .iter()
            .filter_map(|id| registry.capability(id))
            .map(|c| {
                let signature = ir::CapabilitySignature {
                    effect: c.effect,
                    idempotent: c.idempotent,
                    input: c.input.clone(),
                    output: c.output.clone(),
                };
                (c.id.clone(), signature)
            })
            .collect(),
        functions: checker
            .functions_used
            .iter()
            .filter_map(|name| registry.function(name))
            .map(|f| (f.name.clone(), ir::FunctionSignature { params: f.params.clone(), output: f.output.clone() }))
            .collect(),
    };
    let mut program = ir::Program {
        body,
        result,
        capabilities: checker.capabilities_used.iter().cloned().collect(),
        inputs: used_inputs,
        enums: BTreeMap::new(),
        signatures,
    };
    program.enums = crate::export::referenced_enums(&program);
    CheckResult { program: Some(program), diagnostics: Vec::new(), header_inputs }
}

pub fn compile_source(
    source: &str,
    registry: &Registry,
    authority: Authority<'_>,
    inputs: &BTreeMap<String, Type>,
    budgets: Budgets,
    dialect: Dialect,
) -> CheckResult {
    let size = source.len() as u64;
    if size > budgets.source_bytes {
        let start = crate::syntax::Position { line: 1, column: 1 };
        let message =
            format!("The program is {size} bytes, more than the source-bytes budget of {}.", budgets.source_bytes);
        return CheckResult::rejected(vec![Diagnostic::new("too-large", Span::new(start, start), message)]);
    }
    let read = crate::reader::read(source);
    match read.program {
        None => CheckResult::rejected(read.diagnostics),
        Some(program) => check(&program, registry, authority, inputs, budgets, dialect),
    }
}
