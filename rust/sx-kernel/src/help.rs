use std::borrow::Cow;

use crate::json::{self, Json};
use crate::registry::{Capability, Core, Effect, Function, Registry, RegistryError};
use crate::types::{CallErrorKind, Type, show_definition, show_type};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: &'static str,
    pub signature: &'static str,
    pub description: Cow<'static, str>,
    pub example: &'static str,
    pub result: &'static str,
}

impl Entry {
    pub const fn new(
        name: &'static str,
        signature: &'static str,
        description: &'static str,
        example: &'static str,
        result: &'static str,
    ) -> Entry {
        Entry { name, signature, description: Cow::Borrowed(description), example, result }
    }
}

const fn entry(
    name: &'static str,
    signature: &'static str,
    description: &'static str,
    example: &'static str,
    result: &'static str,
) -> Entry {
    Entry::new(name, signature, description, example, result)
}

pub static FORMS: [Entry; 19] = [
    entry("let", "(let [name value ...] body)", "Binds names, each visible to later bindings and the body. A fn value binds a local function, and _ runs a value for its effect. A form name such as attempt cannot be a binding name: write attempts.", "(let [a 1 b (+ a 1) double (fn [x] (* 2 x))] (double b))", "4"),
    entry("if", "(if condition then else)", "Chooses a branch. The condition is true or false, or an optional, which tests that a value is present. When the condition is an optional name, the then-branch sees the value inside it.", "(let [n (some 3)] (if n (+ n 1) 0))", "4"),
    entry("cond", "(cond test result ... :else result)", "Returns the result of the first true test, or the :else result when none is true. :else is required and last. A test may be an optional, as in if.", "(cond (> 1 2) \"big\" (> 3 2) \"medium\" :else \"small\")", "\"medium\""),
    entry("match", "(match value pattern result ...)", "Chooses the first clause whose pattern matches. Patterns: (some x) none (ok x) (err e) Variant (Variant x) _, or a name that binds the whole value like _. An integer, string, or boolean literal matches an equal value; true and false together cover Bool, and other literals need a final _.", "[(match (some 3) (some n) (+ n 1) none 0) (match 2 1 \"one\" 2 \"two\" _ \"many\")]", "[4 \"two\"]"),
    entry("fn", "(fn [param ...] body)", "A function, as an argument to map, filter, and the like, or as the value of a let binding.", "(map (fn [x] (+ x 1)) [1 2])", "[2 3]"),
    entry("get", "(get record :field [default]) or (get map \"key\" [default])", "Reads a record field, or a map entry by its string key. A map entry is optional unless you give a default. With a default, a record field must be optional, and the default replaces none.", "(let [stock (assoc {} \"pen\" 3)] [(get {:nick none} :nick \"anon\") (get stock \"pen\") (get stock \"ink\" 0)])", "[\"anon\" (some 3) 0]"),
    entry("call", "(call tool.name {:field value})", "Calls a tool with an input record.", "(count (call people.list {:team \"infra\"}))", "3"),
    entry("attempt", "(attempt expression)", "(ok value), or (err {:kind K :message m}) if a tool call failed or a core refused its input.", "(attempt (call metrics.uptime {:host \"gone\"}))", "(err {:kind NotFound :message \"no such host\"})"),
    entry("retry", "(retry {:delays [1s 2s]} expression)", "Evaluates the expression again after each delay while it fails with a transient kind: Timeout, Unavailable, or RateLimited. :on [Kind ...] chooses the kinds, and {:backoff [100ms 5s] :within 30s} computes doubling delays. A write that is not idempotent may be retried only when :on names refusal kinds alone: RateLimited, NotFound, PermissionDenied, Invalid, Conflict.", "(let [up (retry {:delays [1s 2s]} (call metrics.uptime {:host \"flaky\"})) _ (retry {:delays [1s] :on [RateLimited]} (call chat.post {:text \"hi\"}))] (get up :seconds))", "60"),
    entry("wait-until", "(wait-until {:within 2m :every 5s} (fn [v] done?) expression)", "Evaluates the expression until the function returns true for its value, and returns that value. It fails with kind Timeout after :within.", "(wait-until {:within 1m :every 5s} (fn [s] (= s \"done\")) (call job.state {:id \"j1\"}))", "\"done\""),
    entry("sleep", "(sleep 10s)", "Waits the literal duration and returns Unit, as one delay request. Bind it to _ to run it as a step. A duration of 0ms waits for nothing.", "(let [_ (sleep 10s)] \"rested\")", "\"rested\""),
    entry("loop", "(loop {:max 20} [name value ...] body)", "Binds like let and evaluates the body. (recur value ...) as its last step runs the body again with new values, at most :max times.", "(loop {:max 10} [n 0 total 0] (if (= n 4) total (recur (+ n 1) (+ total n))))", "6"),
    entry("recur", "(recur value ...)", "Runs the enclosing loop body again with new values for its names. It must be the last step of the body.", "(loop {:max 5} [n 3] (if (= n 0) \"liftoff\" (recur (- n 1))))", "\"liftoff\""),
    entry("par", "(par expression ...)", "Evaluates tool reads in parallel and returns a tuple in source order.", "(let [[a b] (par (call metrics.uptime {:host \"db1\"}) (call metrics.uptime {:host \"db2\"}))] (+ (get a :seconds) (get b :seconds)))", "300"),
    entry("par-map", "(par-map {:limit 8} (fn [x] ...) xs)", "Maps over a list in parallel, at most :limit at a time, with reads only. Results keep list order.", "(par-map {:limit 2} (fn [h] (get (call metrics.uptime {:host h}) :seconds)) [\"db1\" \"db2\"])", "[100 200]"),
    entry("some", "(some value)", "An optional value that is present.", "(some 1)", "(some 1)"),
    entry("none", "none", "An optional value that is absent. Sx has no nil.", "(match none (some x) x none 0)", "0"),
    entry("ok", "(ok value)", "A successful result.", "(match (ok 1) (ok x) x (err e) 0)", "1"),
    entry("err", "(err value)", "A failed result.", "(match (err \"bad\") (ok x) \"fine\" (err e) e)", "\"bad\""),
];

pub static PRELUDE: [Entry; 44] = [
    entry("+", "(+ a b) -> Int or Float", "Adds two Ints or two Floats, never one of each. Overflow stops the program.", "(+ 2 3)", "5"),
    entry("-", "(- a b) -> Int or Float, (- x)", "Subtracts, or negates one number. Two Ints or two Floats.", "(- 10 4)", "6"),
    entry("*", "(* a b) -> Int or Float", "Multiplies two Ints or two Floats.", "(* 6 7)", "42"),
    entry("/", "(/ a b) -> Float", "Divides two Floats. Dividing by zero stops the program, because JSON has no infinity. Int division is quot.", "(/ 1.0 4.0)", "0.25"),
    entry("quot", "(quot a b) -> Int", "Divides, truncating toward zero. Division by zero stops the program.", "(quot -7 2)", "-3"),
    entry("rem", "(rem a b) -> Int", "The remainder of quot.", "(rem -7 2)", "-1"),
    entry("float", "(float n) -> Float", "The Float nearest an Int. Sx never converts between Int and Float on its own, so mixing them in arithmetic, a comparison, or = is rejected.", "(+ (float 2) 0.5)", "2.5"),
    entry("round", "(round x) -> Int", "The nearest Int, with halves going away from zero. Outside the Int range stops the program.", "[(round 2.5) (round -2.5) (round 2.4)]", "[3 -3 2]"),
    entry("floor", "(floor x) -> Int", "The largest Int not above x. Outside the Int range stops the program.", "[(floor 2.7) (floor -2.1)]", "[2 -3]"),
    entry("ceil", "(ceil x) -> Int", "The smallest Int not below x. Outside the Int range stops the program.", "[(ceil 2.1) (ceil -2.7)]", "[3 -2]"),
    entry("=", "(= a b) -> Bool", "True when two values of one type are equal.", "(= [1 2] [1 2])", "true"),
    entry("not=", "(not= a b) -> Bool", "True when two values differ.", "(not= \"a\" \"b\")", "true"),
    entry("<", "(< a b) -> Bool", "Compares two Ints, two Floats, or two strings by code point.", "(< 1 2)", "true"),
    entry("<=", "(<= a b) -> Bool", "Compares Ints, Floats, or strings.", "(<= 2 2)", "true"),
    entry(">", "(> a b) -> Bool", "Compares Ints, Floats, or strings.", "(> \"b\" \"a\")", "true"),
    entry(">=", "(>= a b) -> Bool", "Compares Ints, Floats, or strings.", "(>= 1.5 2.5)", "false"),
    entry("not", "(not b) -> Bool", "Negates a boolean.", "(not false)", "true"),
    entry("and", "(and a b) -> Bool", "True when both are true. Both arguments always evaluate.", "(and true false)", "false"),
    entry("or", "(or a b) -> Bool", "True when either is true. Both arguments always evaluate.", "(or true false)", "true"),
    entry("str", "(str x ...) -> String", "Joins strings, integers, floats, booleans, and enum values. A Float is written as JavaScript writes it: 1.0 is 1.", "(str \"a\" 1 true 2.50)", "\"a1true2.5\""),
    entry("count", "(count xs) -> Int", "The number of elements, of map entries, or of bytes.", "(count [1 2 3])", "3"),
    entry("first", "(first xs) -> T", "The first element. An empty list stops the program.", "(first [1 2 3])", "1"),
    entry("rest", "(rest xs) -> List<T>", "Every element after the first.", "(rest [1 2 3])", "[2 3]"),
    entry("nth", "(nth xs i) -> T", "The element at index i, from 0. A missing index stops the program.", "(nth [10 20 30] 1)", "20"),
    entry("take", "(take n xs) -> List<T>", "The first n elements.", "(take 2 [1 2 3])", "[1 2]"),
    entry("concat", "(concat xs ys) -> List<T>", "Joins two lists.", "(concat [1] [2 3])", "[1 2 3]"),
    entry("flatten", "(flatten xss) -> List<T>", "Joins a list of lists.", "(flatten [[1] [2 3]])", "[1 2 3]"),
    entry("contains?", "(contains? xs x) -> Bool", "True when the list holds the value, or the map has the key.", "(contains? [1 2] 2)", "true"),
    entry("assoc", "(assoc m key value) -> Map<String, V>", "A map with the key set to the value, replacing any earlier value. Start from {}. Maps list in key order.", "(assoc (assoc {} \"b\" 2) \"a\" 1)", "(into-map [[\"a\" 1] [\"b\" 2]])"),
    entry("dissoc", "(dissoc m key) -> Map<String, V>", "A map without the key. A key that is absent is fine.", "(dissoc (assoc (assoc {} \"a\" 1) \"b\" 2) \"a\")", "(into-map [[\"b\" 2]])"),
    entry("keys", "(keys m) -> List<String>", "The keys of a map in ascending order.", "(keys (assoc (assoc {} \"b\" 2) \"a\" 1))", "[\"a\" \"b\"]"),
    entry("vals", "(vals m) -> List<V>", "The values of a map, in key order.", "(vals (assoc (assoc {} \"b\" 2) \"a\" 1))", "[1 2]"),
    entry("entries", "(entries m) -> List<Tuple<String, V>>", "The [key value] pairs of a map, in key order.", "(entries (assoc (assoc {} \"b\" 2) \"a\" 1))", "[[\"a\" 1] [\"b\" 2]]"),
    entry("into-map", "(into-map pairs) -> Map<String, V>", "Builds a map from [key value] pairs. A later pair replaces an earlier pair with the same key. It takes the output of group-by when the key is a string.", "(into-map (group-by (fn [x] (if (> x 1) \"big\" \"small\")) [1 2 3]))", "(into-map [[\"big\" [2 3]] [\"small\" [1]]])"),
    entry("identity", "(identity x) -> T", "Returns its argument. Pass it as a function, for example as the key of group-by.", "(group-by identity [1 2 1])", "[[1 [1 1]] [2 [2]]]"),
    entry("map", "(map f xs) -> List<U>", "Applies f to each element, in order.", "(map (fn [x] (* x x)) [1 2 3])", "[1 4 9]"),
    entry("filter", "(filter f xs) -> List<T>", "Keeps the elements for which f returns true.", "(filter (fn [x] (> x 1)) [1 2 3])", "[2 3]"),
    entry("keep", "(keep f xs) -> List<U>", "Applies f, which returns an optional, and keeps the some values unwrapped.", "(keep (fn [x] (if (> x 1) x none)) [1 2 3])", "[2 3]"),
    entry("any?", "(any? f xs) -> Bool", "True when f returns true for some element. Stops at the first.", "(any? (fn [x] (> x 2)) [1 2 3])", "true"),
    entry("all?", "(all? f xs) -> Bool", "True when f returns true for every element. Stops at the first false.", "(all? (fn [x] (> x 2)) [1 2 3])", "false"),
    entry("reduce", "(reduce f init xs) -> A", "Combines the elements into one value, left to right.", "(reduce + 0 [1 2 3])", "6"),
    entry("sort-by", "(sort-by f xs) -> List<T>", "Sorts ascending by a key: an integer, a float, a string, a boolean, or a tuple of keys compared in order. Negate an integer key to sort descending.", "(sort-by (fn [p] [(- (get p :n)) (get p :name)]) [{:name \"b\" :n 1} {:name \"a\" :n 2}])", "[{:name \"a\" :n 2} {:name \"b\" :n 1}]"),
    entry("distinct-by", "(distinct-by f xs) -> List<T>", "Keeps the first element for each key.", "(distinct-by (fn [x] (rem x 2)) [1 2 3 4])", "[1 2]"),
    entry("group-by", "(group-by f xs) -> List<Tuple<K, List<T>>>", "Groups elements by key, as [key items] tuples in first-seen order.", "(group-by (fn [x] (rem x 2)) [1 2 3])", "[[1 [1 3]] [0 [2]]]"),
];

pub static EXTENSIONS: [Entry; 13] = [entry(
    "script",
    "(script {:inputs {:name \"Type\" ...}} body)",
    "The outermost form of a program that declares its own typed inputs, so it needs no host to declare them. Each type is a string in the declaration type language, such as \"List<String>\" or \"Option<Int>\", and the body reads each input by name. A header declares no authority. An Option input may be left out when running and is then none. Run this example with: sx run report.sx --input 'owners=[\"acme\",\"beta\",\"cyan\"]' --input limit=2",
    "(script {:inputs {:owners \"List<String>\" :limit \"Int\"}} (take limit owners))",
    "[\"acme\" \"beta\"]",
), entry(
    "def",
    "(def name value)",
    "A top-level definition, before the program's final expression. It binds the value for every later form, like one binding of an outer let, and (def _ value) runs a step for its effect. A definition is allowed only at the top level.",
    "(def base 10) (def scaled (* base 3)) (+ scaled 1)",
    "31",
), entry(
    "defn",
    "(defn name [param ...] body)",
    "A top-level function definition, before the program's final expression. Later forms call it like a let-bound fn. It cannot call itself, and it takes no docstring.",
    "(defn double [x] (* 2 x)) (map double [1 2 3])",
    "[2 4 6]",
), entry(
    "when",
    "(when condition expression)",
    "(some value) of the expression when the condition holds, and none otherwise, without evaluating it. The condition follows the rules of if. Use it for a step that runs only sometimes, such as (def _ (when failed (call ...))).",
    "[(when (> 3 1) \"big\") (when (> 1 3) \"big\")]",
    "[(some \"big\") none]",
), entry(
    "do",
    "(do expression ... last)",
    "Evaluates each expression in order for its effect and returns the value of the last, like (let [_ e1 _ e2] last).",
    "(do (+ 1 1) (* 2 3))",
    "6",
), entry(
    "to-json",
    "(to-json x) -> String",
    "Writes a value as compact JSON text, with the encoding a program's result has: Bytes as base64, an enum value without a payload as its variant name, none as null. Record fields and map keys come in ascending order, so equal values give equal text. A Float is written as str writes it, so 2.0 is 2. A type with no JSON encoding, such as an Option of an Option, is a check error. Use it with fs.write-text, or to send JSON text to a tool.",
    "(to-json {:name \"ada\" :langs [\"sx\" \"lean\"]})",
    "\"{\\\"langs\\\":[\\\"sx\\\",\\\"lean\\\"],\\\"name\\\":\\\"ada\\\"}\"",
), entry(
    "fail",
    "(fail message)",
    "Stops the program with the failure class thrown and the message. attempt catches it as an error of kind Failed with that message, so a failure raised inside attempt is handled like a failed call. Given a caught error instead of a String, it raises that same error again, keeping its kind and message. It has any type, so it fits where a value is needed. retry repeats it only when :on names Failed.",
    "(attempt (fail \"boom\"))",
    "(err {:kind Failed :message \"boom\"})",
), entry(
    "log",
    "(log value ...)",
    "Appends one line to the run's logs and returns Unit. The line is each value written and joined by single spaces: a String as itself, an Int, Float, Bool, or enum value as str writes it, and anything else as to-json writes it, so none and unit are null. The run result lists the lines in order under logs, and omits logs when there are none. Bind it to _ to run it as a step.",
    "(let [_ (log \"total\" (+ 1 2) [1 2])] 3)",
    "3",
), entry(
    "unit",
    "unit",
    "The Unit value, for a program or branch with no result. Tools that return nothing return it too.",
    "(let [_ (log \"done\")] unit)",
    "unit",
), entry(
    "put",
    "(put target key value) -> same type",
    "A record with one field replaced, or a Map with a key set. On a record the key is a string literal naming a field the record has, and the value must have that field's type. The record keeps its type and field order. On a Map it is assoc. A record a tool declares open cannot be put into.",
    "[(put {:name \"ada\" :age 36} \"age\" 37) (put (assoc {} \"a\" 1) \"b\" 2)]",
    "[{:name \"ada\" :age 37} (into-map [[\"a\" 1] [\"b\" 2]])]",
), entry(
    "range",
    "(range start end [step]) -> List<Int>",
    "The Ints from start up to but not including end, stepping by 1. A step is a nonzero integer literal; a negative one counts down and stops before end. An empty range is []. A range longer than the value-size budget stops the program.",
    "[(range 1 4) (range 0 10 3) (range 5 0 -2)]",
    "[[1 2 3] [0 3 6 9] [5 3 1]]",
), entry(
    "enumerate",
    "(enumerate xs) -> List<Tuple<Int, T>>",
    "Pairs each element with its index, from 0.",
    "(enumerate [\"a\" \"b\"])",
    "[[0 \"a\"] [1 \"b\"]]",
), entry(
    "safe-int",
    "(safe-int n) -> Int",
    "The Int itself when it lies within plus or minus 2^53, the Ints that a double holds exactly. Outside that range stops the program with the failure class arithmetic, which attempt does not catch.",
    "[(safe-int 9007199254740992) (safe-int -5)]",
    "[9007199254740992 -5]",
)];

pub fn types() -> Vec<Entry> {
    let kinds = CallErrorKind::ALL.map(CallErrorKind::name).join(", ");
    vec![
        Entry::new("Option", "Option<T>: (some x) or none", "An optional value. Wherever an optional is expected, a plain value also works and becomes (some value).", "(map (fn [x] (if (> x 1) x none)) [1 2])", "[none (some 2)]"),
        Entry::new("Result", "Result<T, CallError>: (ok x) or (err e)", "What attempt returns.", "(match (attempt (call metrics.uptime {:host \"db1\"})) (ok m) (get m :seconds) (err e) 0)", "100"),
        Entry { name: "CallError", signature: "Record {kind: CallErrorKind, message: String}", description: format!("Why a tool call failed. Kinds: {kinds}. Branch on the kind, never the message.").into(), example: "(match (attempt (call metrics.uptime {:host \"gone\"})) (ok m) \"up\" (err e) (match (get e :kind) NotFound \"missing\" _ \"other\"))", result: "\"missing\"" },
        Entry::new("Float", "Float: 1.5, -0.25, 2e3", "A 64-bit IEEE float. A literal has a fraction or an exponent: 1 is an Int. A JSON number decodes as a Float where a Float is expected. A result that is infinite or not a number stops the program.", "(* 1.5 (float 2))", "3.0"),
        Entry::new("Bytes", "Bytes", "Opaque binary data from a tool. Only count and = take it; there is no literal. JSON carries it as standard base64 with padding, and a string that is not that is an invalid host response.", "[(count (call blob.read {:name \"greeting\"})) (= (call blob.read {:name \"greeting\"}) (call blob.read {:name \"again\"}))]", "[5 true]"),
        Entry::new("Map", "Map<String, V>: (into-map [[key value] ...])", "A JSON object with string keys and values of one type V, listed in key order. Keys are strings, never keywords: keywords only name record fields.", "(get (assoc {} \"pen\" 3) \"pen\" 0)", "3"),
        Entry::new("Record", "Record {k: T, ...}: {:k value ...}", "A group of named fields. Branches that return records with different fields join to the union of the fields; a field only some branches write is optional, and none in the others.", "(map (fn [n] (if (> n 1) {:n n :big true} {:n n})) [1 2])", "[{:n 1 :big none} {:n 2 :big (some true)}]"),
        Entry::new("open", "Record open {k: T, ...}", "A record type a tool declares open: when a result is decoded into it, the fields the type does not name are dropped, so a program sees only the named ones. A plain Record rejects extra fields, and a record literal is always plain.", "(call contacts.get {:id \"c1\"})", "{:name \"ada\"}"),
        Entry::new("Tuple", "Tuple<A, B, ...>: [a b ...]", "A fixed-size group of values of different types, from par, group-by, and bracket literals of mixed types or that a tuple pattern takes apart.", "(let [[n s] [1 \"a\"]] s)", "\"a\""),
    ]
}

pub fn entries() -> Vec<Entry> {
    FORMS.iter().cloned().chain(PRELUDE.iter().cloned()).chain(types()).collect()
}

pub fn entry_names() -> Vec<String> {
    entries().into_iter().map(|e| e.name.to_string()).collect()
}

const EXAMPLE_DECLARATIONS: &str = r#"{"types": {"User": "Record {name: String, active: Bool}", "File": "Record {bytes: Int}", "Contact": "Record open {name: String}"}, "capabilities": [{"id": "people.list", "effect": "read", "input": "Record {team: String}", "output": "List<User>", "description": "Lists the people on a team."}, {"id": "metrics.uptime", "effect": "read", "input": "Record {host: String}", "output": "Record {seconds: Int}", "description": "Reads a host's uptime."}, {"id": "storage.files", "effect": "read", "input": "Record {page: Int}", "output": "Record {files: List<File>, last: Bool}", "description": "Reads one page of files."}, {"id": "blob.read", "effect": "read", "input": "Record {name: String}", "output": "Bytes", "description": "Reads one stored blob."}, {"id": "contacts.get", "effect": "read", "input": "Record {id: String}", "output": "Contact", "description": "Reads one contact."}, {"id": "job.state", "effect": "read", "input": "Record {id: String}", "output": "String", "description": "Reads a job's state."}, {"id": "chat.post", "effect": "write", "idempotent": false, "input": "Record {text: String}", "output": "Unit", "description": "Posts one chat message."}]}"#;

const EXAMPLE_RESPONSES: &str = r#"[{"capability": "people.list", "input": {"team": "infra"}, "results": [{"output": [{"name": "lin", "active": true}, {"name": "ada", "active": true}, {"name": "bo", "active": false}]}]}, {"capability": "metrics.uptime", "input": {"host": "db1"}, "results": [{"output": {"seconds": 100}}]}, {"capability": "metrics.uptime", "input": {"host": "db2"}, "results": [{"output": {"seconds": 200}}]}, {"capability": "metrics.uptime", "input": {"host": "web1"}, "results": [{"error": {"kind": "Timeout", "message": "no answer"}}]}, {"capability": "metrics.uptime", "input": {"host": "gone"}, "results": [{"error": {"kind": "NotFound", "message": "no such host"}}]}, {"capability": "metrics.uptime", "input": {"host": "flaky"}, "results": [{"error": {"kind": "Unavailable", "message": "502"}}, {"output": {"seconds": 60}}]}, {"capability": "storage.files", "input": {"page": 1}, "results": [{"output": {"files": [{"bytes": 10}, {"bytes": 20}], "last": false}}]}, {"capability": "storage.files", "input": {"page": 2}, "results": [{"output": {"files": [{"bytes": 5}], "last": true}}]}, {"capability": "chat.post", "input": {"text": "hi"}, "results": [{"error": {"kind": "RateLimited", "message": "slow down"}}, {"output": null}]}, {"capability": "blob.read", "input": {"name": "greeting"}, "results": [{"output": "aGVsbG8="}]}, {"capability": "blob.read", "input": {"name": "again"}, "results": [{"output": "aGVsbG8="}]}, {"capability": "contacts.get", "input": {"id": "c1"}, "results": [{"output": {"name": "ada", "email": "ada@example.com"}}]}, {"capability": "job.state", "input": {"id": "j1"}, "results": [{"output": "running"}, {"output": "done"}]}]"#;

pub fn example_declarations() -> Json {
    json::parse(EXAMPLE_DECLARATIONS).expect("the example declarations are valid json")
}

pub fn example_responses() -> Json {
    json::parse(EXAMPLE_RESPONSES).expect("the example responses are valid json")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideLevel {
    Minimal,
    Standard,
    Full,
}

impl GuideLevel {
    pub const ALL: [GuideLevel; 3] = [GuideLevel::Minimal, GuideLevel::Standard, GuideLevel::Full];

    pub fn name(self) -> &'static str {
        match self {
            GuideLevel::Minimal => "minimal",
            GuideLevel::Standard => "standard",
            GuideLevel::Full => "full",
        }
    }

    pub fn from_name(name: &str) -> Option<GuideLevel> {
        GuideLevel::ALL.into_iter().find(|level| level.name() == name)
    }
}

const STANDARD_GUIDE: &str = include_str!("../../../src/sx/guide.md");
pub const KERNEL: &str = include_str!("../guide/kernel.md");
pub const TYPESCRIPT: &str = include_str!("../guide/typescript.md");
pub const WRITING: &str = include_str!("../guide/writing.md");

const MINIMAL_HEADER: &str = "# Sx

Sx calls tools and combines their results. A program is one expression; its value is the result.
It looks like Clojure, but only the forms and functions below exist.

Values: true false, integers, floats (1.5, never mixed with integers), \"strings\", [lists],
{:field value} records (keywords only name fields), [1 \"a\"] tuples (mixed types), (some x) and none (no nil;
a plain value works where an optional is expected), (ok x) and (err e). Durations such as 250ms 5s 2m 1h
appear only in retry and wait-until options and as the argument of sleep. Names may be kebab-case, camelCase, or
snake_case. Comments start with ;.
";

fn first_sentence(text: &str) -> String {
    let head = text.split(". ").next().unwrap_or(text);
    format!("{}.", head.trim_end_matches('.'))
}

pub fn guide(level: GuideLevel) -> String {
    match level {
        GuideLevel::Standard => STANDARD_GUIDE.to_string(),
        GuideLevel::Minimal => {
            let mut lines = vec![MINIMAL_HEADER.to_string(), "Forms:".to_string()];
            for e in FORMS.iter().filter(|e| !matches!(e.name, "some" | "ok" | "err")) {
                lines.push(format!("  {}  {}", e.signature, first_sentence(&e.description)));
            }
            lines.push(String::new());
            lines.push("Functions (function arguments come first):".to_string());
            for e in &PRELUDE {
                lines.push(format!("  {}", e.signature));
            }
            let kinds = CallErrorKind::ALL.map(CallErrorKind::name).join(" ");
            lines.push(String::new());
            lines.push(format!("Error kinds for attempt, retry :on, and (get e :kind): {kinds}."));
            lines.push(String::new());
            lines.join("\n")
        }
        GuideLevel::Full => {
            let mut lines = vec![STANDARD_GUIDE.trim_end().to_string(), String::new(), "## More examples".to_string(), String::new()];
            for e in entries() {
                lines.push(format!("- `{}` returns `{}`", e.example, e.result));
            }
            lines.join("\n") + "\n"
        }
    }
}

pub fn render_entry(e: &Entry) -> String {
    format!("{}\n{}\nExample: {}\nResult: {}\n", e.signature, e.description, e.example, e.result)
}

pub fn lookup_entry(name: &str) -> Option<String> {
    entries().iter().find(|e| e.name == name).map(render_entry)
}

pub fn lookup_extension(name: &str) -> Option<String> {
    EXTENSIONS.iter().find(|e| e.name == name).map(render_entry)
}

pub fn not_found_message(name: &str, candidates: &[String]) -> String {
    let hint = crate::checker::closest(name, candidates).map(|s| format!(" Did you mean {s}?")).unwrap_or_default();
    format!("sx help: nothing is named {name}.{hint}")
}

pub fn effect_label(effect: Effect, idempotent: bool) -> &'static str {
    match (effect, idempotent) {
        (Effect::Read, _) => "read",
        (Effect::Write, true) => "write, idempotent",
        (Effect::Write, false) => "write, not idempotent",
    }
}

fn parameters(function: &Function) -> String {
    function.params.iter().map(show_type).collect::<Vec<_>>().join(" ")
}

fn variants_text(variants: &[(String, Option<Type>)]) -> String {
    variants
        .iter()
        .map(|(variant, payload)| match payload {
            None => variant.clone(),
            Some(payload) => format!("({variant} {})", show_type(payload)),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

pub fn render_function(function: &Function) -> String {
    let mut lines = vec![
        format!("({} {}) -> {}", function.name, parameters(function), show_type(&function.output)),
        function.description.clone(),
    ];
    if let Some(example) = &function.example {
        lines.push(format!("Example: {}", example.program));
        lines.push(format!("Result: {}", example.result));
    }
    lines.join("\n") + "\n"
}

pub fn capability_signature(capability: &Capability) -> String {
    format!(
        "(call {} {}) -> {}   [{}]",
        capability.id,
        show_type(&capability.input),
        show_type(&capability.output),
        effect_label(capability.effect, capability.idempotent)
    )
}

pub fn render_capability(capability: &Capability, registry: &Registry) -> String {
    let mut lines = vec![capability_signature(capability), capability.description.clone()];
    if let Some(scope) = &capability.scope {
        lines.push(format!("Scope: {}, granted by the host; the program never names it.", show_type(scope)));
    }
    lines.extend(named_types(&[&capability.input, &capability.output], registry).into_iter().map(|line| format!("  {line}")));
    lines.join("\n") + "\n"
}

fn named_types(types: &[&Type], registry: &Registry) -> Vec<String> {
    let wanted: Vec<String> = types.iter().map(|t| show_type(t)).collect();
    let mut lines = Vec::new();
    for (name, t) in registry.types() {
        if !wanted.iter().any(|text| text.contains(name)) {
            continue;
        }
        match t {
            Type::Enum(enumeration) => lines.push(format!("{name} = {}", variants_text(&enumeration.variants))),
            Type::Record(_) => lines.push(format!("{name} = {}", show_definition(t))),
            Type::Nominal(nominal) if nominal.validation.is_some() => {
                lines.push(format!("{name} = {}", show_definition(t)));
            }
            _ => {}
        }
    }
    lines
}

const UNVERIFIED_MEANING: &str = "the interface was validated when the core registered";

fn render_assurance(core: &Core) -> Vec<String> {
    let mut lines = vec!["Assurance:".to_string()];
    let levels = core.assurance();
    for (level, references) in &levels {
        if *level == crate::registry::Assurance::Unverified {
            lines.push(format!("  {}: {UNVERIFIED_MEANING}", level.name()));
            continue;
        }
        lines.push(format!("  {}:", level.name()));
        lines.extend(references.iter().map(|reference| format!("    {reference}")));
    }
    for level in crate::registry::Assurance::ALL {
        if !levels.iter().any(|(held, _)| *held == level) {
            lines.push(format!("  {}: none", level.name()));
        }
    }
    lines
}

pub fn render_core(core: &Core) -> String {
    let mut lines = vec![format!("{} {}  [core]", core.id, core.version)];
    lines.extend(render_assurance(core));
    lines.push("Functions:".to_string());
    for function in &core.functions {
        lines.push(format!("  ({} {}) -> {}", function.name, parameters(function), show_type(&function.output)));
    }
    lines.join("\n") + "\n"
}

pub fn render_cores(registry: &Registry) -> String {
    registry
        .cores()
        .map(|core| {
            let levels: Vec<&str> = core.assurance().iter().map(|(level, _)| level.name()).collect();
            format!("{} {}  {}\n", core.id, core.version, levels.join(", "))
        })
        .collect()
}

pub fn lookup(name: &str, registry: &Registry) -> Option<String> {
    if let Some(found) = lookup_entry(name).or_else(|| lookup_extension(name)) {
        return Some(found);
    }
    if let Some(core) = registry.core(name) {
        return Some(render_core(core));
    }
    if let Some(function) = registry.function(name) {
        return Some(render_function(function));
    }
    if let Some(capability) = registry.capability(name) {
        return Some(render_capability(capability, registry));
    }
    let t = registry.named_types().get(name)?;
    let mut lines = named_types(&[t], registry);
    if lines.is_empty() {
        lines.push(format!("{name} = {}", show_type(t)));
    }
    Some(lines.join("\n") + "\n")
}

pub fn names(registry: &Registry) -> Vec<String> {
    let mut found = entry_names();
    found.extend(EXTENSIONS.iter().map(|e| e.name.to_string()));
    found.extend(registry.cores().map(|core| core.id.clone()));
    found.extend(registry.functions().map(|function| function.name.clone()));
    found.extend(registry.capabilities().map(|capability| capability.id.clone()));
    found.extend(registry.types().map(|(name, _)| name.to_string()));
    found
}

fn namespace(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

pub fn tool_listing(
    registry: &Registry,
    capability_ids: &[&str],
    core_ids: &[&str],
) -> Result<String, RegistryError> {
    let mut lines: Vec<String> = Vec::new();
    let outside: Vec<&Type> =
        registry.types().filter(|(name, _)| !core_ids.contains(&namespace(name))).map(|(_, t)| t).collect();
    let records: Vec<(&str, &Type)> =
        registry.types().filter(|(_, t)| matches!(t, Type::Record(_)) && outside.contains(t)).collect();
    let enums: Vec<(&str, &Type)> = registry.types().filter(|(_, t)| matches!(t, Type::Enum(_))).collect();
    if !records.is_empty() {
        lines.push("Types:".to_string());
        lines.extend(records.iter().map(|(name, t)| format!("  {name} = {}", show_definition(t))));
        lines.push(String::new());
    }
    if !enums.is_empty() {
        lines.push("Enums (match on their variants):".to_string());
        for (name, t) in &enums {
            if let Type::Enum(enumeration) = t {
                lines.push(format!("  {name} = {}", variants_text(&enumeration.variants)));
            }
        }
        lines.push(String::new());
    }
    lines.push("Tools:".to_string());
    for id in capability_ids {
        let capability = registry
            .capability(id)
            .ok_or_else(|| RegistryError(format!("the listing names a tool the registry does not define: {id}")))?;
        lines.push(format!("  {}", capability_signature(capability)));
        lines.push(format!("      {}", capability.description));
    }
    lines.push(String::new());
    let functions: Vec<&Function> = registry.functions().filter(|f| core_ids.contains(&namespace(&f.name))).collect();
    if !functions.is_empty() {
        lines.push("Functions:".to_string());
        for (name, t) in registry.types() {
            if core_ids.contains(&namespace(name)) {
                lines.push(format!("  {name} = {}", show_definition(t)));
            }
        }
        for function in functions {
            lines.push(format!(
                "  ({} {}) -> {}   {}",
                function.name,
                parameters(function),
                show_type(&function.output),
                function.description
            ));
        }
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}
