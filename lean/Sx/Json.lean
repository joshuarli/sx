import Lean.Data.Json
import Sx.Value

namespace Sx

open Lean (Json JsonNumber)

def objFields (j : Json) : List (String × Json) :=
  match j with
  | .obj kvs => kvs.toList
  | _ => []

def field (j : Json) (k : String) : Except String Json :=
  match fieldLookup (objFields j) k with
  | some v => pure v
  | none => throw s!"missing field {k} in {j.compress}"

def asArr (j : Json) : Except String (List Json) :=
  match j with
  | .arr a => pure a.toList
  | _ => throw s!"expected an array, found {j.compress}"

def asStr (j : Json) : Except String String :=
  match j with
  | .str s => pure s
  | _ => throw s!"expected a string, found {j.compress}"

def asNat (j : Json) : Except String Nat := do
  let i ← j.getInt?
  if i < 0 then throw s!"expected a natural number, found {i}" else pure i.toNat

def asPair (j : Json) : Except String (Json × Json) := do
  match ← asArr j with
  | [a, b] => pure (a, b)
  | _ => throw s!"expected a pair, found {j.compress}"

def tagOf (j : Json) : Except String (String × Json) :=
  match j with
  | .str s => pure (s, .null)
  | .obj _ =>
    match objFields j with
    | (k, v) :: _ => pure (k, v)
    | [] => throw "expected a tagged object, found {}"
  | _ => throw s!"expected a tag, found {j.compress}"

partial def parseTy (j : Json) : Except String Ty := do
  match j with
  | .str "Bool" => pure .bool
  | .str "Int" => pure .int
  | .str "String" => pure .str
  | .str "Unit" => pure .unit
  | .str "Never" => pure .never
  | _ =>
    if let .ok t := field j "list" then return .list (← parseTy t)
    if let .ok t := field j "map" then return .map (← parseTy t)
    if let .ok ts := field j "tuple" then return .tuple (← (← asArr ts).mapM parseTy)
    if let .ok t := field j "option" then return .option (← parseTy t)
    if let .ok p := field j "result" then
      let (a, b) ← asPair p
      return .result (← parseTy a) (← parseTy b)
    if let .ok fs := field j "record" then
      return .record (← (← asArr fs).mapM fun f => do
        let (n, t) ← asPair f
        pure (← asStr n, ← parseTy t))
    if let .ok n := field j "enum" then return .enum (← asStr n)
    if let .ok n := field j "nominal" then return .nominal (← asStr n) (← parseTy (← field j "base"))
    if let .ok n := field j "var" then return .var (← asNat n)
    throw s!"unknown type {j.compress}"

def parseInt (j : Json) : Except String Int := j.getInt?

partial def parseVal (j : Json) : Except String Val := do
  match j with
  | .str "unit" => pure .unit
  | .str "none" => pure .none
  | _ =>
    let (tag, v) ← tagOf j
    match tag with
    | "bool" =>
      match v with
      | .bool b => pure (.bool b)
      | _ => throw "bad bool literal"
    | "int" => pure (.int (← parseInt v))
    | "string" => pure (.str (← asStr v))
    | "some" => pure (.some (← parseVal v))
    | "ok" => pure (.ok (← parseVal v))
    | "err" => pure (.err (← parseVal v))
    | "list" => pure (.list (← (← asArr v).mapM parseVal))
    | "tuple" => pure (.tuple (← (← asArr v).mapM parseVal))
    | "record" => pure (.record (← (← asArr v).mapM fun f => do
        let (n, x) ← asPair f
        pure (← asStr n, ← parseVal x)))
    | "map" => pure (.map (← (← asArr v).mapM fun f => do
        let (n, x) ← asPair f
        pure (← asStr n, ← parseVal x)))
    | "variant" =>
      let payload ← field v "payload"
      let p ← if payload.isNull then pure none else some <$> parseVal payload
      pure (.variant (← asStr (← field v "enum")) (← asStr (← field v "name")) p)
    | _ => throw s!"unknown value {j.compress}"

partial def parseBinder (j : Json) : Except String Binder := do
  match j with
  | .str "discard" => pure .discard
  | _ =>
    let (tag, v) ← tagOf j
    match tag with
    | "bind" => pure (.name (← asStr v))
    | "tuple" => pure (.tuple (← (← asArr v).mapM parseBinder))
    | _ => throw s!"unknown binder {j.compress}"

partial def parsePat (j : Json) : Except String Pat := do
  match j with
  | .str "none" => pure .none
  | _ =>
    if let .ok v := field j "variant" then
      let inner ← field j "inner"
      let p ← if inner.isNull then pure none else some <$> parsePat inner
      return .variant (← asStr v) p
    let (tag, v) ← tagOf j
    match tag with
    | "some" => pure (.some (← parsePat v))
    | "ok" => pure (.ok (← parsePat v))
    | "err" => pure (.err (← parsePat v))
    | "any" => pure (.any (← parseBinder v))
    | _ => throw s!"unknown pattern {j.compress}"

partial def parseConv (j : Json) : Except String Conv := do
  match j with
  | .str "variant-name" => pure .variantName
  | _ =>
    let opt (x : Json) : Except String (Option Conv) :=
      if x.isNull then pure none else some <$> parseConv x
    let (tag, v) ← tagOf j
    match tag with
    | "to-some" => pure (.toSome (← opt v))
    | "each-field" => pure (.eachField (← (← asArr v).mapM fun f => do
        let (n, c) ← asPair f
        pure (← asStr n, ← opt c)))
    | "each-item" => pure (.eachItem (← (← asArr v).mapM opt))
    | "each-element" => pure (.eachElement (← parseConv v))
    | _ => throw s!"unknown conversion {j.compress}"

def parseWrap : String → Except String WrapKind
  | "some" => pure .some
  | "ok" => pure .ok
  | "err" => pure .err
  | k => throw s!"unknown wrapper {k}"

partial def parseExpr (j : Json) : Except String Expr := do
  let node ← asStr (← field j "node")
  let sub (k : String) : Except String Expr := do parseExpr (← field j k)
  let subs (k : String) : Except String (List Expr) := do (← asArr (← field j k)).mapM parseExpr
  match node with
  | "Literal" => pure (.lit (← parseVal (← field j "value")) (← parseTy (← field j "type")))
  | "Variable" => pure (.var (← asStr (← field j "name")))
  | "Let" => pure (.let_ (← parseBinder (← field j "binder")) (← sub "value") (← sub "body"))
  | "LocalFunction" => pure (.localFn (← asStr (← field j "name")) (← sub "body"))
  | "If" => pure (.if_ (← sub "condition") (← sub "then") (← sub "otherwise"))
  | "Match" =>
    let clauses ← (← asArr (← field j "clauses")).mapM fun c => do
      let (p, b) ← asPair c
      pure (← parsePat p, ← parseExpr b)
    pure (.match_ (← sub "scrutinee") clauses)
  | "Lambda" =>
    let captured ← field j "captured"
    let cap ← if captured.isNull then pure none else some <$> asStr captured
    pure (.lam (← (← asArr (← field j "params")).mapM parseBinder)
      (← (← asArr (← field j "param_types")).mapM parseTy) (← parseTy (← field j "result")) (← sub "body") cap)
  | "ApplyLocal" => pure (.applyLocal (← asStr (← field j "name")) (← sub "function") (← subs "args"))
  | "Prelude" =>
    let name ← asStr (← field j "op")
    match Op.ofString name with
    | some op => pure (.prelude op (← subs "args"))
    | none => throw s!"unknown prelude function {name}"
  | "CoreCall" => pure (.core (← asStr (← field j "function")) (← subs "args"))
  | "Call" => pure (.call (← asStr (← field j "capability")) (← sub "argument"))
  | "Get" => pure (.get (← sub "record") (← asStr (← field j "field")))
  | "MakeRecord" =>
    pure (.mkRecord (← (← asArr (← field j "fields")).mapM fun f => do
      let (n, e) ← asPair f
      pure (← asStr n, ← parseExpr e)))
  | "MakeList" => pure (.mkList (← subs "items") (← parseTy (← field j "element")))
  | "MakeTuple" => pure (.mkTuple (← subs "items"))
  | "Wrap" => pure (.wrap (← parseWrap (← asStr (← field j "kind"))) (← sub "value") (← parseTy (← field j "type")))
  | "MakeVariant" =>
    let payload ← field j "payload"
    let p ← if payload.isNull then pure none else some <$> parseExpr payload
    pure (.mkVariant (← asStr (← field j "enum")) (← asStr (← field j "variant")) p)
  | "Coerce" => pure (.coerce (← sub "value") (← parseConv (← field j "conversion")))
  | "Attempt" => pure (.attempt (← sub "body"))
  | "Retry" =>
    pure (.retry (← (← asArr (← field j "delays")).mapM asNat) (← (← asArr (← field j "kinds")).mapM asStr) (← sub "body"))
  | "WaitUntil" => pure (.waitUntil (← (← asArr (← field j "intervals")).mapM asNat) (← sub "predicate") (← sub "body"))
  | "Loop" =>
    pure (.loop (← asNat (← field j "maximum")) (← (← asArr (← field j "binders")).mapM parseBinder)
      (← subs "initial") (← sub "body"))
  | "Recur" => pure (.recur (← subs "args"))
  | "Par" => pure (.par (← subs "branches"))
  | "ParMap" => pure (.parMap (← asNat (← field j "limit")) (← sub "function") (← sub "items"))
  | other => throw s!"the IR node {other} cannot appear in a checked program"

def parseEffect : String → Except String Effect
  | "read" => pure .read
  | "write" => pure .write
  | e => throw s!"unknown effect {e}"

def parseProgram (j : Json) : Except String Program := do
  let enums ← (objFields (← field j "enums")).mapM fun (name, vs) => do
    let variants ← (← asArr vs).mapM fun v => do
      let (n, p) ← asPair v
      pure (← asStr n, ← if p.isNull then pure none else some <$> parseTy p)
    pure (name, variants)
  let sigs ← field j "signatures"
  let capSigs ← (objFields (← field sigs "capabilities")).mapM fun (id, s) => do
    let idempotent ← match ← field s "idempotent" with
      | .bool b => pure b
      | _ => throw "idempotent must be a boolean"
    pure (id, ({ effect := ← parseEffect (← asStr (← field s "effect")), idempotent,
                 input := ← parseTy (← field s "input"), output := ← parseTy (← field s "output") } : CapSig))
  let funSigs ← (objFields (← field sigs "functions")).mapM fun (name, s) => do
    pure (name, ({ params := ← (← asArr (← field s "params")).mapM parseTy,
                   output := ← parseTy (← field s "output") } : FunSig))
  let inputs ← (objFields (← field j "inputs")).mapM fun (n, t) => do pure (n, ← parseTy t)
  pure {
    body := ← parseExpr (← field j "body")
    result := ← parseTy (← field j "result")
    caps := ← (← asArr (← field j "capabilities")).mapM asStr
    inputs
    enums := callErrorKindEnum :: enums.filter (·.1 != "CallErrorKind")
    capSigs
    funSigs
  }

def intJson (i : Int) : Json := .num (JsonNumber.fromInt i)

mutual
def Val.encode : Val → Json
  | .bool b => .bool b
  | .int i => intJson i
  | .str s => .str s
  | .unit => .null
  | .none => .null
  | .list vs => .arr (encodeList vs).toArray
  | .tuple vs => .arr (encodeList vs).toArray
  | .record fs => Json.mkObj (encodeFields fs)
  | .map es => Json.mkObj (encodeFields es)
  | .some v => v.encode
  | .ok v => Json.mkObj [("ok", v.encode)]
  | .err v => Json.mkObj [("err", v.encode)]
  | .variant _ n none => .str n
  | .variant _ n (some p) => Json.mkObj [("tag", .str n), ("value", p.encode)]
def encodeList : List Val → List Json
  | [] => []
  | v :: vs => v.encode :: encodeList vs
def encodeFields : List (String × Val) → List (String × Json)
  | [] => []
  | (k, v) :: fs => (k, v.encode) :: encodeFields fs
end

def jsonKind : Json → String
  | .null => "null"
  | .bool _ => "a boolean"
  | .num n => if n.exponent == 0 then "an integer" else "a number with a fraction"
  | .str _ => "a string"
  | .arr _ => "an array"
  | .obj _ => "an object"

def enumVariants (enums : Enums) (name : String) : List (String × Option Ty) :=
  (fieldLookup enums name).getD []

partial def decode (enums : Enums) (t : Ty) (j : Json) (path : String) : Except String Val := do
  let fail {α : Type} (expected : String) : Except String α :=
    throw s!"{path}: expected {expected}, found {jsonKind j}"
  match t, j with
  | .bool, .bool b => pure (.bool b)
  | .bool, _ => fail "Bool"
  | .int, .num n =>
    if n.exponent != 0 then fail "Int"
    else if intMin ≤ n.mantissa ∧ n.mantissa ≤ intMax then pure (.int n.mantissa)
    else throw s!"{path}: expected Int, found an integer outside the signed 64-bit range"
  | .int, _ => fail "Int"
  | .str, .str s => pure (.str s)
  | .str, _ => fail "String"
  | .unit, .null => pure .unit
  | .unit, _ => fail "Unit (null)"
  | .list e, .arr items => do
    let vs ← (items.toList.zipIdx).mapM fun (x, i) => decode enums e x s!"{path}[{i}]"
    pure (.list vs)
  | .list _, _ => fail "a list"
  | .tuple ts, .arr items =>
    if items.size != ts.length then fail s!"a tuple as an array of {ts.length}"
    else do
      let vs ← ((items.toList.zip ts).zipIdx).mapM fun ((x, it), i) => decode enums it x s!"{path}[{i}]"
      pure (.tuple vs)
  | .tuple ts, _ => fail s!"a tuple as an array of {ts.length}"
  | .option _, .null => pure .none
  | .option inner, _ => do pure (.some (← decode enums inner j path))
  | .result ok err, .obj _ =>
    match objFields j with
    | [("ok", x)] => do pure (.ok (← decode enums ok x s!"{path}.ok"))
    | [("err", x)] => do pure (.err (← decode enums err x s!"{path}.err"))
    | _ => fail "{\"ok\": ...} or {\"err\": ...}"
  | .result _ _, _ => fail "{\"ok\": ...} or {\"err\": ...}"
  | .map v, .obj _ => do
    let es ← (objFields j).mapM fun (k, x) => do pure (k, ← decode enums v x s!"{path}[{(Json.str k).compress}]")
    pure (.map es)
  | .map _, _ => fail "a map"
  | .record fs, .obj _ => do
    let obj := objFields j
    match fs.find? fun (n, _) => (fieldLookup obj n).isNone with
    | some (n, _) => throw s!"{path}.{n}: expected a value, found no field"
    | none =>
      match obj.find? fun (k, _) => (fieldLookup fs k).isNone with
      | some (k, _) => throw s!"{path}.{k}: expected no field, because the record type does not declare it, found a field"
      | none => do
        let vs ← fs.mapM fun (n, ft) => do
          match fieldLookup obj n with
          | some x => pure (n, ← decode enums ft x s!"{path}.{n}")
          | none => throw s!"{path}.{n}: missing"
        pure (.record vs)
  | .record _, _ => fail "a record"
  | .enum name, .str s =>
    match (enumVariants enums name).find? fun (v, p) => v == s && p.isNone with
    | some _ => pure (.variant name s none)
    | none => fail name
  | .enum name, .obj _ =>
    match objFields j with
    | [("tag", .str tag), ("value", x)] =>
      match (enumVariants enums name).find? fun (v, p) => v == tag && p.isSome with
      | some (_, some pt) => do pure (.variant name tag (some (← decode enums pt x s!"{path}.value")))
      | _ => fail name
    | _ => fail name
  | .enum name, _ => fail name
  | .nominal _ base, _ => decode enums base j path
  | .var _, _ => throw s!"{path}: expected a known type, found a type the checker could not resolve"
  | _, _ => fail "a value of a type with no encoding"

partial def jsonEq (a b : Json) : Bool :=
  match a, b with
  | .null, .null => true
  | .bool x, .bool y => x == y
  | .num x, .num y => x == y
  | .str x, .str y => x == y
  | .arr xs, .arr ys => xs.size == ys.size && (xs.toList.zip ys.toList).all fun (x, y) => jsonEq x y
  | .obj _, .obj _ =>
    let xs := objFields a
    let ys := objFields b
    xs.length == ys.length && (xs.zip ys).all fun ((k, x), (k', y)) => k == k' && jsonEq x y
  | _, _ => false

end Sx
