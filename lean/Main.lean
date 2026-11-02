import Sx

open Lean (Json)

def readJson (path : String) : IO Json := do
  match Json.parse (← IO.FS.readFile path) with
  | .ok j => pure j
  | .error e => throw (IO.userError s!"{path}: {e}")

def main (args : List String) : IO UInt32 := do
  match args with
  | [irPath, fixturePath] =>
    let ir ← readJson irPath
    let fixture ← readJson fixturePath
    match Sx.parseProgram ir >>= fun prog => Sx.runFixture prog fixture with
    | .ok out =>
      IO.println out.compress
      pure 0
    | .error e =>
      IO.eprintln s!"sx-ref: {e}"
      pure 1
  | _ =>
    IO.eprintln "usage: sx-ref IR.json FIXTURE.json"
    pure 64
