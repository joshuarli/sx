import Sx.Host
import Sx.Check
import Sx.Eval

open Lean (Json)

def readJson (path : String) : IO Json := do
  match Json.parse (← IO.FS.readFile path) with
  | .ok j => pure j
  | .error e => throw (IO.userError s!"{path}: {e}")

def main (args : List String) : IO UInt32 := do
  match args with
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
        let r := Sx.evalP { cores := fun _ => none, limit := budgets.valueSize } budgets.steps [] prog.body
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
