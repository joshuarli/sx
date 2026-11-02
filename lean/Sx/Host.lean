import Sx.Run

namespace Sx

open Lean (Json)

structure Entry where
  cap : String
  input : Json
  results : List Json
  served : Nat

def parseBudgets (j : Json) : Except String Budgets := do
  let get (k : String) (d : Nat) : Except String Nat :=
    match fieldLookup (objFields j) k with
    | some v => asNat v
    | none => pure d
  let b : Budgets := {}
  pure { steps := ← get "steps" b.steps, calls := ← get "calls" b.calls,
         concurrency := ← get "concurrency" b.concurrency, valueSize := ← get "value_size" b.valueSize,
         applications := ← get "applications" b.applications, delayMs := ← get "delay_ms" b.delayMs }

def parseResponse (j : Json) : Except String Response := do
  match fieldLookup (objFields j) "output" with
  | some out => pure (.output out)
  | none =>
    let e ← field j "error"
    pure (.refusal (← asStr (← field e "kind")) (← asStr (← field e "message")))

def answer (prog : Program) (entries : List Entry) (r : Req) : Except String (List Entry × Response) :=
  match r with
  | .delay .. => pure (entries, .output .null)
  | .call _ _ cap input _ =>
    let rec go (before : List Entry) : List Entry → Except String (List Entry × Response)
      | [] => throw s!"{cap} has no fixture for this input: {input.compress}"
      | e :: rest =>
        if e.cap == cap && jsonEq e.input input then
          let write := match fieldLookup prog.capSigs cap with
            | some s => s.effect == .write
            | none => false
          if write && e.served ≥ e.results.length then
            throw s!"{cap} was called more often than the fixture allows: {input.compress}"
          else
            match e.results[min e.served (e.results.length - 1)]? with
            | none => throw s!"{cap} has a fixture with no results"
            | some result => do
              let resp ← parseResponse result
              pure (before.reverse ++ { e with served := e.served + 1 } :: rest, resp)
        else go (e :: before) rest
    go [] entries

def trailingZeros : Nat → Nat → Nat × Nat
  | 0, m => (m, 0)
  | fuel + 1, m => if m ≠ 0 ∧ m % 10 = 0 then let (r, k) := trailingZeros fuel (m / 10); (r, k + 1) else (m, 0)

def renderNumber (n : Lean.JsonNumber) : String :=
  if n.exponent = 0 ∧ n.mantissa.natAbs ≥ 10 ^ 21 then
    let (m, k) := trailingZeros 4000 n.mantissa.natAbs
    (if n.mantissa < 0 then "-" else "") ++ toString m ++ "e" ++ toString k
  else (Json.num n).compress

partial def renderJson : Json → String
  | .num n => renderNumber n
  | .arr items => "[" ++ ",".intercalate (items.toList.map renderJson) ++ "]"
  | .obj kvs =>
    "{" ++ ",".intercalate (kvs.toList.map fun (k, v) => (Json.str k).compress ++ ":" ++ renderJson v) ++ "}"
  | j => j.compress

def eventJson : Event → Json
  | .request id cap input => Json.mkObj [("request", id), ("capability", cap), ("input", input)]
  | .delay id ms => Json.mkObj [("delay", id), ("milliseconds", (ms : Int))]
  | .output id out => Json.mkObj [("response", id), ("output", out)]
  | .refusal id kind message =>
    Json.mkObj [("response", id), ("error", Json.mkObj [("kind", kind), ("message", message)])]

def outcomeJson : Outcome → Json
  | .completed v => Json.mkObj [("completed", v.encode)]
  | .failed f => Json.mkObj [("failed", f.kind.name)]
  | .suspended reqs => Json.mkObj [("suspended", Json.arr (reqs.map fun r => Json.str r.id).toArray)]

def runResponses (prog : Program) (entries : List Entry) (m : Machine) (o : Outcome) :
    Nat → Except String (Machine × Outcome)
  | 0 => throw "the fixture host ran out of rounds"
  | n + 1 =>
    match o with
    | .suspended reqs => do
      let (entries, answers) ← reqs.foldlM (fun (entries, acc) r => do
        let (entries, resp) ← answer prog entries r
        pure (entries, acc ++ [(r.id, resp)])) (entries, [])
      let (m, o) := m.resume answers
      runResponses prog entries m o n
    | _ => pure (m, o)

def runSteps (m : Machine) (o : Outcome) : List Json → Except String (Machine × Outcome)
  | [] => pure (m, o)
  | step :: rest => do
    let wanted ← (← asArr (← field step "requests")).mapM asStr
    match o with
    | .suspended reqs =>
      if reqs.map Req.id != wanted then
        throw s!"the machine suspended with {reqs.map Req.id}, but the fixture expects {wanted}"
      else
        let answers ← (← asArr (← field step "respond")).mapM fun p => do
          let (id, r) ← asPair p
          pure (← asStr id, ← parseResponse r)
        let (m, o) := m.resume answers
        runSteps m o rest
    | _ => throw "the machine finished before the fixture's steps ran out"

def runFixture (prog : Program) (fixture : Json) : Except String Json := do
  let budgets ← match fieldLookup (objFields fixture) "budgets" with
    | some b => parseBudgets b
    | none => pure {}
  let ctx : Ctx := { prog, budgets, cores := fun _ => none }
  let (m, o) := (Machine.init ctx []).run
  let (m, o) ← match fieldLookup (objFields fixture) "steps" with
    | some steps => runSteps m o (← asArr steps)
    | none => do
      let responses ← match fieldLookup (objFields fixture) "responses" with
        | some r => asArr r
        | none => pure []
      let entries ← responses.mapM fun r => do
        pure ({ cap := ← asStr (← field r "capability"), input := ← field r "input",
                results := ← asArr (← field r "results"), served := 0 } : Entry)
      runResponses prog entries m o 10000000
  pure (Json.mkObj [("outcome", outcomeJson o), ("trace", Json.arr (m.trace.map eventJson).toArray)])

end Sx
