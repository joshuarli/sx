import Sx.Value

namespace Sx

structure Sig where
  enums : Enums
  caps : List (String × CapSig)
  funs : List (String × FunSig)

def Program.sig (p : Program) : Sig := ⟨p.enums, p.capSigs, p.funSigs⟩

def callErrorTy : Ty := .record [("kind", .enum "CallErrorKind"), ("message", .str)]

inductive All2 {α β : Type} (R : α → β → Prop) : List α → List β → Prop
  | nil : All2 R [] []
  | cons : R a b → All2 R as bs → All2 R (a :: as) (b :: bs)

def variantPayload (E : Enums) (e n : String) : Option (Option Ty) :=
  match fieldLookup E e with
  | some vs => fieldLookup vs n
  | none => none

inductive VT (E : Enums) : Val → Ty → Prop
  | bool : VT E (.bool b) .bool
  | int : VT E (.int i) .int
  | str : VT E (.str s) .str
  | unit : VT E .unit .unit
  | list : (∀ v ∈ vs, VT E v t) → VT E (.list vs) (.list t)
  | tuple : All2 (VT E) vs ts → VT E (.tuple vs) (.tuple ts)
  | record : fs.map Prod.fst = tfs.map Prod.fst → All2 (VT E) (fs.map Prod.snd) (tfs.map Prod.snd) →
      VT E (.record fs) (.record tfs)
  | map : (∀ e ∈ es, VT E e.2 t) → VT E (.map es) (.map t)
  | none : VT E .none (.option t)
  | some : VT E v t → VT E (.some v) (.option t)
  | ok : VT E v a → VT E (.ok v) (.result a b)
  | err : VT E v b → VT E (.err v) (.result a b)
  | variant : variantPayload E e n = some none → VT E (.variant e n none) (.enum e)
  | variantPayload : variantPayload E e n = some (some pt) → VT E p pt → VT E (.variant e n (some p)) (.enum e)
  | nominal : VT E v base → VT E v (.nominal name base)

inductive Bnd where
  | val (t : Ty)
  | fn (Γ : List (String × Bnd))

abbrev TyEnv := List (String × Bnd)

inductive ET (E : Enums) : Env → TyEnv → Prop
  | nil : ET E [] []
  | val : VT E v t → ET E ρ Γ → ET E ((n, .val v) :: ρ) ((n, .val t) :: Γ)
  | fn : ET E ρd Γd → ET E ρ Γ → ET E ((n, .scope ρd) :: ρ) ((n, .fn Γd) :: Γ)

mutual
def bindTy (Γ : TyEnv) : Binder → Ty → Option TyEnv
  | .name n, t => some ((n, .val t) :: Γ)
  | .discard, _ => some Γ
  | .tuple bs, .tuple ts => bindAllTy Γ bs ts
  | .tuple _, _ => none
def bindAllTy (Γ : TyEnv) : List Binder → List Ty → Option TyEnv
  | [], [] => some Γ
  | b :: bs, t :: ts =>
    match bindTy Γ b t with
    | some Γ' => bindAllTy Γ' bs ts
    | none => none
  | _, _ => none
end

def patTy (E : Enums) (Γ : TyEnv) : Pat → Ty → Option TyEnv
  | .any b, t => bindTy Γ b t
  | .none, .option _ => some Γ
  | .some p, .option t => patTy E Γ p t
  | .ok p, .result a _ => patTy E Γ p a
  | .err p, .result _ b => patTy E Γ p b
  | .variant n none, .enum e =>
    match variantPayload E e n with
    | some _ => some Γ
    | none => none
  | .variant n (some p), .enum e =>
    match variantPayload E e n with
    | some (some pt) => patTy E Γ p pt
    | _ => none
  | _, _ => none

def Pat.irrefutable : Pat → Bool
  | .any _ => true
  | _ => false

def coversVariant (pats : List Pat) (n : String) : Bool :=
  pats.any fun p =>
    match p with
    | .variant m none => m == n
    | .variant m (some q) => m == n && q.irrefutable
    | _ => false

def exhaustive (E : Enums) (pats : List Pat) (t : Ty) : Bool :=
  pats.any Pat.irrefutable ||
    match t with
    | .option _ =>
      pats.any (· matches .none) && pats.any fun p => match p with
        | .some q => q.irrefutable
        | _ => false
    | .result _ _ =>
      (pats.any fun p => match p with
        | .ok q => q.irrefutable
        | _ => false) &&
      (pats.any fun p => match p with
        | .err q => q.irrefutable
        | _ => false)
    | .enum e =>
      match fieldLookup E e with
      | some vs => vs.all fun (n, _) => coversVariant pats n
      | none => false
    | .never => true
    | _ => false

def nullaryEnum (E : Enums) (e : String) : Bool :=
  match fieldLookup E e with
  | some vs => vs.all fun (_, p) => p.isNone
  | none => false

mutual
def convTy (E : Enums) : Conv → Ty → Option Ty
  | .variantName, .enum e => if nullaryEnum E e then some .str else none
  | .toSome none, t => some (.option t)
  | .toSome (some c), t =>
    match convTy E c t with
    | some u => some (.option u)
    | none => none
  | .eachField cs, .record fs =>
    match convFieldsTy E cs fs with
    | some fs' => some (.record fs')
    | none => none
  | .eachElement c, .list t =>
    match convTy E c t with
    | some u => some (.list u)
    | none => none
  | .eachItem cs, .tuple ts =>
    match convItemsTy E cs ts with
    | some ts' => some (.tuple ts')
    | none => none
  | _, _ => none
def convFieldsTy (E : Enums) : List (String × Option Conv) → List (String × Ty) → Option (List (String × Ty))
  | [], [] => some []
  | (n, none) :: cs, (m, t) :: fs =>
    if n = m then
      match convFieldsTy E cs fs with
      | some fs' => some ((m, t) :: fs')
      | none => none
    else none
  | (n, some c) :: cs, (m, t) :: fs =>
    if n = m then
      match convTy E c t, convFieldsTy E cs fs with
      | some u, some fs' => some ((m, u) :: fs')
      | _, _ => none
    else none
  | _, _ => none
def convItemsTy (E : Enums) : List (Option Conv) → List Ty → Option (List Ty)
  | [], [] => some []
  | none :: cs, t :: ts =>
    match convItemsTy E cs ts with
    | some ts' => some (t :: ts')
    | none => none
  | some c :: cs, t :: ts =>
    match convTy E c t, convItemsTy E cs ts with
    | some u, some ts' => some (u :: ts')
    | _, _ => none
  | _, _ => none
end

def orderable : Ty → Bool
  | .int | .str | .var _ | .never => true
  | _ => false

mutual
def sortKey : Ty → Bool
  | .int | .str | .bool | .var _ | .never => true
  | .tuple ts => sortKeys ts
  | .list t => sortKey t
  | .nominal _ b => sortKey b
  | _ => false
def sortKeys : List Ty → Bool
  | [] => true
  | t :: ts => sortKey t && sortKeys ts
end

def allStr : List Ty → Bool
  | [] => true
  | .str :: ts => allStr ts
  | _ :: _ => false

def opTy : Op → List Ty → Option Ty
  | .add, [.int, .int] => some .int
  | .sub, [.int] => some .int
  | .sub, [.int, .int] => some .int
  | .mul, [.int, .int] => some .int
  | .quot, [.int, .int] => some .int
  | .rem, [.int, .int] => some .int
  | .eq, [_, _] => some .bool
  | .neq, [_, _] => some .bool
  | .lt, [a, b] => if a == b && orderable a then some .bool else none
  | .le, [a, b] => if a == b && orderable a then some .bool else none
  | .gt, [a, b] => if a == b && orderable a then some .bool else none
  | .ge, [a, b] => if a == b && orderable a then some .bool else none
  | .not, [.bool] => some .bool
  | .and, [.bool, .bool] => some .bool
  | .or, [.bool, .bool] => some .bool
  | .str, t :: ts => if allStr (t :: ts) then some .str else none
  | .count, [.list _] => some .int
  | .count, [.map _] => some .int
  | .first, [.list t] => some t
  | .rest, [.list t] => some (.list t)
  | .nth, [.list t, .int] => some t
  | .take, [.int, .list t] => some (.list t)
  | .concat, [.list a, .list b] => if a == b then some (.list a) else none
  | .flatten, [.list (.list t)] => some (.list t)
  | .contains, [.list a, b] => if a == b then some .bool else none
  | .contains, [.map _, .str] => some .bool
  | .mapGet, [.map v, .str] => some (.option v)
  | .assoc, [.map v, .str, w] => if v == w then some (.map v) else none
  | .dissoc, [.map v, .str] => some (.map v)
  | .keys, [.map _] => some (.list .str)
  | .vals, [.map v] => some (.list v)
  | .entries, [.map v] => some (.list (.tuple [.str, v]))
  | .intoMap, [.list (.tuple [.str, v])] => some (.map v)
  | _, _ => none

def lamBase (Γ : TyEnv) : Option String → Option TyEnv
  | none => some Γ
  | some n =>
    match fieldLookup Γ n with
    | some (.fn Γd) => some Γd
    | _ => none

def capInput (S : Sig) (c : String) : Option CapSig := fieldLookup S.caps c

mutual
inductive HasType (S : Sig) : TyEnv → Option (List Ty) → Expr → Ty → Prop
  | lit : VT S.enums v t → HasType S Γ L (.lit v t) t
  | var : fieldLookup Γ n = some (.val t) → HasType S Γ L (.var n) t
  | let_ : HasType S Γ none v tv → bindTy Γ b tv = some Γ' → HasType S Γ' L body t →
      HasType S Γ L (.let_ b v body) t
  | localFn : HasType S ((n, .fn Γ) :: Γ) L body t → HasType S Γ L (.localFn n body) t
  | if_ : HasType S Γ none c .bool → HasType S Γ L th t → HasType S Γ L el t → HasType S Γ L (.if_ c th el) t
  | match_ : HasType S Γ none s ts → Clauses S Γ L cs ts t → exhaustive S.enums (cs.map Prod.fst) ts = true →
      HasType S Γ L (.match_ s cs) t
  | applyLocal : fieldLookup Γ n = some (.fn Γd) → bindAllTy Γd ps tys = some Γ' → HasType S Γ' none body t →
      HasTypes S Γ args tys → HasType S Γ L (.applyLocal n (.lam ps tys t body cap) args) t
  | prelude : HasTypes S Γ args ts → opTy op ts = some t → op.higherOrder = false →
      HasType S Γ L (.prelude op args) t
  | map : LamT S Γ f [a] b → HasType S Γ none xs (.list a) → HasType S Γ L (.prelude .map [f, xs]) (.list b)
  | filter : LamT S Γ f [a] .bool → HasType S Γ none xs (.list a) →
      HasType S Γ L (.prelude .filter [f, xs]) (.list a)
  | keep : LamT S Γ f [a] (.option b) → HasType S Γ none xs (.list a) →
      HasType S Γ L (.prelude .keep [f, xs]) (.list b)
  | any : LamT S Γ f [a] .bool → HasType S Γ none xs (.list a) → HasType S Γ L (.prelude .any [f, xs]) .bool
  | all : LamT S Γ f [a] .bool → HasType S Γ none xs (.list a) → HasType S Γ L (.prelude .all [f, xs]) .bool
  | reduce : LamT S Γ f [acc, a] acc → HasType S Γ none init acc → HasType S Γ none xs (.list a) →
      HasType S Γ L (.prelude .reduce [f, init, xs]) acc
  | sortBy : LamT S Γ f [a] k → sortKey k = true → HasType S Γ none xs (.list a) →
      HasType S Γ L (.prelude .sortBy [f, xs]) (.list a)
  | distinctBy : LamT S Γ f [a] k → HasType S Γ none xs (.list a) →
      HasType S Γ L (.prelude .distinctBy [f, xs]) (.list a)
  | groupBy : LamT S Γ f [a] k → HasType S Γ none xs (.list a) →
      HasType S Γ L (.prelude .groupBy [f, xs]) (.list (.tuple [k, .list a]))
  | core : fieldLookup S.funs fname = some sig → HasTypes S Γ args sig.params →
      HasType S Γ L (.core fname args) sig.output
  | call : capInput S c = some sig → HasType S Γ none arg sig.input → HasType S Γ L (.call c arg) sig.output
  | get : HasType S Γ none r (.record fs) → fieldLookup fs fld = some t → HasType S Γ L (.get r fld) t
  | mkRecord : Fields S Γ fs tfs → HasType S Γ L (.mkRecord fs) (.record tfs)
  | mkList : AllT S Γ es t → HasType S Γ L (.mkList es t) (.list t)
  | mkTuple : HasTypes S Γ es ts → HasType S Γ L (.mkTuple es) (.tuple ts)
  | wrapSome : HasType S Γ none e t → HasType S Γ L (.wrap .some e (.option t)) (.option t)
  | wrapOk : HasType S Γ none e a → HasType S Γ L (.wrap .ok e (.result a b)) (.result a b)
  | wrapErr : HasType S Γ none e b → HasType S Γ L (.wrap .err e (.result a b)) (.result a b)
  | mkVariant : variantPayload S.enums en n = some none → HasType S Γ L (.mkVariant en n none) (.enum en)
  | mkVariantPayload : variantPayload S.enums en n = some (some pt) → HasType S Γ none p pt →
      HasType S Γ L (.mkVariant en n (some p)) (.enum en)
  | coerce : HasType S Γ none e s → convTy S.enums c s = some t → HasType S Γ L (.coerce e c) t
  | attempt : HasType S Γ none e t → HasType S Γ L (.attempt e) (.result t callErrorTy)
  | retry : HasType S Γ none e t → HasType S Γ L (.retry ds ks e) t
  | waitUntil : LamT S Γ p [t] .bool → HasType S Γ none e t → HasType S Γ L (.waitUntil is p e) t
  | loop : HasTypes S Γ init ts → bindAllTy Γ bs ts = some Γ' → HasType S Γ' (some ts) body t →
      HasType S Γ L (.loop m bs init body) t
  | recur : HasTypes S Γ args ts → HasType S Γ (some ts) (.recur args) t
  | par : HasTypes S Γ bs ts → HasType S Γ L (.par bs) (.tuple ts)
  | parMap : LamT S Γ f [a] b → HasType S Γ none xs (.list a) → HasType S Γ L (.parMap lim f xs) (.list b)
inductive HasTypes (S : Sig) : TyEnv → List Expr → List Ty → Prop
  | nil : HasTypes S Γ [] []
  | cons : HasType S Γ none e t → HasTypes S Γ es ts → HasTypes S Γ (e :: es) (t :: ts)
inductive AllT (S : Sig) : TyEnv → List Expr → Ty → Prop
  | nil : AllT S Γ [] t
  | cons : HasType S Γ none e t → AllT S Γ es t → AllT S Γ (e :: es) t
inductive Fields (S : Sig) : TyEnv → List (String × Expr) → List (String × Ty) → Prop
  | nil : Fields S Γ [] []
  | cons : HasType S Γ none e t → Fields S Γ fs tfs → Fields S Γ ((n, e) :: fs) ((n, t) :: tfs)
inductive Clauses (S : Sig) : TyEnv → Option (List Ty) → List (Pat × Expr) → Ty → Ty → Prop
  | nil : Clauses S Γ L [] ts t
  | cons : patTy S.enums Γ p ts = some Γ' → HasType S Γ' L body t → Clauses S Γ L cs ts t →
      Clauses S Γ L ((p, body) :: cs) ts t
inductive LamT (S : Sig) : TyEnv → Expr → List Ty → Ty → Prop
  | mk : lamBase Γ cap = some Γb → bindAllTy Γb ps tys = some Γ' → HasType S Γ' none body res →
      LamT S Γ (.lam ps tys res body cap) tys res
end

end Sx
