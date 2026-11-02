import Sx.Machine

namespace Sx

open Lean (Json)

structure Outstanding where
  id : String
  fiber : Nat
  path : List Nat
  seq : Nat

structure Machine where
  ctx : Ctx
  fibers : List Fiber
  glob : Glob
  outstanding : List Outstanding
  trace : List Event

inductive Outcome where
  | completed (v : Val)
  | suspended (reqs : List Req)
  | failed (f : Failure)

inductive Response where
  | output (j : Json)
  | refusal (kind message : String)

def Machine.init (ctx : Ctx) (inputs : List (String × Val)) : Machine :=
  let env : Env := inputs.foldl (fun env (n, v) => (n, .val v) :: env) []
  { ctx
    fibers := [{ id := 0, path := [], parent := none, stack := [], ctrl := .eval ctx.prog.body env, cancelled := false }]
    glob := { counters := {}, nextFiber := 1, sequences := [] }
    outstanding := []
    trace := [] }

def Machine.fiber? (m : Machine) (id : Nat) : Option Fiber :=
  m.fibers.find? (·.id == id)

def Machine.setFiber (m : Machine) (f : Fiber) : Machine :=
  { m with fibers := m.fibers.map fun g => if g.id == f.id then f else g }

def recordChild (index : Nat) (outcome : Sum Val Raised) : List Frame → List Frame
  | .join j :: rest =>
    match outcome with
    | .inl v => .join { j with results := j.results ++ [(index, v)] } :: rest
    | .inr e => .join { j with failures := j.failures ++ [(index, e)] } :: rest
  | stack => stack

def Machine.notifyParent (m : Machine) (f : Fiber) : Machine :=
  match f.parent, f.ctrl with
  | some (pid, i), .done v =>
    match m.fiber? pid with
    | some p => m.setFiber { p with stack := recordChild i (.inl v) p.stack }
    | none => m
  | some (pid, i), .failed e =>
    match m.fiber? pid with
    | some p => m.setFiber { p with stack := recordChild i (.inr e) p.stack }
    | none => m
  | _, _ => m

def Machine.stepOne (m : Machine) (f : Fiber) : Except Failure Machine := do
  let (f', children, g) ← stepFiber m.ctx m.glob f
  let m := { m.setFiber f' with glob := g }
  let m := { m with fibers := m.fibers ++ children }
  pure (m.notifyParent f')

def Machine.runFiber (m : Machine) (id : Nat) : Nat → Except Failure Machine
  | 0 => internal "The machine ran out of fuel."
  | fuel + 1 =>
    match m.fiber? id with
    | some f =>
      if f.runnable then do
        let m ← m.stepOne f
        m.runFiber id fuel
      else pure m
    | none => pure m

def pathLt : List Nat → List Nat → Bool
  | [], [] => false
  | [], _ :: _ => true
  | _ :: _, [] => false
  | a :: as, b :: bs => a < b || (a == b && pathLt as bs)

def insertBy {α : Type} (lt : α → α → Bool) (x : α) : List α → List α
  | [] => [x]
  | y :: ys => if lt x y then x :: y :: ys else y :: insertBy lt x ys

def sortBy {α : Type} (lt : α → α → Bool) (xs : List α) : List α :=
  xs.foldl (fun acc x => insertBy lt x acc) []

def Machine.pass (m : Machine) (fuel : Nat) : Except Failure Machine :=
  let order := (sortBy (fun a b => pathLt a.path b.path) m.fibers).map (·.id)
  order.foldlM (fun m id => m.runFiber id fuel) m

def Machine.cancel (m : Machine) (id : Nat) : Nat → Machine
  | 0 => m
  | fuel + 1 =>
    match m.fiber? id with
    | none => m
    | some f =>
      let m := m.setFiber { f with cancelled := true }
      let children := f.stack.foldr (fun fr acc =>
        match fr with
        | .join j => j.children ++ acc
        | _ => acc) []
      children.foldl (fun m c => m.cancel c fuel) m

def minFailure {α : Type} : List (Nat × α) → Option (Nat × α)
  | [] => none
  | p :: ps =>
    match minFailure ps with
    | some q => if q.1 < p.1 then some q else some p
    | none => some p

def assemble (count : Nat) (results : List (Nat × Val)) : Except Failure (List Val) :=
  (List.range count).mapM fun i =>
    match fieldLookup results i with
    | some v => pure v
    | none => internal "A parallel branch has no result."

def Machine.settleOne (m : Machine) (id : Nat) : Except Failure Machine := do
  match m.fiber? id with
  | some f =>
    match f.ctrl, f.stack, f.cancelled with
    | .join, .join j :: rest, false =>
      match minFailure j.failures with
      | some (_, raised) =>
        let m := j.children.foldl (fun m c => m.cancel c (m.fibers.length + 1)) m
        match m.fiber? id with
        | some f => pure (m.setFiber { f with stack := rest, ctrl := .raise raised })
        | none => pure m
      | none => do
        let (j, children, g) ← match j.clo with
          | some c => startChildren m.ctx m.glob f j c (j.count + 1)
          | none => pure (j, [], m.glob)
        let m := { m with fibers := m.fibers ++ children, glob := g }
        if j.results.length == j.count then do
          let vs ← assemble j.count j.results
          let v := if j.asTuple then Val.tuple vs else Val.list vs
          pure (m.setFiber { f with stack := rest, ctrl := .ret (.val v) })
        else pure (m.setFiber { f with stack := .join j :: rest })
    | _, _, _ => pure m
  | none => pure m

def Machine.settle (m : Machine) : Except Failure Machine :=
  (m.fibers.map (·.id)).foldlM (fun m id => m.settleOne id) m

def reqLt (a b : Req) : Bool :=
  pathLt a.path b.path || (a.path == b.path && a.seq < b.seq)

def Machine.issue (m : Machine) : Machine × List Req :=
  let waiting := m.fibers.filterMap fun f =>
    match f.ctrl with
    | .wait r false => if f.cancelled then none else some (f, r)
    | _ => none
  let chosen := (sortBy (fun a b => reqLt a.2 b.2) waiting).take m.ctx.budgets.concurrency
  chosen.foldl (fun (m, reqs) (f, r) =>
    let event := match r with
      | .call _ _ cap input _ => Event.request r.id cap input
      | .delay _ _ ms => Event.delay r.id ms
    let m := m.setFiber { f with ctrl := .wait r true }
    ({ m with outstanding := m.outstanding ++ [⟨r.id, f.id, r.path, r.seq⟩], trace := m.trace ++ [event] },
      reqs ++ [r])) (m, [])

def Machine.complete (m : Machine) (v : Val) : Outcome :=
  match checkSize m.ctx.budgets.valueSize v with
  | .ok _ => .completed v
  | .error f => .failed f

def failureMessage (err : Val) : String :=
  match err with
  | .record fs =>
    match fieldLookup fs "message" with
    | some (.str s) => s
    | _ => ""
  | _ => ""

def Machine.loop (m : Machine) (fuel : Nat) : Nat → Machine × Outcome
  | 0 => (m, .failed ⟨.internal, "The machine ran out of fuel."⟩)
  | n + 1 =>
    match m.pass fuel >>= Machine.settle with
    | .error f => (m, .failed f)
    | .ok m' =>
      match m'.fiber? 0 with
      | some { ctrl := .done v, .. } => (m', m'.complete v)
      | some { ctrl := .failed raised, .. } =>
        match raised.cause with
        | .call =>
          (m', .failed ⟨.callFailed, s!"A tool call failed with {kindOf raised.err}: {failureMessage raised.err}"⟩)
        | .coreRefusal => (m', .failed ⟨.invalidInput, failureMessage raised.err⟩)
        | .thrown => (m', .failed ⟨.thrown, failureMessage raised.err⟩)
      | _ =>
        if m'.fibers.any (·.runnable) then m'.loop fuel n
        else
          let (m'', reqs) := m'.issue
          if reqs.isEmpty then (m'', .failed ⟨.internal, "No fiber can run and no request is outstanding."⟩)
          else (m'', .suspended reqs)

def Machine.fuel (m : Machine) : Nat := m.ctx.budgets.steps + 2

def Machine.run (m : Machine) : Machine × Outcome :=
  m.loop m.fuel (2 * m.fuel)

def invalid {α : Type} (message : String) : Except Failure α := failWith .invalidHostResponse message

def Machine.deliver (m : Machine) (o : Outstanding) (resp : Response) : Except Failure Machine := do
  match m.fiber? o.fiber with
  | none => internal "A response has no fiber."
  | some f =>
    if f.cancelled then pure m
    else
      match f.ctrl, resp with
      | .wait (.delay ..) _, .output .null => pure (m.setFiber { f with ctrl := .ret (.val .unit) })
      | .wait (.delay ..) _, _ => invalid s!"Delay request {o.id} must be answered with null."
      | .wait (.call _ _ _ _ _) _, .refusal kind message =>
        if callErrorKinds.contains kind then pure (m.setFiber { f with ctrl := .raise ⟨callError kind message, .call⟩ })
        else invalid s!"{o.id}: {kind} is not a call error kind."
      | .wait (.call _ _ cap _ out) _, .output j =>
        match decode m.ctx.prog.decodeEnums out j "$" with
        | .error e => invalid s!"{o.id} ({cap}): {e}"
        | .ok v => do
          checkSize m.ctx.budgets.valueSize v
          pure (m.setFiber { f with ctrl := .ret (.val v) })
      | _, _ => internal "A response reached a fiber that is not waiting."

def responseEvent (id : String) : Response → Event
  | .output j => .output id j
  | .refusal kind message => .refusal id kind message

def outstandingLt (a b : Outstanding) : Bool :=
  pathLt a.path b.path || (a.path == b.path && a.seq < b.seq)

def Machine.validate (m : Machine) (responses : List (String × Response)) : Option Failure :=
  let rec go (seen : List String) : List (String × Response) → Option Failure
    | [] =>
      let missing := sortBy outstandingLt (m.outstanding.filter fun o => !seen.contains o.id)
      match missing with
      | o :: _ => some ⟨.invalidHostResponse, s!"The host did not answer request {o.id}."⟩
      | [] => none
    | (id, _) :: rest =>
      if seen.contains id then some ⟨.invalidHostResponse, s!"The host answered request {id} twice."⟩
      else if !(m.outstanding.any (·.id == id)) then
        some ⟨.invalidHostResponse, s!"The host answered {id}, which is not an outstanding request."⟩
      else go (id :: seen) rest
  go [] responses

def Machine.resume (m : Machine) (responses : List (String × Response)) : Machine × Outcome :=
  match m.validate responses with
  | some f => (m, .failed f)
  | none =>
    let ordered := sortBy outstandingLt m.outstanding
    let rec deliverAll (m : Machine) : List Outstanding → Machine × Option Failure
      | [] => (m, none)
      | o :: os =>
        match fieldLookup responses o.id with
        | none => (m, some ⟨.internal, "A validated response went missing."⟩)
        | some resp =>
          let m := { m with trace := m.trace ++ [responseEvent o.id resp] }
          match m.deliver o resp with
          | .error f => (m, some f)
          | .ok m => deliverAll m os
    match deliverAll { m with outstanding := [] } ordered with
    | (m, some f) => (m, .failed f)
    | (m, none) => m.run

end Sx
