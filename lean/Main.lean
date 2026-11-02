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
