import Lean.Data.Json
import Sx.Value
import Sx.Decimal

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
  | .str "Float" => pure .float
  | .str "Bytes" => pure .bytes
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
      return .record (sortFields (← (← asArr fs).mapM fun f => do
        let (n, t) ← asPair f
        pure (← asStr n, ← parseTy t)))
    if let .ok n := field j "enum" then return .enum (← asStr n)
    if let .ok n := field j "nominal" then return .nominal (← asStr n) (← parseTy (← field j "base"))
    if let .ok n := field j "var" then return .var (← asNat n)
    throw s!"unknown type {j.compress}"

partial def parseDTy (j : Json) : Except String DTy := do
  match j with
  | .str "Bool" => pure .bool
  | .str "Int" => pure .int
  | .str "Float" => pure .float
  | .str "Bytes" => pure .bytes
  | .str "String" => pure .str
  | .str "Unit" => pure .unit
  | .str "Never" => pure .never
  | _ =>
    if let .ok t := field j "list" then return .list (← parseDTy t)
    if let .ok t := field j "map" then return .map (← parseDTy t)
    if let .ok ts := field j "tuple" then return .tuple (← (← asArr ts).mapM parseDTy)
    if let .ok t := field j "option" then return .option (← parseDTy t)
    if let .ok p := field j "result" then
      let (a, b) ← asPair p
      return .result (← parseDTy a) (← parseDTy b)
    if let .ok fs := field j "record" then
      let isOpen := match fieldLookup (objFields j) "open" with
        | some (.bool b) => b
        | _ => false
      return .record (sortFields (← (← asArr fs).mapM fun f => do
        let (n, t) ← asPair f
        pure (← asStr n, ← parseDTy t))) isOpen
    if let .ok n := field j "enum" then return .enum (← asStr n)
    if let .ok n := field j "nominal" then return .nominal (← asStr n) (← parseDTy (← field j "base"))
    if let .ok n := field j "var" then return .var (← asNat n)
    throw s!"unknown type {j.compress}"

def numberRat (n : JsonNumber) : Bool × Nat × Nat := (n.mantissa < 0, n.mantissa.natAbs, 10 ^ n.exponent)

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
    | "float" =>
      match v with
      | .num n =>
        let (neg, a, b) := numberRat n
        match floatOfRat neg a b with
        | some f => pure (.float f)
        | none => throw "a float literal is out of range"
      | _ => throw "bad float literal"
    | "bytes" => pure (.bytes (← asStr v))
    | "string" => pure (.str (← asStr v))
    | "some" => pure (.some (← parseVal v))
    | "ok" => pure (.ok (← parseVal v))
    | "err" => pure (.err (← parseVal v))
    | "list" => pure (.list (← (← asArr v).mapM parseVal))
    | "tuple" => pure (.tuple (← (← asArr v).mapM parseVal))
    | "record" => pure (.record (sortFields (← (← asArr v).mapM fun f => do
        let (n, x) ← asPair f
        pure (← asStr n, ← parseVal x))))
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
    | "literal" => pure (.literal (← parseVal v))
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
    | "each-field" => pure (.eachField (sortFields (← (← asArr v).mapM fun f => do
        let (n, c) ← asPair f
        pure (← asStr n, ← opt c))))
    | "widen-record" => pure (.widenRecord (sortFields (← (← asArr v).mapM fun f => do
        match ← asArr f with
        | [n, present, c, t] =>
          let present ← match present with
            | .bool b => pure b
            | _ => throw "present must be a boolean"
          pure (← asStr n, present, ← opt c, ← parseTy t)
        | _ => throw "bad widened field")))
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
    if name == "tuple-item" then
      match ← subs "args" with
      | [xs, .lit (.int i) .int] =>
        if i < 0 then throw "tuple-item takes a nonnegative index" else return .tupleItem xs i.toNat
      | _ => throw "tuple-item takes a tuple and a literal index"
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
  | "Fail" => pure (.fail (← sub "message"))
  | "Log" => pure (.log (← subs "args"))
  | "Retry" =>
    pure (.retry (← (← asArr (← field j "delays")).mapM asNat) (← (← asArr (← field j "kinds")).mapM asStr) (← sub "body"))
  | "Sleep" => pure (.sleep (← asNat (← field j "milliseconds")))
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
  let decodeEnums ← (objFields (← field j "enums")).mapM fun (name, vs) => do
    let variants ← (← asArr vs).mapM fun v => do
      let (n, p) ← asPair v
      pure (← asStr n, ← if p.isNull then pure none else some <$> parseDTy p)
    pure (name, variants)
  let capOutputs ← (objFields (← field (← field j "signatures") "capabilities")).mapM fun (id, s) => do
    pure (id, ← parseDTy (← field s "output"))
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
  let inputDecodes ← (objFields (← field j "inputs")).mapM fun (n, t) => do pure (n, ← parseDTy t)
  pure {
    body := ← parseExpr (← field j "body")
    result := ← parseTy (← field j "result")
    caps := ← (← asArr (← field j "capabilities")).mapM asStr
    inputs
    enums := callErrorKindEnum :: enums.filter (·.1 != "CallErrorKind")
    capSigs
    funSigs
    capOutputs
    inputDecodes
    decodeEnums := (callErrorKindEnum.1, callErrorKinds.map fun k => (k, none)) ::
      decodeEnums.filter (·.1 != "CallErrorKind")
  }

def intJson (i : Int) : Json := .num (JsonNumber.fromInt i)

def floatJson (f : Float) : Json :=
  match shortestDigits f with
  | some (c, e) =>
    let m : Int := if f < 0.0 then -(c : Int) else c
    if e ≥ 0 then .num ⟨m * 10 ^ e.toNat, 0⟩ else .num ⟨m, (-e).toNat⟩
  | none => .null

mutual
def Val.encode : Val → Json
  | .bool b => .bool b
  | .int i => intJson i
  | .float f => floatJson f
  | .bytes b => .str b
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

def enumVariants (enums : DecodeEnums) (name : String) : List (String × Option DTy) :=
  (fieldLookup enums name).getD []

partial def decode (enums : DecodeEnums) (t : DTy) (j : Json) (path : String) : Except String Val := do
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
  | .float, .num n =>
    let (neg, a, b) := numberRat n
    match floatOfRat neg a b with
    | some f => pure (.float f)
    | none => throw s!"{path}: expected Float, found a number outside the finite 64-bit Float range"
  | .float, _ => fail "Float"
  | .bytes, .str s =>
    if canonicalBase64 s then pure (.bytes s)
    else throw s!"{path}: expected Bytes (standard base64 with padding), found a string that is not valid base64"
  | .bytes, _ => fail "Bytes (a base64 string)"
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
  | .record fs isOpen, .obj _ => do
    let obj := objFields j
    match fs.find? fun (n, _) => (fieldLookup obj n).isNone with
    | some (n, _) => throw s!"{path}.{n}: expected a value, found no field"
    | none =>
      match obj.find? fun (k, _) => !isOpen && (fieldLookup fs k).isNone with
      | some (k, _) => throw s!"{path}.{k}: expected no field, because the record type does not declare it, found a field"
      | none => do
        let vs ← fs.mapM fun (n, ft) => do
          match fieldLookup obj n with
          | some x => pure (n, ← decode enums ft x s!"{path}.{n}")
          | none => throw s!"{path}.{n}: missing"
        pure (.record vs)
  | .record _ _, _ => fail "a record"
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
  | .nominal name base, _ =>
    match nominalCanon name with
    | none => decode enums base j path
    | some canon =>
      match j with
      | .str s =>
        match canon s with
        | .ok canonical => pure (.str canonical)
        | .error reason => throw s!"{path}: expected {name}, found a string that is not valid: {reason}"
      | _ => fail s!"{name} (a string)"
  | .var _, _ => throw s!"{path}: expected a known type, found a type the checker could not resolve"
  | _, _ => fail "a value of a type with no encoding"

partial def jsonEq (a b : Json) : Bool :=
  match a, b with
  | .null, .null => true
  | .bool x, .bool y => x == y
  | .num x, .num y => x.mantissa * 10 ^ y.exponent == y.mantissa * 10 ^ x.exponent
  | .str x, .str y => x == y
  | .arr xs, .arr ys => xs.size == ys.size && (xs.toList.zip ys.toList).all fun (x, y) => jsonEq x y
  | .obj _, .obj _ =>
    let xs := objFields a
    let ys := objFields b
    xs.length == ys.length && (xs.zip ys).all fun ((k, x), (k', y)) => k == k' && jsonEq x y
  | _, _ => false

def zeros (n : Nat) : String := String.ofList (List.replicate n '0')

def numberChar (c : Char) : Bool := c.isDigit || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E'

def expandExponent (token : String) : String :=
  match token.toList.span (fun c => c != 'e' && c != 'E') with
  | (_, []) => token
  | (mant, _ :: exp) =>
    let neg := mant.head? == some '-'
    let digits := (if neg then mant.drop 1 else mant)
    let (whole, frac) := digits.span (· != '.')
    let frac := frac.drop 1
    match (String.ofList exp).toInt? with
    | none => token
    | some e =>
      if e > 400 || e < -400 then token else
      let all := String.ofList (whole ++ frac)
      let sign := if neg then "-" else ""
      if all.all (· == '0') then sign ++ "0.0"
      else
        let point : Int := whole.length + e
        let text :=
          if point ≤ 0 then
            "0." ++ zeros point.natAbs ++ all
          else
            let p := point.toNat
            if p ≥ all.length then all ++ zeros (p - all.length) ++ ".0"
            else String.ofList (all.toList.take p) ++ "." ++ String.ofList (all.toList.drop p)
        let (w, f) := text.toList.span (· != '.')
        let w := String.ofList (w.dropWhile (· == '0'))
        sign ++ (if w.isEmpty then "0" else w) ++ String.ofList f

partial def expandExponents (cs : List Char) (inString escaped : Bool) (out : String) : String :=
  match cs with
  | [] => out
  | c :: rest =>
    if inString then
      if escaped then expandExponents rest true false (out.push c)
      else if c == '\\' then expandExponents rest true true (out.push c)
      else expandExponents rest (c != '"') false (out.push c)
    else if c == '"' then expandExponents rest true false (out.push c)
    else if c.isDigit || c == '-' then
      let (token, after) := cs.span numberChar
      expandExponents after false false (out ++ expandExponent (String.ofList token))
    else expandExponents rest false false (out.push c)

def hexValue? (c : Char) : Option Nat :=
  if c.isDigit then some (c.toNat - '0'.toNat)
  else if 'a' ≤ c ∧ c ≤ 'f' then some (c.toNat - 'a'.toNat + 10)
  else if 'A' ≤ c ∧ c ≤ 'F' then some (c.toNat - 'A'.toNat + 10)
  else none

def hex4? : List Char → Option Nat
  | a :: b :: c :: d :: _ => do
    pure ((← hexValue? a) * 4096 + (← hexValue? b) * 256 + (← hexValue? c) * 16 + (← hexValue? d))
  | _ => none

partial def loneSurrogate : List Char → Bool → Bool
  | [], _ => false
  | '\\' :: 'u' :: rest, true =>
    match hex4? rest with
    | some v =>
      if 0xD800 ≤ v ∧ v ≤ 0xDBFF then
        match rest.drop 4 with
        | '\\' :: 'u' :: next =>
          match hex4? next with
          | some w => if 0xDC00 ≤ w ∧ w ≤ 0xDFFF then loneSurrogate (next.drop 4) true else true
          | none => true
        | _ => true
      else if 0xDC00 ≤ v ∧ v ≤ 0xDFFF then true
      else loneSurrogate (rest.drop 4) true
    | none => loneSurrogate rest true
  | '\\' :: _ :: rest, true => loneSurrogate rest true
  | '"' :: rest, inString => loneSurrogate rest (!inString)
  | _ :: rest, inString => loneSurrogate rest inString

def parseJsonText (text : String) : Except String Json :=
  if loneSurrogate text.toList false then .error "a JSON string has a lone surrogate escape"
  else Json.parse (expandExponents text.toList false false "")

end Sx
