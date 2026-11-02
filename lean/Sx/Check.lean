import Sx.Typing
import Sx.Effects

namespace Sx

mutual
def checkVal (E : Enums) : Val → Ty → Bool
  | .bool _, .bool => true
  | .int _, .int => true
  | .float _, .float => true
  | .bytes _, .bytes => true
  | .str _, .str => true
  | .unit, .unit => true
  | .list vs, .list t => checkVals E vs t
  | .tuple vs, .tuple ts => checkItems E vs ts
  | .record fs, .record tfs => checkFieldVals E fs tfs
  | .map es, .map t => checkEntries E es t
  | .none, .option _ => true
  | .some v, .option t => checkVal E v t
  | .ok v, .result a _ => checkVal E v a
  | .err v, .result _ b => checkVal E v b
  | .variant e n none, .enum e' => e == e' && variantPayload E e n == some none
  | .variant e n (some p), .enum e' =>
    e == e' &&
      match variantPayload E e n with
      | some (some pt) => checkVal E p pt
      | _ => false
  | v, .nominal _ base => checkVal E v base
  | _, _ => false
def checkVals (E : Enums) : List Val → Ty → Bool
  | [], _ => true
  | v :: vs, t => checkVal E v t && checkVals E vs t
def checkItems (E : Enums) : List Val → List Ty → Bool
  | [], [] => true
  | v :: vs, t :: ts => checkVal E v t && checkItems E vs ts
  | _, _ => false
def checkFieldVals (E : Enums) : List (String × Val) → List (String × Ty) → Bool
  | [], [] => true
  | (n, v) :: fs, (m, t) :: tfs => n == m && checkVal E v t && checkFieldVals E fs tfs
  | _, _ => false
def checkEntries (E : Enums) : List (String × Val) → Ty → Bool
  | [], _ => true
  | (_, v) :: es, t => checkVal E v t && checkEntries E es t
end

def listElem : Option Ty → Option Ty
  | some (.list a) => some a
  | _ => none

def withElem (t : Option Ty) (k : Ty → Option Ty) : Option Ty :=
  match t with
  | some (.list a) => k a
  | _ => none

def expect (exp : Option Ty) (r : Option Ty) : Option Ty :=
  match exp, r with
  | none, r => r
  | some t, some u => if u == t then some t else none
  | some _, none => none

mutual
def tc (S : Sig) (Γ : TyEnv) (L : Option (List Ty)) (e : Expr) (exp : Option Ty) : Option Ty :=
  match e with
  | .recur args =>
    match exp, L with
    | some t, some ts => if checkAll S Γ args ts then some t else none
    | _, _ => none
  | .let_ b v body =>
    match tc S Γ none v none with
    | some tv =>
      match bindTy Γ b tv with
      | some Γ' => tc S Γ' L body exp
      | none => none
    | none => none
  | .localFn n body => tc S ((n, .fn Γ) :: Γ) L body exp
  | .if_ c th el =>
    if (tc S Γ none c (some .bool)).isSome then
      match exp with
      | some t => if (tc S Γ L th (some t)).isSome && (tc S Γ L el (some t)).isSome then some t else none
      | none =>
        match tc S Γ L th none with
        | some t =>
          if (tc S Γ L el (some t)).isSome then some t
          else if t == .never then
            match tc S Γ L el none with
            | some u => if (tc S Γ L th (some u)).isSome then some u else none
            | none => none
          else none
        | none =>
          match tc S Γ L el none with
          | some t => if (tc S Γ L th (some t)).isSome then some t else none
          | none => none
    else none
  | .match_ s cs =>
    match tc S Γ none s none with
    | some ts =>
      if exhaustive S.enums (cs.map Prod.fst) ts then
        match exp with
        | some t => if checkClauses S Γ L cs ts t then some t else none
        | none =>
          match inferClauses S Γ L cs ts with
          | some t => if checkClauses S Γ L cs ts t then some t else none
          | none => none
      else none
    | none => none
  | .loop _ bs init body =>
    match inferAll S Γ init with
    | some ts =>
      match bindAllTy Γ bs ts with
      | some Γ' => tc S Γ' (some ts) body exp
      | none => none
    | none => none
  | .lit v t => expect exp (if checkVal S.enums v t then some t else none)
  | .var n =>
    expect exp (match fieldLookup Γ n with
      | some (.val t) => some t
      | _ => none)
  | .applyLocal n (.lam ps tys res body _) args =>
    expect exp (match fieldLookup Γ n with
      | some (.fn Γd) =>
        match bindAllTy Γd ps tys with
        | some Γ' => if (tc S Γ' none body (some res)).isSome && checkAll S Γ args tys then some res else none
        | none => none
      | _ => none)
  | .prelude op args =>
    expect exp (if op.higherOrder then
      match op, args with
      | .map, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a =>
          match lamResult S Γ f [a] with
          | some b => some (.list b)
          | none => none
        | none => none
      | .filter, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a => if lamResult S Γ f [a] == some .bool then some (.list a) else none
        | none => none
      | .keep, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a =>
          match lamResult S Γ f [a] with
          | some (.option b) => some (.list b)
          | _ => none
        | none => none
      | .any, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a => if lamResult S Γ f [a] == some .bool then some .bool else none
        | none => none
      | .all, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a => if lamResult S Γ f [a] == some .bool then some .bool else none
        | none => none
      | .reduce, [f, init, xs] =>
        match tc S Γ none init none, listElem (tc S Γ none xs none) with
        | some acc, some a => if lamResult S Γ f [acc, a] == some acc then some acc else none
        | _, _ => none
      | .sortBy, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a =>
          match lamResult S Γ f [a] with
          | some k => if sortKey k then some (.list a) else none
          | none => none
        | none => none
      | .distinctBy, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a =>
          match lamResult S Γ f [a] with
          | some _ => some (.list a)
          | none => none
        | none => none
      | .groupBy, [f, xs] =>
        match listElem (tc S Γ none xs none) with
        | some a =>
          match lamResult S Γ f [a] with
          | some k => some (.list (.tuple [k, .list a]))
          | none => none
        | none => none
      | _, _ => none
    else
      match inferAll S Γ args with
      | some ts => opTy S.enums op ts
      | none => none)
  | .core f args =>
    expect exp (match fieldLookup S.funs f with
      | some sig => if checkAll S Γ args sig.params then some sig.output else none
      | none => none)
  | .call c arg =>
    expect exp (match capInput S c with
      | some sig => if (tc S Γ none arg (some sig.input)).isSome then some sig.output else none
      | none => none)
  | .tupleItem xs i =>
    expect exp (match tc S Γ none xs none with
      | some (.tuple ts) => ts[i]?
      | _ => none)
  | .get r fld =>
    expect exp (match tc S Γ none r none with
      | some (.record fs) => fieldLookup fs fld
      | _ => none)
  | .mkRecord fs =>
    expect exp (match inferFields S Γ fs with
      | some tfs => some (.record (sortFields tfs))
      | none => none)
  | .mkList es t => expect exp (if checkEach S Γ es t then some (.list t) else none)
  | .mkTuple es =>
    expect exp (match inferAll S Γ es with
      | some ts => some (.tuple ts)
      | none => none)
  | .wrap k e t =>
    expect exp (match k, t with
      | .some, .option a => if (tc S Γ none e (some a)).isSome then some t else none
      | .ok, .result a _ => if (tc S Γ none e (some a)).isSome then some t else none
      | .err, .result _ b => if (tc S Γ none e (some b)).isSome then some t else none
      | _, _ => none)
  | .mkVariant en n none =>
    expect exp (if variantPayload S.enums en n == some none then some (.enum en) else none)
  | .mkVariant en n (some p) =>
    expect exp (match variantPayload S.enums en n with
      | some (some pt) => if (tc S Γ none p (some pt)).isSome then some (.enum en) else none
      | _ => none)
  | .coerce e c =>
    expect exp (match tc S Γ none e none with
      | some s => convTy S.enums c s
      | none => none)
  | .attempt e =>
    expect exp (match tc S Γ none e none with
      | some t => some (.result t callErrorTy)
      | none => none)
  | .retry _ _ e => expect exp (tc S Γ none e none)
  | .sleep _ => expect exp (some .unit)
  | .fail m =>
    match tc S Γ none m none with
    | some u => if u == .str || u == callErrorTy then (match exp with | some t => some t | none => some .never) else none
    | none => none
  | .log args =>
    expect exp (match inferAll S Γ args with
      | some _ => some .unit
      | none => none)
  | .waitUntil _ p e =>
    expect exp (match tc S Γ none e none with
      | some t => if lamResult S Γ p [t] == some .bool then some t else none
      | none => none)
  | .par bs =>
    expect exp (match inferAll S Γ bs with
      | some ts => some (.tuple ts)
      | none => none)
  | .parMap _ f xs =>
    expect exp (match listElem (tc S Γ none xs none) with
      | some a =>
        match lamResult S Γ f [a] with
        | some b => some (.list b)
        | none => none
      | none => none)
  | _ => none
def inferAll (S : Sig) (Γ : TyEnv) : List Expr → Option (List Ty)
  | [] => some []
  | e :: es =>
    match tc S Γ none e none, inferAll S Γ es with
    | some t, some ts => some (t :: ts)
    | _, _ => none
def checkAll (S : Sig) (Γ : TyEnv) : List Expr → List Ty → Bool
  | [], [] => true
  | e :: es, t :: ts => (tc S Γ none e (some t)).isSome && checkAll S Γ es ts
  | _, _ => false
def checkEach (S : Sig) (Γ : TyEnv) : List Expr → Ty → Bool
  | [], _ => true
  | e :: es, t => (tc S Γ none e (some t)).isSome && checkEach S Γ es t
def inferFields (S : Sig) (Γ : TyEnv) : List (String × Expr) → Option (List (String × Ty))
  | [] => some []
  | (n, e) :: fs =>
    match tc S Γ none e none, inferFields S Γ fs with
    | some t, some tfs => some ((n, t) :: tfs)
    | _, _ => none
def inferClauses (S : Sig) (Γ : TyEnv) (L : Option (List Ty)) : List (Pat × Expr) → Ty → Option Ty
  | [], _ => none
  | (p, body) :: cs, ts =>
    match patTy S.enums Γ p ts with
    | some Γ' =>
      match tc S Γ' L body none with
      | some t =>
        if t == .never then
          match inferClauses S Γ L cs ts with
          | some u => some u
          | none => some t
        else some t
      | none => inferClauses S Γ L cs ts
    | none => none
def checkClauses (S : Sig) (Γ : TyEnv) (L : Option (List Ty)) : List (Pat × Expr) → Ty → Ty → Bool
  | [], _, _ => true
  | (p, body) :: cs, ts, t =>
    match patTy S.enums Γ p ts with
    | some Γ' => (tc S Γ' L body (some t)).isSome && checkClauses S Γ L cs ts t
    | none => false
def lamResult (S : Sig) (Γ : TyEnv) (f : Expr) (args : List Ty) : Option Ty :=
  match f with
  | .lam ps tys res body cap =>
    if tys == args then
      match lamBase Γ cap with
      | some Γb =>
        match bindAllTy Γb ps tys with
        | some Γ' => if (tc S Γ' none body (some res)).isSome then some res else none
        | none => none
      | none => none
    else none
  | _ => none
end

def check (S : Sig) (Γ : TyEnv) (L : Option (List Ty)) (e : Expr) (t : Ty) : Bool := (tc S Γ L e (some t)).isSome

def isRead (S : Sig) (c : String) : Bool :=
  match capInput S c with
  | some sig => sig.effect == .read
  | none => false

def refusalKinds : List String := ["RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict"]

def isRepeatable (S : Sig) (c : String) : Bool :=
  match capInput S c with
  | some sig => sig.effect == .read || sig.idempotent
  | none => false

mutual
def effectsOk (S : Sig) : Expr → Bool
  | .lit .. => true
  | .var _ => true
  | .let_ _ v body => effectsOk S v && effectsOk S body
  | .localFn _ body => effectsOk S body
  | .if_ c t e => effectsOk S c && effectsOk S t && effectsOk S e
  | .match_ s cs => effectsOk S s && effectsOkClauses S cs
  | .lam _ _ _ body _ => effectsOk S body
  | .applyLocal _ f args => effectsOk S f && effectsOkList S args
  | .prelude _ args => effectsOkList S args
  | .core _ args => effectsOkList S args
  | .call _ arg => effectsOk S arg
  | .get r _ => effectsOk S r
  | .tupleItem e _ => effectsOk S e
  | .mkRecord fs => effectsOkFields S fs
  | .mkList es _ => effectsOkList S es
  | .mkTuple es => effectsOkList S es
  | .wrap _ e _ => effectsOk S e
  | .mkVariant _ _ none => true
  | .mkVariant _ _ (some e) => effectsOk S e
  | .coerce e _ => effectsOk S e
  | .attempt e => effectsOk S e
  | .fail m => effectsOk S m
  | .log args => effectsOkList S args
  | .retry _ ks e =>
    let calls := callsIn false e
    (calls.all (fun c => isRepeatable S c.1) ||
      (ks.all refusalKinds.contains &&
        calls.dropLast.all (fun c => isRepeatable S c.1) &&
        calls.getLast?.all (fun c => isRepeatable S c.1 || !c.2))) && effectsOk S e
  | .waitUntil _ p e => (callsOf e).all (isRead S) && effectsOk S p && effectsOk S e
  | .sleep _ => true
  | .loop _ _ init body => effectsOkList S init && effectsOk S body
  | .recur args => effectsOkList S args
  | .par bs => (callsOfList bs).all (isRead S) && effectsOkList S bs
  | .parMap _ f xs => (callsOf f).all (isRead S) && effectsOk S f && effectsOk S xs
def effectsOkList (S : Sig) : List Expr → Bool
  | [] => true
  | e :: es => effectsOk S e && effectsOkList S es
def effectsOkClauses (S : Sig) : List (Pat × Expr) → Bool
  | [] => true
  | (_, e) :: cs => effectsOk S e && effectsOkClauses S cs
def effectsOkFields (S : Sig) : List (String × Expr) → Bool
  | [] => true
  | (_, e) :: fs => effectsOk S e && effectsOkFields S fs
end

def Program.inputEnv (p : Program) : TyEnv := p.inputs.map fun (n, t) => (n, .val t)

def checkProgram (p : Program) : Except String Unit :=
  if !check p.sig p.inputEnv none p.body p.result then
    .error "the program is not well typed"
  else if !effectsOk p.sig p.body then
    .error "a par branch, par-map function, or wait-until body writes, or a retry repeats a write that is not idempotent"
  else if !(callsOf p.body).all p.caps.contains then
    .error "the program calls a capability outside its effect set"
  else .ok ()

end Sx
