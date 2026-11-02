import Sx.Syntax
import Sx.Float

namespace Sx

inductive FailKind where
  | callFailed
  | invalidHostResponse
  | budgetExhausted
  | arithmetic
  | invalidInput
  | outOfRange
  | loopLimit
  | internal
  deriving Repr, BEq, DecidableEq, Inhabited

def FailKind.name : FailKind → String
  | .callFailed => "call-failed"
  | .invalidHostResponse => "invalid-host-response"
  | .budgetExhausted => "budget-exhausted"
  | .arithmetic => "arithmetic"
  | .invalidInput => "invalid-input"
  | .outOfRange => "out-of-range"
  | .loopLimit => "loop-limit"
  | .internal => "internal"

structure Failure where
  kind : FailKind
  message : String
  deriving Repr, Inhabited

def internal {α : Type} (message : String) : Except Failure α :=
  .error ⟨.internal, message⟩

def failWith {α : Type} (kind : FailKind) (message : String) : Except Failure α :=
  .error ⟨kind, message⟩

inductive Slot where
  | val (v : Val)
  | scope (env : List (String × Slot))
  deriving Inhabited

abbrev Env := List (String × Slot)

mutual
def Val.size : Val → Nat
  | .str s => 1 + s.length / 32
  | .bytes b => 1 + base64Length b / 32
  | .list vs => 1 + sizeList vs
  | .tuple vs => 1 + sizeList vs
  | .record fs => 1 + sizeFields fs
  | .map es => 1 + sizeFields es
  | .some v => 1 + v.size
  | .ok v => 1 + v.size
  | .err v => 1 + v.size
  | .variant _ _ (some p) => 1 + p.size
  | _ => 1
def sizeList : List Val → Nat
  | [] => 0
  | v :: vs => v.size + sizeList vs
def sizeFields : List (String × Val) → Nat
  | [] => 0
  | (_, v) :: fs => v.size + sizeFields fs
end

def checkSize (limit : Nat) (v : Val) : Except Failure Unit :=
  if v.size > limit then
    failWith .budgetExhausted s!"A value grew past the value-size budget of {limit}."
  else
    pure ()

def fieldLookup {κ α : Type} [BEq κ] : List (κ × α) → κ → Option α
  | [], _ => none
  | (m, v) :: rest, n => if m == n then some v else fieldLookup rest n

def Env.lookup (env : Env) (n : String) : Option Slot := fieldLookup env n

mutual
def Val.eqv : Val → Val → Bool
  | .bool a, .bool b => a == b
  | .int a, .int b => a == b
  | .float a, .float b => a == b
  | .bytes a, .bytes b => a == b
  | .str a, .str b => a == b
  | .unit, .unit => true
  | .none, .none => true
  | .list as, .list bs => eqvList as bs
  | .tuple as, .tuple bs => eqvList as bs
  | .map as, .map bs => eqvEntries as bs
  | .record as, .record bs => as.length == bs.length && eqvFields as bs
  | .some a, .some b => a.eqv b
  | .ok a, .ok b => a.eqv b
  | .err a, .err b => a.eqv b
  | .variant e n p, .variant e' n' p' => e == e' && n == n' && eqvPayload p p'
  | _, _ => false
def eqvList : List Val → List Val → Bool
  | [], [] => true
  | a :: as, b :: bs => a.eqv b && eqvList as bs
  | _, _ => false
def eqvEntries : List (String × Val) → List (String × Val) → Bool
  | [], [] => true
  | (k, a) :: as, (k', b) :: bs => k == k' && a.eqv b && eqvEntries as bs
  | _, _ => false
def eqvFields : List (String × Val) → List (String × Val) → Bool
  | [], _ => true
  | (k, a) :: as, bs =>
    (match fieldLookup bs k with
      | some b => a.eqv b
      | none => false) && eqvFields as bs
def eqvPayload : Option Val → Option Val → Bool
  | none, none => true
  | some a, some b => a.eqv b
  | _, _ => false
end

def ordOfBool (b : Bool) : Nat := if b then 1 else 0

def compareFloat (a b : Float) : Ordering := if a < b then .lt else if b < a then .gt else .eq

mutual
def cmpKeys : Val → Val → Except Failure Ordering
  | .list as, .list bs => cmpKeyLists as bs
  | .list as, .tuple bs => cmpKeyLists as bs
  | .tuple as, .list bs => cmpKeyLists as bs
  | .tuple as, .tuple bs => cmpKeyLists as bs
  | .bool a, .bool b => pure (compare (ordOfBool a) (ordOfBool b))
  | .int a, .int b => pure (compare a b)
  | .float a, .float b => pure (compareFloat a b)
  | .str a, .str b => pure (compare a b)
  | _, _ => internal "A sort key is not an Int, String, Bool, or tuple."
def cmpKeyLists : List Val → List Val → Except Failure Ordering
  | [], [] => pure .eq
  | [], _ :: _ => pure .lt
  | _ :: _, [] => pure .gt
  | a :: as, b :: bs => do
    let o ← cmpKeys a b
    if o == .eq then cmpKeyLists as bs else pure o
end

def intMin : Int := -9223372036854775808
def intMax : Int := 9223372036854775807

def checkedInt (i : Int) : Except Failure Val :=
  if intMin ≤ i ∧ i ≤ intMax then pure (.int i)
  else failWith .arithmetic "Integer overflow: the result does not fit in a signed 64-bit Int."

mutual
def convert : Val → Conv → Except Failure Val
  | .variant _ n _, .variantName => pure (.str n)
  | v, .toSome none => pure (.some v)
  | v, .toSome (some c) => do
    let w ← convert v c
    pure (.some w)
  | .record fs, .eachField cs => do
    let fs' ← convertFields fs cs
    pure (.record fs')
  | .record fs, .widenRecord ws => do
    let fs' ← widenFields fs ws
    pure (.record fs')
  | .list vs, .eachElement c => do
    let ws ← convertEach vs c
    pure (.list ws)
  | .tuple vs, .eachItem cs => do
    let ws ← convertItems vs cs
    pure (.tuple ws)
  | _, _ => internal "A conversion does not fit its value."
def convertFields : List (String × Val) → List (String × Option Conv) → Except Failure (List (String × Val))
  | [], [] => pure []
  | (m, v) :: fs, (n, oc) :: cs =>
    if m = n then do
      let w ← match oc with
        | none => pure v
        | some c => convert v c
      let rest ← convertFields fs cs
      pure ((m, w) :: rest)
    else internal "A record conversion names a different field."
  | _, _ => internal "A record conversion has the wrong number of fields."
def widenFields (fs : List (String × Val)) :
    List (String × Bool × Option Conv × Ty) → Except Failure (List (String × Val))
  | [] => pure []
  | (n, false, _, _) :: ws => do
    let rest ← widenFields fs ws
    pure ((n, .none) :: rest)
  | (n, true, oc, _) :: ws =>
    match fieldLookup fs n with
    | some v => do
      let w ← match oc with
        | none => pure v
        | some c => convert v c
      let rest ← widenFields fs ws
      pure ((n, w) :: rest)
    | none => internal "A widened record lost a field."
def convertEach (vs : List Val) (c : Conv) : Except Failure (List Val) :=
  vs.mapM fun v => convert v c
def convertItems : List Val → List (Option Conv) → Except Failure (List Val)
  | [], [] => pure []
  | v :: vs, none :: cs => do
    let ws ← convertItems vs cs
    pure (v :: ws)
  | v :: vs, some c :: cs => do
    let w ← convert v c
    let ws ← convertItems vs cs
    pure (w :: ws)
  | _, _ => internal "A tuple conversion has the wrong length."
end

mutual
def bind (env : Env) : Binder → Val → Except Failure Env
  | .name n, v => pure ((n, .val v) :: env)
  | .discard, _ => pure env
  | .tuple bs, .tuple vs => bindAll env bs vs
  | .tuple _, _ => internal "A tuple pattern met a value that is not a tuple."
def bindAll (env : Env) : List Binder → List Val → Except Failure Env
  | [], [] => pure env
  | b :: bs, v :: vs => do
    let env' ← bind env b v
    bindAll env' bs vs
  | _, _ => internal "A pattern and its values have different lengths."
end

def matchPat (env : Env) : Pat → Val → Except Failure (Option Env)
  | .any b, v => do
    let env' ← bind env b v
    pure (some env')
  | .none, .none => pure (some env)
  | .none, _ => pure none
  | .literal lit, v => pure (if v.eqv lit then some env else none)
  | .some p, .some v => matchPat env p v
  | .some _, _ => pure none
  | .ok p, .ok v => matchPat env p v
  | .ok _, _ => pure none
  | .err p, .err v => matchPat env p v
  | .err _, _ => pure none
  | .variant n inner, .variant _ n' payload =>
    if n = n' then
      match inner, payload with
      | none, _ => pure (some env)
      | some p, some w => matchPat env p w
      | some _, none => internal "A variant pattern expects a payload."
    else pure none
  | .variant _ _, _ => pure none

def mapInsert (k : String) (v : Val) : List (String × Val) → List (String × Val)
  | [] => [(k, v)]
  | (k', v') :: rest =>
    if k = k' then (k, v) :: rest
    else if k < k' then (k, v) :: (k', v') :: rest
    else (k', v') :: mapInsert k v rest

def insertField {α : Type} (p : String × α) : List (String × α) → List (String × α)
  | [] => [p]
  | q :: qs => if p.1 < q.1 then p :: q :: qs else q :: insertField p qs

def sortFields {α : Type} : List (String × α) → List (String × α)
  | [] => []
  | p :: ps => insertField p (sortFields ps)

def makeMap (pairs : List (String × Val)) : List (String × Val) :=
  pairs.foldl (fun acc (k, v) => mapInsert k v acc) []

def textOf : Val → Except Failure String
  | .bool b => pure (if b then "true" else "false")
  | .int i => pure (toString i)
  | .str s => pure s
  | .float f => pure (floatText f)
  | .variant _ n none => pure n
  | _ => internal "str cannot write this value."

def hexDigit (n : Nat) : Char := if n < 10 then Char.ofNat (48 + n) else Char.ofNat (87 + n)

def jsonEscapeChar (c : Char) : String :=
  if c = '"' then "\\\""
  else if c = '\\' then "\\\\"
  else if c = '\n' then "\\n"
  else if c = '\r' then "\\r"
  else if c = '\t' then "\\t"
  else if c.val = 8 then "\\b"
  else if c.val = 12 then "\\f"
  else if c.val < 32 then "\\u00" ++ (hexDigit (c.val.toNat / 16)).toString ++ (hexDigit (c.val.toNat % 16)).toString
  else c.toString

def jsonString (s : String) : String :=
  "\"" ++ String.join (s.toList.map jsonEscapeChar) ++ "\""

mutual
def Val.jsonText : Val → String
  | .bool b => if b then "true" else "false"
  | .int i => toString i
  | .float f => floatText f
  | .bytes b => jsonString b
  | .str s => jsonString s
  | .unit => "null"
  | .none => "null"
  | .list vs => "[" ++ ",".intercalate (jsonTextList vs) ++ "]"
  | .tuple vs => "[" ++ ",".intercalate (jsonTextList vs) ++ "]"
  | .record fs => "{" ++ ",".intercalate ((sortFields (jsonTextFields fs)).map Prod.snd) ++ "}"
  | .map es => "{" ++ ",".intercalate ((sortFields (jsonTextFields es)).map Prod.snd) ++ "}"
  | .some v => v.jsonText
  | .ok v => "{\"ok\":" ++ v.jsonText ++ "}"
  | .err v => "{\"err\":" ++ v.jsonText ++ "}"
  | .variant _ n none => jsonString n
  | .variant _ n (some p) => "{\"tag\":" ++ jsonString n ++ ",\"value\":" ++ p.jsonText ++ "}"
def jsonTextList : List Val → List String
  | [] => []
  | v :: vs => v.jsonText :: jsonTextList vs
def jsonTextFields : List (String × Val) → List (String × String)
  | [] => []
  | (k, v) :: fs => (k, jsonString k ++ ":" ++ v.jsonText) :: jsonTextFields fs
end

def checkedFloat (x : Float) : Except Failure Val :=
  if x.isFinite then pure (.float x)
  else failWith .arithmetic "Float overflow: the result is not a finite number, and JSON cannot carry one."

def intFromFloat (r : Option Int) : Except Failure Val :=
  match r with
  | some i =>
    if intMin ≤ i ∧ i ≤ intMax then pure (.int i)
    else failWith .outOfRange "The Float does not fit in a signed 64-bit Int."
  | none => failWith .arithmetic "A Float is not finite."

def flattenLists : List Val → Except Failure (List Val)
  | [] => pure []
  | .list xs :: rest => do
    let ys ← flattenLists rest
    pure (xs ++ ys)
  | _ :: _ => internal "flatten met an element that is not a list."

def pairsOf : List Val → Except Failure (List (String × Val))
  | [] => pure []
  | .tuple [.str k, v] :: rest => do
    let ps ← pairsOf rest
    pure ((k, v) :: ps)
  | _ :: _ => internal "into-map met an element that is not a [String value] pair."

def compareWith (op : Op) (o : Ordering) : Bool :=
  match op with
  | .lt => o == .lt
  | .le => o != .gt
  | .gt => o == .gt
  | _ => o != .lt

def applyOp (limit : Nat) : Op → List Val → Except Failure Val
  | .add, [.int a, .int b] => checkedInt (a + b)
  | .add, [.float a, .float b] => checkedFloat (a + b)
  | .sub, [.float a] => pure (.float (-a))
  | .sub, [.float a, .float b] => checkedFloat (a - b)
  | .mul, [.float a, .float b] => checkedFloat (a * b)
  | .div, [.float a, .float b] => if b == 0.0 then failWith .arithmetic "/ by zero." else checkedFloat (a / b)
  | .toFloat, [.int i] => pure (.float (floatOfInt i))
  | .round, [.float x] => intFromFloat (floatRound x)
  | .floor, [.float x] => intFromFloat (floatFloor x)
  | .ceil, [.float x] => intFromFloat (floatCeil x)
  | .count, [.bytes b] => pure (.int (base64Length b))
  | .sub, [.int a] => checkedInt (-a)
  | .sub, [.int a, .int b] => checkedInt (a - b)
  | .mul, [.int a, .int b] => checkedInt (a * b)
  | .quot, [.int a, .int b] =>
    if b = 0 then failWith .arithmetic "quot by zero." else checkedInt (Int.tdiv a b)
  | .rem, [.int a, .int b] =>
    if b = 0 then failWith .arithmetic "rem by zero." else checkedInt (a - b * Int.tdiv a b)
  | .eq, [a, b] => pure (.bool (a.eqv b))
  | .neq, [a, b] => pure (.bool !(a.eqv b))
  | .lt, [a, b] => do let o ← cmpKeys a b; pure (.bool (compareWith .lt o))
  | .le, [a, b] => do let o ← cmpKeys a b; pure (.bool (compareWith .le o))
  | .gt, [a, b] => do let o ← cmpKeys a b; pure (.bool (compareWith .gt o))
  | .ge, [a, b] => do let o ← cmpKeys a b; pure (.bool (compareWith .ge o))
  | .not, [v] => pure (.bool (v == .bool false))
  | .and, [a, b] => pure (.bool (a == .bool true && b == .bool true))
  | .or, [a, b] => pure (.bool (a == .bool true || b == .bool true))
  | .str, args => do
    let parts ← args.mapM textOf
    let v := Val.str (String.join parts)
    checkSize limit v
    pure v
  | .toJson, [v] => do
    let t := Val.str v.jsonText
    checkSize limit t
    pure t
  | .count, [.map es] => pure (.int es.length)
  | .count, [.list vs] => pure (.int vs.length)
  | .first, [.list []] => failWith .outOfRange "first of an empty list."
  | .first, [.list (v :: _)] => pure v
  | .rest, [.list vs] => pure (.list (vs.drop 1))
  | .nth, [.list vs, .int i] =>
    if h : 0 ≤ i ∧ i.toNat < vs.length then pure (vs[i.toNat]'h.2)
    else failWith .outOfRange s!"nth {i} of a list of {vs.length}."
  | .take, [.int n, .list vs] =>
    if n < 0 then failWith .arithmetic s!"take with a negative count, {n}."
    else pure (.list (vs.take n.toNat))
  | .concat, [.list as, .list bs] => do
    let v := Val.list (as ++ bs)
    checkSize limit v
    pure v
  | .flatten, [.list vs] => do
    let ws ← flattenLists vs
    pure (.list ws)
  | .contains, [.map es, .str k] => pure (.bool (fieldLookup es k).isSome)
  | .contains, [.list vs, x] => pure (.bool (vs.any fun v => v.eqv x))
  | .mapGet, [.map es, .str k] =>
    pure (match fieldLookup es k with
      | some v => .some v
      | none => .none)
  | .assoc, [.map es, .str k, v] => do
    let m := Val.map (makeMap (es ++ [(k, v)]))
    checkSize limit m
    pure m
  | .dissoc, [.map es, .str k] => pure (.map (es.filter fun e => e.1 != k))
  | .keys, [.map es] => pure (.list (es.map fun e => .str e.1))
  | .vals, [.map es] => pure (.list (es.map fun e => e.2))
  | .entries, [.map es] => pure (.list (es.map fun e => .tuple [.str e.1, e.2]))
  | .intoMap, [.list vs] => do
    let ps ← pairsOf vs
    let m := Val.map (makeMap ps)
    checkSize limit m
    pure m
  | _, _ => internal "A prelude function got arguments that do not fit its signature."

def sortStable (key : α → Val) : List α → Except Failure (List α)
  | [] => pure []
  | x :: xs => do
    let sorted ← sortStable key xs
    insertSortedFirst key x sorted
where
  insertSortedFirst (key : α → Val) (x : α) : List α → Except Failure (List α)
    | [] => pure [x]
    | y :: ys => do
      let o ← cmpKeys (key x) (key y)
      if o == .gt then do
        let rest ← insertSortedFirst key x ys
        pure (y :: rest)
      else pure (x :: y :: ys)

def distinctBy (pairs : List (Val × Val)) : List Val :=
  go [] pairs
where
  go (seen : List Val) : List (Val × Val) → List Val
    | [] => []
    | (item, key) :: rest =>
      if seen.any (fun s => s.eqv key) then go seen rest
      else item :: go (key :: seen) rest

def addToGroup (item key : Val) : List (Val × List Val) → List (Val × List Val)
  | [] => [(key, [item])]
  | (k, items) :: rest =>
    if k.eqv key then (k, items ++ [item]) :: rest else (k, items) :: addToGroup item key rest

def groupBy (pairs : List (Val × Val)) : List Val :=
  (pairs.foldl (fun acc (item, key) => addToGroup item key acc) []).map fun (k, items) => .tuple [k, .list items]

def filterKept : List Val → List Val → List Val
  | item :: items, r :: rs => if r == .bool true then item :: filterKept items rs else filterKept items rs
  | _, _ => []

def keepSome : List Val → List Val
  | [] => []
  | .some v :: rs => v :: keepSome rs
  | _ :: rs => keepSome rs

def finishEach (op : Op) (items results : List Val) (acc : Option Val) : Except Failure Val :=
  match op with
  | .map => pure (.list results)
  | .filter => pure (.list (filterKept items results))
  | .keep => pure (.list (keepSome results))
  | .any => pure (.bool false)
  | .all => pure (.bool true)
  | .reduce =>
    match acc with
    | some a => pure a
    | none => internal "reduce lost its accumulator."
  | .sortBy => do
    let sorted ← sortStable (fun (p : Val × Val) => p.2) (items.zip results)
    pure (.list (sorted.map Prod.fst))
  | .distinctBy => pure (.list (distinctBy (items.zip results)))
  | .groupBy => pure (.list (groupBy (items.zip results)))
  | _ => internal "No rule to finish this function."

end Sx
