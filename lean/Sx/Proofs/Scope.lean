import Sx.Proofs.Effect

namespace Sx

mutual
def literalOf : Expr → Option Val
  | .lit (.bool b) _ => some (.bool b)
  | .lit (.int i) _ => some (.int i)
  | .lit (.float x) _ => some (.float x)
  | .lit (.str s) _ => some (.str s)
  | .lit Val.none _ => some Val.none
  | .wrap .some e _ => (literalOf e).map Val.some
  | .mkRecord fs => (literalFields fs).map .record
  | .mkList es _ => (literalList es).map .list
  | _ => none
def literalFields : List (String × Expr) → Option (List (String × Val))
  | [] => some []
  | (n, e) :: fs =>
    match literalOf e, literalFields fs with
    | some v, some rest => some ((n, v) :: rest)
    | _, _ => none
def literalList : List Expr → Option (List Val)
  | [] => some []
  | e :: es =>
    match literalOf e, literalList es with
    | some v, some rest => some (v :: rest)
    | _, _ => none
end

mutual
def knownCalls : Expr → List (String × Val)
  | .lit .. => []
  | .var _ => []
  | .let_ _ v body => knownCalls v ++ knownCalls body
  | .localFn _ body => knownCalls body
  | .if_ c t e => knownCalls c ++ knownCalls t ++ knownCalls e
  | .match_ s cs => knownCalls s ++ knownCallsClauses cs
  | .lam _ _ _ body _ => knownCalls body
  | .applyLocal _ f args => knownCalls f ++ knownCallsList args
  | .prelude _ args => knownCallsList args
  | .core _ args => knownCallsList args
  | .call c arg => (match literalOf arg with | some v => [(c, v)] | none => []) ++ knownCalls arg
  | .get r _ => knownCalls r
  | .tupleItem e _ => knownCalls e
  | .mkRecord fs => knownCallsFields fs
  | .mkList es _ => knownCallsList es
  | .mkTuple es => knownCallsList es
  | .wrap _ e _ => knownCalls e
  | .mkVariant _ _ none => []
  | .mkVariant _ _ (some e) => knownCalls e
  | .coerce e _ => knownCalls e
  | .attempt e => knownCalls e
  | .fail m => knownCalls m
  | .log args => knownCallsList args
  | .retry _ _ e => knownCalls e
  | .waitUntil _ p e => knownCalls p ++ knownCalls e
  | .sleep _ => []
  | .loop _ _ init body => knownCallsList init ++ knownCalls body
  | .recur args => knownCallsList args
  | .par bs => knownCallsList bs
  | .parMap _ f xs => knownCalls f ++ knownCalls xs
def knownCallsList : List Expr → List (String × Val)
  | [] => []
  | e :: es => knownCalls e ++ knownCallsList es
def knownCallsClauses : List (Pat × Expr) → List (String × Val)
  | [] => []
  | (_, e) :: cs => knownCalls e ++ knownCallsClauses cs
def knownCallsFields : List (String × Expr) → List (String × Val)
  | [] => []
  | (_, e) :: fs => knownCalls e ++ knownCallsFields fs
end

abbrev ScopeCheck := String → Val → Bool

def scopeOk (inScope : ScopeCheck) (p : Program) : Bool :=
  (knownCalls p.body).all fun (c, v) => inScope c v

def checkScoped (inScope : ScopeCheck) (p : Program) : Except String Unit :=
  match checkProgram p with
  | .error e => .error e
  | .ok () => if scopeOk inScope p then .ok () else .error "out-of-scope"

inductive Admitted (inScope : ScopeCheck) (ctx : Ctx) : Machine → Prop
  | run {m : Machine} : checkScoped inScope ctx.prog = .ok () → Reachable ctx m → Admitted inScope ctx m

theorem out_of_scope_rejected {inScope : ScopeCheck} {p : Program} {c : String} {v : Val}
    (hknown : (c, v) ∈ knownCalls p.body) (hdenied : inScope c v = false) : checkScoped inScope p ≠ .ok () := by
  intro h
  unfold checkScoped at h
  split at h
  · cases h
  · have hok : scopeOk inScope p = true := by
      cases hs : scopeOk inScope p with
      | false => rw [hs] at h; cases h
      | true => rfl
    have hallowed : inScope c v = true := List.all_eq_true.mp hok (c, v) hknown
    rw [hdenied] at hallowed
    cases hallowed

theorem out_of_scope_never_requests {inScope : ScopeCheck} {ctx : Ctx} {c : String} {v : Val}
    (hknown : (c, v) ∈ knownCalls ctx.prog.body) (hdenied : inScope c v = false) :
    ∀ m, Admitted inScope ctx m → ∀ id cap input, Event.request id cap input ∉ m.trace := by
  intro m hm
  cases hm with
  | run hchecked _ => exact absurd hchecked (out_of_scope_rejected hknown hdenied)

theorem admitted_checks {inScope : ScopeCheck} {p : Program} (h : checkScoped inScope p = .ok ()) :
    checkProgram p = .ok () := by
  unfold checkScoped at h
  split at h
  · cases h
  · assumption

theorem admitted_effect_soundness {inScope : ScopeCheck} {ctx : Ctx} {m : Machine} (h : Admitted inScope ctx m) :
    ∀ id cap input, Event.request id cap input ∈ m.trace → cap ∈ ctx.prog.caps := by
  cases h with
  | run hchecked hreach => exact effect_soundness (admitted_checks hchecked) hreach

end Sx
