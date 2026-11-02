import Sx.Host
import Sx.Check
import Sx.Eval
import Sx.Cores

open Lean (Json)

def readJson (path : String) : IO Json := do
  match Sx.parseJsonText (← IO.FS.readFile path) with
  | .ok j => pure j
  | .error e => throw (IO.userError s!"{path}: {e}")

def vectorArgs (function : String) (args : List Json) : Except String (List Sx.Val) := do
  let kinds := match function with
    | "decimal.parse" => "S"
    | "decimal.show" | "decimal.to-float" => "D"
    | "decimal.format" => "DI"
    | "decimal.from-int" => "I"
    | "decimal.add" | "decimal.sub" | "decimal.mul" | "decimal.compare" => "DD"
    | "decimal.div" => "DDIR"
    | "decimal.round" => "DIR"
    | _ => ""
  if kinds.length != args.length then throw s!"{function} takes {kinds.length} arguments"
  (kinds.toList.zip args).mapM fun (kind, j) => do
    match kind with
    | 'S' => pure (.str (← Sx.asStr j))
    | 'D' => pure (.str (← Sx.Decimal.canonicalText (← Sx.asStr j)))
    | 'I' => pure (.int (← j.getInt?))
    | _ => pure (.variant "decimal.Rounding" (← Sx.asStr j) none)

def checkVector (v : Json) : Except String Bool := do
  let function ← Sx.asStr (← Sx.field v "function")
  let args ← vectorArgs function (← Sx.asArr (← Sx.field v "args"))
  let implementation ← match Sx.coreLibrary function with
    | some f => pure f
    | none => throw s!"{function} has no implementation"
  match implementation args, Sx.fieldLookup (Sx.objFields v) "fails", Sx.fieldLookup (Sx.objFields v) "result" with
  | .ok value, none, some expected => pure (Sx.jsonEq value.encode expected)
  | .error f, some _, none => pure (f.kind == .arithmetic)
  | _, _, _ => pure false

def pathOf (j : Json) : Except String (List String) := do
  (← Sx.asArr j).mapM Sx.asStr

def jsonCheck (text : String) (check : Json) : Except String Bool := do
  let fields := Sx.objFields check
  let is ← Sx.field check "is"
  if (Sx.fieldLookup fields "get").isSome then
    let path ← pathOf (← Sx.field check "get")
    let expected : Option String := match is with
      | .str s => some s
      | _ => none
    pure (match Sx.JsonCore.get text path with
      | .ok found => found == expected
      | .error _ => false)
  else if (Sx.fieldLookup fields "keys").isSome then
    let path ← pathOf (← Sx.field check "keys")
    let expected ← pathOf is
    pure (match Sx.JsonCore.keys text path with
      | .ok found => found == expected
      | .error _ => false)
  else
    let path ← pathOf (← Sx.field check "kind")
    let expected ← Sx.asStr is
    pure (match Sx.JsonCore.kind text path with
      | .ok found => found == expected
      | .error _ => false)

def jsonSupported (cases : List Json) : Except String (Nat × List String) := do
  let mut total := 0
  let mut wrong : List String := []
  for case in cases do
    let text ← Sx.asStr (← Sx.field case "text")
    for check in ← Sx.asArr (← Sx.field case "checks") do
      total := total + 1
      unless ← jsonCheck text check do
        wrong := s!"{check.compress} in {text.quote}" :: wrong
  pure (total, wrong.reverse)

def jsonRefused (cases : List Json) : Except String (Nat × List String) := do
  let mut total := 0
  let mut wrong : List String := []
  for case in cases do
    let text ← Sx.asStr (← Sx.field case "text")
    let expected := (← Sx.asStr (← Sx.field case "at")) ++ ": " ++ (← Sx.asStr (← Sx.field case "reason"))
    for path in [([] : List String), ["a"]] do
      total := total + 1
      let outcomes : List (Except String String) :=
        [(Sx.JsonCore.get text path).map (fun _ => ""), (Sx.JsonCore.keys text path).map (fun _ => ""),
         (Sx.JsonCore.kind text path).map (fun _ => "")]
      for outcome in outcomes do
        match outcome with
        | .error message => if message != expected then wrong := s!"{text.quote}: {message} != {expected}" :: wrong
        | .ok _ => wrong := s!"{text.quote}: accepted" :: wrong
  pure (total, wrong.reverse)

def main (args : List String) : IO UInt32 := do
  match args with
  | ["decimal-vectors", path] =>
    let doc ← readJson path
    let vectors ← IO.ofExcept (Sx.field doc "vectors" >>= Sx.asArr)
    let mut wrong : List String := []
    for v in vectors do
      match checkVector v with
      | .ok true => pure ()
      | .ok false => wrong := v.compress :: wrong
      | .error e => wrong := s!"{e}: {v.compress}" :: wrong
    for w in wrong.reverse.take 20 do IO.eprintln s!"disagree {w}"
    IO.println (Lean.Json.mkObj [("vectors", vectors.length), ("disagreements", wrong.length)]).compress
    pure (if wrong.isEmpty then 0 else 1)
  | ["json-golden", supportedPath, refusedPath] =>
    let supported ← IO.ofExcept (Sx.asArr (← readJson supportedPath))
    let refused ← IO.ofExcept (Sx.asArr (← readJson refusedPath))
    let (readChecks, readWrong) ← IO.ofExcept (jsonSupported supported)
    let (refusedChecks, refusedWrong) ← IO.ofExcept (jsonRefused refused)
    for w in (readWrong ++ refusedWrong).take 20 do IO.eprintln s!"disagree {w}"
    IO.println (Lean.Json.mkObj [("reads", readChecks), ("refusals", refusedChecks),
      ("disagreements", readWrong.length + refusedWrong.length)]).compress
    pure (if readWrong.isEmpty && refusedWrong.isEmpty then 0 else 1)
  | ["check", irPath] =>
    match Sx.parseProgram (← readJson irPath) >>= Sx.checkProgram with
    | .ok () =>
      IO.println "{\"checked\":true}"
      pure 0
    | .error e =>
      IO.eprintln s!"sx-ref: {e}"
      pure 1
  | ["pure", irPath] =>
    match Sx.parseProgram (← readJson irPath) with
    | .ok prog =>
      if Sx.isPure prog.body then
        let budgets : Sx.Budgets := {}
        let r := Sx.evalP { cores := Sx.coreLibrary, limit := budgets.valueSize } budgets.steps [] prog.body
        IO.println (Lean.Json.mkObj [("outcome", match r with
          | .ok v => Lean.Json.mkObj [("completed", v.encode)]
          | .error f => Lean.Json.mkObj [("failed", f.kind.name)])]).compress
        pure 0
      else
        IO.println "{\"impure\":true}"
        pure 0
    | .error e =>
      IO.eprintln s!"sx-ref: {e}"
      pure 1
  | [irPath, fixturePath] =>
    let ir ← readJson irPath
    let fixture ← readJson fixturePath
    match Sx.parseProgram ir >>= fun prog => Sx.runFixture prog fixture with
    | .ok out =>
      IO.println (Sx.renderJson out)
      pure 0
    | .error e =>
      IO.eprintln s!"sx-ref: {e}"
      pure 1
  | _ =>
    IO.eprintln "usage: sx-ref IR.json FIXTURE.json | sx-ref check IR.json"
    pure 64
