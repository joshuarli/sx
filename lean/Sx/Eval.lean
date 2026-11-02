import Sx.Machine

namespace Sx

mutual
def isPure : Expr → Bool
  | .lit .. => true
  | .var _ => true
  | .let_ _ v body => isPure v && isPure body
  | .localFn _ body => isPure body
  | .if_ c t e => isPure c && isPure t && isPure e
  | .match_ s cs => isPure s && isPureClauses cs
  | .lam _ _ _ body _ => isPure body
  | .applyLocal _ f args => isPure f && isPureList args
  | .prelude _ args => isPureList args
  | .core _ args => isPureList args
  | .get r _ => isPure r
  | .mkRecord fs => isPureFields fs
  | .mkList es _ => isPureList es
  | .mkTuple es => isPureList es
  | .wrap _ e _ => isPure e
  | .mkVariant _ _ none => true
  | .mkVariant _ _ (some e) => isPure e
  | .coerce e _ => isPure e
  | _ => false
def isPureList : List Expr → Bool
  | [] => true
  | e :: es => isPure e && isPureList es
def isPureClauses : List (Pat × Expr) → Bool
  | [] => true
  | (_, e) :: cs => isPure e && isPureClauses cs
def isPureFields : List (String × Expr) → Bool
  | [] => true
  | (_, e) :: fs => isPure e && isPureFields fs
end

structure PureCtx where
  cores : Cores
  limit : Nat

def outOfFuel {α : Type} : Except Failure α := budgetFail "The pure evaluator ran out of fuel (steps budget)."

mutual
def evalP (P : PureCtx) : Nat → Env → Expr → Except Failure Val
  | 0, _, _ => outOfFuel
  | n + 1, ρ, e =>
    match e with
    | .lit v _ => pure v
    | .var x =>
      match ρ.lookup x with
      | some (.val v) => pure v
      | _ => internal s!"{x} is not a value here."
    | .let_ b v body => do
      let x ← evalP P n ρ v
      let ρ' ← bind ρ b x
      evalP P n ρ' body
    | .localFn x body => evalP P n ((x, .scope ρ) :: ρ) body
    | .if_ c t e => do
      let b ← evalP P n ρ c
      match b with
      | .bool true => evalP P n ρ t
      | _ => evalP P n ρ e
    | .match_ s cs => do
      let v ← evalP P n ρ s
      let (body, ρ') ← selectClause ρ v cs
      evalP P n ρ' body
    | .applyLocal x (.lam ps _ _ body _) args => do
      let vs ← evalList P n ρ args
      match ρ.lookup x with
      | some (.scope ρd) => do
        let ρ' ← bindAll ρd ps vs
        evalP P n ρ' body
      | _ => internal s!"{x} is not a local function here."
    | .prelude op args =>
      if op.higherOrder then evalHO P n ρ op args
      else do
        let vs ← evalList P n ρ args
        applyOp P.limit op vs
    | .core f args => do
      let vs ← evalList P n ρ args
      match P.cores f with
      | some impl => do
        let v ← impl vs
        checkSize P.limit v
        pure v
      | none => internal s!"{f} has no implementation."
    | .get r fld => do
      let v ← evalP P n ρ r
      match v with
      | .record fs =>
        match fieldLookup fs fld with
        | some w => pure w
        | none => internal s!"The record has no field {fld}."
      | _ => internal "get reads a field from something that is not a record."
    | .mkRecord fs => do
      let vs ← evalFields P n ρ fs
      let v := Val.record ((fs.map Prod.fst).zip vs)
      checkSize P.limit v
      pure v
    | .mkList es _ => do
      let vs ← evalList P n ρ es
      checkSize P.limit (.list vs)
      pure (.list vs)
    | .mkTuple es => do
      let vs ← evalList P n ρ es
      pure (.tuple vs)
    | .wrap k e _ => do
      let v ← evalP P n ρ e
      pure (match k with
        | .some => .some v
        | .ok => .ok v
        | .err => .err v)
    | .mkVariant en nm none => pure (.variant en nm none)
    | .mkVariant en nm (some p) => do
      let v ← evalP P n ρ p
      pure (.variant en nm (some v))
    | .coerce e c => do
      let v ← evalP P n ρ e
      convert v c
    | _ => internal "The pure evaluator has no rule for an effect."
def evalList (P : PureCtx) : Nat → Env → List Expr → Except Failure (List Val)
  | 0, _, _ => outOfFuel
  | _ + 1, _, [] => pure []
  | n + 1, ρ, e :: es => do
    let v ← evalP P n ρ e
    let vs ← evalList P n ρ es
    pure (v :: vs)
def evalFields (P : PureCtx) : Nat → Env → List (String × Expr) → Except Failure (List Val)
  | 0, _, _ => outOfFuel
  | _ + 1, _, [] => pure []
  | n + 1, ρ, (_, e) :: fs => do
    let v ← evalP P n ρ e
    let vs ← evalFields P n ρ fs
    pure (v :: vs)
def evalHO (P : PureCtx) : Nat → Env → Op → List Expr → Except Failure Val
  | 0, _, _, _ => outOfFuel
  | n + 1, ρ, op, args =>
    match op, args with
    | .reduce, [f, init, xs] => do
      let c ← closureOf ρ f
      let a ← evalP P n ρ init
      let items ← evalP P n ρ xs
      match items with
      | .list items => foldClo P n c a items
      | _ => internal "reduce got something that is not a list."
    | .any, [f, xs] => do
      let c ← closureOf ρ f
      let items ← evalP P n ρ xs
      match items with
      | .list items => anyClo P n c true items
      | _ => internal "any? got something that is not a list."
    | .all, [f, xs] => do
      let c ← closureOf ρ f
      let items ← evalP P n ρ xs
      match items with
      | .list items => anyClo P n c false items
      | _ => internal "all? got something that is not a list."
    | op, [f, xs] => do
      let c ← closureOf ρ f
      let items ← evalP P n ρ xs
      match items with
      | .list items => do
        let rs ← mapClo P n c items
        finishEach op items rs none
      | _ => internal "A list function got something that is not a list."
    | _, _ => internal "A list function got the wrong arguments."
def applyClo (P : PureCtx) : Nat → Clo → List Val → Except Failure Val
  | 0, _, _ => outOfFuel
  | n + 1, c, args => do
    let ρ' ← bindAll c.env c.params args
    evalP P n ρ' c.body
def mapClo (P : PureCtx) : Nat → Clo → List Val → Except Failure (List Val)
  | 0, _, _ => outOfFuel
  | _ + 1, _, [] => pure []
  | n + 1, c, x :: xs => do
    let r ← applyClo P n c [x]
    let rs ← mapClo P n c xs
    pure (r :: rs)
def anyClo (P : PureCtx) : Nat → Clo → Bool → List Val → Except Failure Val
  | 0, _, _, _ => outOfFuel
  | _ + 1, _, stop, [] => pure (.bool !stop)
  | n + 1, c, stop, x :: xs => do
    let r ← applyClo P n c [x]
    if r == .bool stop then pure (.bool stop) else anyClo P n c stop xs
def foldClo (P : PureCtx) : Nat → Clo → Val → List Val → Except Failure Val
  | 0, _, _, _ => outOfFuel
  | _ + 1, _, a, [] => pure a
  | n + 1, c, a, x :: xs => do
    let a' ← applyClo P n c [a, x]
    foldClo P n c a' xs
end

end Sx
