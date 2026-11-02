import Sx.Json

namespace Sx

open Lean (Json)

structure Clo where
  params : List Binder
  body : Expr
  env : Env
  deriving Inhabited

inductive Res where
  | val (v : Val)
  | clo (c : Clo)
  | recur (vs : List Val)
  deriving Inhabited

inductive Cause where
  | call
  | coreRefusal
  deriving Repr, BEq, DecidableEq, Inhabited

structure Raised where
  err : Val
  cause : Cause
  deriving Inhabited

structure Join where
  asTuple : Bool
  count : Nat
  clo : Option Clo
  items : List Val
  limit : Nat
  started : Nat
  results : List (Nat × Val)
  failures : List (Nat × Raised)
  children : List Nat
  deriving Inhabited

inductive Frame where
  | seq (origin : Expr) (env : Env) (done : List Res) (rest : List Expr)
  | let_ (b : Binder) (body : Expr) (env : Env)
  | if_ (t e : Expr) (env : Env)
  | match_ (cs : List (Pat × Expr)) (env : Env)
  | attempt
  | retry (delays : List Nat) (kinds : List String) (body : Expr) (env : Env) (attempt : Nat)
  | wait (intervals : List Nat) (pred : Clo) (body : Expr) (env : Env) (round : Nat)
  | waitCheck (intervals : List Nat) (pred : Clo) (body : Expr) (env : Env) (round : Nat) (v : Val)
  | loop (max : Nat) (bs : List Binder) (body : Expr) (env : Env) (iter : Nat)
  | each (op : Op) (clo : Clo) (items : List Val) (index : Nat) (acc : Option Val) (results : List Val)
  | then_ (e : Expr) (env : Env)
  | join (j : Join)
  deriving Inhabited

inductive Req where
  | call (path : List Nat) (seq : Nat) (cap : String) (input : Json) (output : DTy)
  | delay (path : List Nat) (seq : Nat) (ms : Nat)
  deriving Inhabited

def pathText (path : List Nat) : String :=
  "r" ++ String.join (path.map fun i => s!".{i}")

def Req.path : Req → List Nat
  | .call p .. => p
  | .delay p .. => p

def Req.seq : Req → Nat
  | .call _ s .. => s
  | .delay _ s _ => s

def Req.id (r : Req) : String := s!"{pathText r.path}:{r.seq}"

inductive Ctrl where
  | eval (e : Expr) (env : Env)
  | ret (r : Res)
  | raise (r : Raised)
  | wait (req : Req) (issued : Bool)
  | join
  | done (v : Val)
  | failed (r : Raised)
  deriving Inhabited

structure Fiber where
  id : Nat
  path : List Nat
  parent : Option (Nat × Nat)
  stack : List Frame
  ctrl : Ctrl
  cancelled : Bool
  deriving Inhabited

def Fiber.runnable (f : Fiber) : Bool :=
  !f.cancelled &&
    match f.ctrl with
    | .eval .. | .ret _ | .raise _ => true
    | _ => false

inductive Event where
  | request (id cap : String) (input : Json)
  | delay (id : String) (ms : Nat)
  | output (id : String) (out : Json)
  | refusal (id kind message : String)

structure Budgets where
  steps : Nat := 2000000
  calls : Nat := 1000
  concurrency : Nat := 16
  valueSize : Nat := 1000000
  applications : Nat := 200000
  delayMs : Nat := 3600000

structure Counters where
  steps : Nat := 0
  calls : Nat := 0
  applications : Nat := 0
  delay : Nat := 0

abbrev Cores := String → Option (List Val → Except Failure Val)

structure Glob where
  counters : Counters
  nextFiber : Nat
  sequences : List (List Nat × Nat)

structure Ctx where
  prog : Program
  budgets : Budgets
  cores : Cores

def budgetFail {α : Type} (message : String) : Except Failure α := failWith .budgetExhausted message

def tick (ctx : Ctx) (g : Glob) : Except Failure Glob :=
  let c := g.counters
  if c.steps + 1 > ctx.budgets.steps then
    budgetFail s!"The program ran more than {ctx.budgets.steps} steps (steps budget)."
  else pure { g with counters := { c with steps := c.steps + 1 } }

def countCall (ctx : Ctx) (g : Glob) : Except Failure Glob :=
  let c := g.counters
  if c.calls + 1 > ctx.budgets.calls then
    budgetFail s!"The program made more than {ctx.budgets.calls} tool calls (calls budget)."
  else pure { g with counters := { c with calls := c.calls + 1 } }

def countApplication (ctx : Ctx) (g : Glob) : Except Failure Glob :=
  let c := g.counters
  if c.applications + 1 > ctx.budgets.applications then
    budgetFail s!"The program applied functions more than {ctx.budgets.applications} times (applications budget)."
  else pure { g with counters := { c with applications := c.applications + 1 } }

def countDelay (ctx : Ctx) (g : Glob) (ms : Nat) : Except Failure Glob :=
  let c := g.counters
  if c.delay + ms > ctx.budgets.delayMs then
    budgetFail s!"The program asked for more than {ctx.budgets.delayMs} ms of delays (delay budget)."
  else pure { g with counters := { c with delay := c.delay + ms } }

def nextSeq (g : Glob) (path : List Nat) : Glob × Nat :=
  let n := (fieldLookup g.sequences path).getD 0 + 1
  ({ g with sequences := (path, n) :: g.sequences.filter (·.1 != path) }, n)

def plain : Res → Except Failure Val
  | .val v => pure v
  | _ => internal "A function or recur reached a value position."

def plainAll : List Res → Except Failure (List Val)
  | [] => pure []
  | r :: rs => do
    let v ← plain r
    let vs ← plainAll rs
    pure (v :: vs)

def closureOf (env : Env) : Expr → Except Failure Clo
  | .lam ps _ _ body none => pure ⟨ps, body, env⟩
  | .lam ps _ _ body (some n) =>
    match env.lookup n with
    | some (.scope defEnv) => pure ⟨ps, body, defEnv⟩
    | _ => internal s!"{n} is not a local function here."
  | _ => internal "A function position does not hold a function."

def apply (ctx : Ctx) (g : Glob) (c : Clo) (args : List Val) : Except Failure (Glob × Ctrl) := do
  let g ← countApplication ctx g
  let env ← bindAll c.env c.params args
  pure (g, .eval c.body env)

def seqChildren : Expr → Except Failure (List Expr)
  | .prelude _ args => pure args
  | .core _ args => pure args
  | .applyLocal _ _ args => pure args
  | .recur args => pure args
  | .call _ arg => pure [arg]
  | .get r _ => pure [r]
  | .tupleItem e i => pure [e, .lit (.int i) .int]
  | .mkRecord fs => pure (fs.map Prod.snd)
  | .mkList es _ => pure es
  | .mkTuple es => pure es
  | .wrap _ e _ => pure [e]
  | .coerce e _ => pure [e]
  | .mkVariant _ _ none => pure []
  | .mkVariant _ _ (some e) => pure [e]
  | .loop _ _ init _ => pure init
  | .parMap _ f items => pure [f, items]
  | _ => internal "No evaluation rule for this node."

def kindOf (err : Val) : String :=
  match err with
  | .record fs =>
    match fieldLookup fs "kind" with
    | some (.variant _ k _) => k
    | _ => ""
  | _ => ""

def requestDelay (ctx : Ctx) (g : Glob) (f : Fiber) (ms : Nat) (stack : List Frame) :
    Except Failure (Fiber × List Fiber × Glob) := do
  let g ← countDelay ctx g ms
  let (g, n) := nextSeq g f.path
  pure ({ f with stack, ctrl := .wait (.delay f.path n ms) false }, [], g)

def unwind (ctx : Ctx) (g : Glob) (f : Fiber) (raised : Raised) :
    List Frame → Except Failure (Fiber × List Fiber × Glob)
  | [] => pure ({ f with stack := [], ctrl := .failed raised }, [], g)
  | .attempt :: rest => pure ({ f with stack := rest, ctrl := .ret (.val (.err raised.err)) }, [], g)
  | .retry delays kinds body env n :: rest =>
    if kinds.contains (kindOf raised.err) then
      match delays[n]? with
      | some ms => requestDelay ctx g f ms (.then_ body env :: .retry delays kinds body env (n + 1) :: rest)
      | none => unwind ctx g f raised rest
    else unwind ctx g f raised rest
  | .join _ :: _ => internal "A failure reached a join frame."
  | _ :: rest => unwind ctx g f raised rest

def startChildren (ctx : Ctx) (g : Glob) (f : Fiber) (j : Join) (c : Clo) :
    Nat → Except Failure (Join × List Fiber × Glob)
  | 0 => pure (j, [], g)
  | fuel + 1 =>
    let active := j.started - j.results.length - j.failures.length
    if j.started < j.count ∧ active < j.limit then
      match j.items[j.started]? with
      | none => internal "par-map lost an item."
      | some item => do
        let (g, ctrl) ← apply ctx g c [item]
        let child : Fiber :=
          { id := g.nextFiber, path := f.path ++ [j.started], parent := some (f.id, j.started),
            stack := [], ctrl, cancelled := false }
        let g := { g with nextFiber := g.nextFiber + 1 }
        let j := { j with started := j.started + 1, children := j.children ++ [child.id] }
        let (j, more, g) ← startChildren ctx g f j c fuel
        pure (j, child :: more, g)
    else pure (j, [], g)

def finishSeq (ctx : Ctx) (g : Glob) (f : Fiber) (stack : List Frame) (origin : Expr) (env : Env)
    (done : List Res) : Except Failure (Fiber × List Fiber × Glob) := do
  let ret (v : Val) : Except Failure (Fiber × List Fiber × Glob) := pure ({ f with stack, ctrl := .ret (.val v) }, [], g)
  match origin with
  | .prelude op _ =>
    if op.higherOrder then
      match done with
      | .clo c :: rest =>
        let (acc, itemsRes) := if op == .reduce then (rest.head?, rest.drop 1) else (none, rest)
        let acc ← match acc with
          | some r => some <$> plain r
          | none => pure none
        match itemsRes with
        | [.val (.list items)] =>
          match items with
          | [] => do ret (← finishEach op [] [] acc)
          | item :: _ => do
            let args := if op == .reduce then (acc.toList ++ [item]) else [item]
            let (g, ctrl) ← apply ctx g c args
            pure ({ f with stack := .each op c items 0 acc [] :: stack, ctrl }, [], g)
        | _ => internal "A list function got something that is not a list."
      | _ => internal "A list function got something that is not a function."
    else do
      let vs ← plainAll done
      ret (← applyOp ctx.budgets.valueSize op vs)
  | .core name _ =>
    match ctx.cores name with
    | some impl => do
      let vs ← plainAll done
      match impl vs with
      | .ok v => do
        checkSize ctx.budgets.valueSize v
        ret v
      | .error ⟨.invalidInput, message⟩ =>
        let refusal : Raised := ⟨callError "Invalid" s!"{name}: {message}", .coreRefusal⟩
        pure ({ f with stack, ctrl := .raise refusal }, [], g)
      | .error failure => .error failure
    | none => internal s!"{name} has a signature but no implementation."
  | .call cap _ => do
    let g ← countCall ctx g
    let arg ← match done with
      | [r] => plain r
      | _ => internal "A call has one argument."
    let output ← match fieldLookup ctx.prog.capOutputs cap with
      | some o => pure o
      | none => internal s!"{cap} has no signature."
    let (g, n) := nextSeq g f.path
    pure ({ f with stack, ctrl := .wait (.call f.path n cap arg.encode output) false }, [], g)
  | .get _ name =>
    match done with
    | [.val (.record fs)] =>
      match fieldLookup fs name with
      | some v => ret v
      | none => internal s!"The record has no field {name}."
    | _ => internal "get reads a field from something that is not a record."
  | .tupleItem _ i =>
    match done with
    | [.val (.tuple vs), .val (.int _)] =>
      match vs[i]? with
      | some v => ret v
      | none => internal "tuple-item is past the end of the tuple."
    | _ => internal "tuple-item reads from something that is not a tuple."
  | .mkRecord fs => do
    let vs ← plainAll done
    let v := Val.record (sortFields ((fs.map Prod.fst).zip vs))
    checkSize ctx.budgets.valueSize v
    ret v
  | .mkList _ _ => do
    let v := Val.list (← plainAll done)
    checkSize ctx.budgets.valueSize v
    ret v
  | .mkTuple _ => do ret (.tuple (← plainAll done))
  | .wrap k _ _ => do
    let v ← match done with
      | [r] => plain r
      | _ => internal "A wrapper has one value."
    ret (match k with
      | .some => .some v
      | .ok => .ok v
      | .err => .err v)
  | .mkVariant e n _ => do
    let vs ← plainAll done
    ret (.variant e n vs.head?)
  | .coerce _ c => do
    let v ← match done with
      | [r] => plain r
      | _ => internal "A conversion has one value."
    ret (← convert v c)
  | .applyLocal name fn _ =>
    match fn, env.lookup name with
    | .lam ps _ _ body _, some (.scope defEnv) => do
      let vs ← plainAll done
      let (g, ctrl) ← apply ctx g ⟨ps, body, defEnv⟩ vs
      pure ({ f with stack, ctrl }, [], g)
    | _, _ => internal s!"{name} is not a local function here."
  | .recur _ => do
    let vs ← plainAll done
    pure ({ f with stack, ctrl := .ret (.recur vs) }, [], g)
  | .loop max bs _ body => do
    let vs ← plainAll done
    let env' ← bindAll env bs vs
    pure ({ f with stack := .loop max bs body env 1 :: stack, ctrl := .eval body env' }, [], g)
  | .parMap limit _ _ =>
    match done with
    | [.clo c, .val (.list items)] => do
      let j : Join :=
        { asTuple := false, count := items.length, clo := some c, items, limit, started := 0,
          results := [], failures := [], children := [] }
      let (j, children, g) ← startChildren ctx g f j c (items.length + 1)
      pure ({ f with stack := .join j :: stack, ctrl := .join }, children, g)
    | _ => internal "par-map got something that is not a function and a list."
  | _ => internal "No evaluation rule for this node."

def evalNode (ctx : Ctx) (g : Glob) (f : Fiber) (e : Expr) (env : Env) :
    Except Failure (Fiber × List Fiber × Glob) :=
  let stack := f.stack
  let go (fr : Frame) (next : Expr) : Except Failure (Fiber × List Fiber × Glob) :=
    pure ({ f with stack := fr :: stack, ctrl := .eval next env }, [], g)
  match e with
  | .lit v _ => pure ({ f with ctrl := .ret (.val v) }, [], g)
  | .var n =>
    match env.lookup n with
    | some (.val v) => pure ({ f with ctrl := .ret (.val v) }, [], g)
    | some (.scope _) => internal s!"{n} is a local function used as a value."
    | none => internal s!"{n} is not bound."
  | .lam .. => do
    let c ← closureOf env e
    pure ({ f with ctrl := .ret (.clo c) }, [], g)
  | .let_ b v body => go (.let_ b body env) v
  | .localFn n body => pure ({ f with ctrl := .eval body ((n, .scope env) :: env) }, [], g)
  | .if_ c t e' => go (.if_ t e' env) c
  | .match_ s cs => go (.match_ cs env) s
  | .attempt body => go .attempt body
  | .retry delays kinds body => go (.retry delays kinds body env 0) body
  | .sleep 0 => pure ({ f with ctrl := .ret (.val .unit) }, [], g)
  | .sleep ms => requestDelay ctx g f ms stack
  | .waitUntil intervals pred body => do
    let c ← closureOf env pred
    go (.wait intervals c body env 0) body
  | .par bs =>
    let ids := (List.range bs.length).map (g.nextFiber + ·)
    let children := (bs.zipIdx).map fun (b, i) =>
      ({ id := g.nextFiber + i, path := f.path ++ [i], parent := some (f.id, i), stack := [],
         ctrl := .eval b env, cancelled := false } : Fiber)
    let j : Join :=
      { asTuple := true, count := bs.length, clo := none, items := [], limit := 0, started := bs.length,
        results := [], failures := [], children := ids }
    pure ({ f with stack := .join j :: stack, ctrl := .join }, children,
      { g with nextFiber := g.nextFiber + bs.length })
  | _ => do
    match ← seqChildren e with
    | [] => finishSeq ctx g f stack e env []
    | c :: rest => go (.seq e env [] rest) c

def selectClause (env : Env) (v : Val) : List (Pat × Expr) → Except Failure (Expr × Env)
  | [] => internal "No match clause matched."
  | (p, body) :: cs => do
    match ← matchPat env p v with
    | some env' => pure (body, env')
    | none => selectClause env v cs

def resumeFrame (ctx : Ctx) (g : Glob) (f : Fiber) (fr : Frame) (stack : List Frame) (r : Res) :
    Except Failure (Fiber × List Fiber × Glob) :=
  let ret (r : Res) : Except Failure (Fiber × List Fiber × Glob) := pure ({ f with stack, ctrl := .ret r }, [], g)
  let eval (e : Expr) (env : Env) : Except Failure (Fiber × List Fiber × Glob) :=
    pure ({ f with stack, ctrl := .eval e env }, [], g)
  match r, fr with
  | .recur vs, .loop max bs body env iter =>
    if iter ≥ max then
      failWith .loopLimit s!"This loop asked to recur after {max} iterations, its :max."
    else do
      let env' ← bindAll env bs vs
      pure ({ f with stack := .loop max bs body env (iter + 1) :: stack, ctrl := .eval body env' }, [], g)
  | .recur _, _ => ret r
  | _, .seq origin env done rest =>
    match rest with
    | next :: rest' => pure ({ f with stack := .seq origin env (done ++ [r]) rest' :: stack, ctrl := .eval next env }, [], g)
    | [] => finishSeq ctx g f stack origin env (done ++ [r])
  | _, .let_ b body env => do
    let v ← plain r
    eval body (← bind env b v)
  | _, .if_ t e env =>
    match r with
    | .val (.bool true) => eval t env
    | _ => eval e env
  | _, .match_ cs env => do
    let (body, env') ← selectClause env (← plain r) cs
    eval body env'
  | _, .attempt => do ret (.val (.ok (← plain r)))
  | _, .retry .. => ret r
  | _, .loop .. => ret r
  | _, .wait intervals pred body env round => do
    let v ← plain r
    let (g, ctrl) ← apply ctx g pred [v]
    pure ({ f with stack := .waitCheck intervals pred body env round v :: stack, ctrl }, [], g)
  | _, .waitCheck intervals pred body env round v =>
    match r with
    | .val (.bool true) => ret (.val v)
    | _ =>
      match intervals[round]? with
      | none =>
        let timeout := callError "Timeout" s!"wait-until gave up after {intervals.sum} ms."
        pure ({ f with stack, ctrl := .raise ⟨timeout, .call⟩ }, [], g)
      | some ms => requestDelay ctx g f ms (.then_ body env :: .wait intervals pred body env (round + 1) :: stack)
  | _, .each op c items index acc results => do
    let v ← plain r
    let (acc, results) := if op == .reduce then (some v, results) else (acc, results ++ [v])
    if (op == .any && v == .bool true) || (op == .all && v == .bool false) then
      ret (.val (.bool (op == .any)))
    else
      match items[index + 1]? with
      | some item => do
        let args := if op == .reduce then (acc.toList ++ [item]) else [item]
        let (g, ctrl) ← apply ctx g c args
        pure ({ f with stack := .each op c items (index + 1) acc results :: stack, ctrl }, [], g)
      | none => do ret (.val (← finishEach op items results acc))
  | _, .then_ e env => eval e env
  | _, .join _ => internal "A join frame received a value directly."

def stepFiber (ctx : Ctx) (g : Glob) (f : Fiber) : Except Failure (Fiber × List Fiber × Glob) := do
  let g ← tick ctx g
  match f.ctrl with
  | .raise raised => unwind ctx g f raised f.stack
  | .eval e env => evalNode ctx g f e env
  | .ret r =>
    match f.stack with
    | [] =>
      match r with
      | .val v => pure ({ f with ctrl := .done v }, [], g)
      | _ => internal "A fiber ended with a function or a recur."
    | fr :: stack => resumeFrame ctx g f fr stack r
  | _ => internal "A fiber that cannot run was stepped."

end Sx
