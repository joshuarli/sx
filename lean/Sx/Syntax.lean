namespace Sx

inductive Ty where
  | bool
  | int
  | float
  | bytes
  | str
  | unit
  | list (t : Ty)
  | map (v : Ty)
  | tuple (ts : List Ty)
  | option (t : Ty)
  | result (ok err : Ty)
  | record (fs : List (String × Ty))
  | enum (name : String)
  | nominal (name : String) (base : Ty)
  | var (n : Nat)
  | never
  deriving Repr, Inhabited

mutual
def Ty.beq : Ty → Ty → Bool
  | .bool, .bool => true
  | .int, .int => true
  | .float, .float => true
  | .bytes, .bytes => true
  | .str, .str => true
  | .unit, .unit => true
  | .list a, .list b => a.beq b
  | .map a, .map b => a.beq b
  | .tuple as, .tuple bs => Ty.beqList as bs
  | .option a, .option b => a.beq b
  | .result a a', .result b b' => a.beq b && a'.beq b'
  | .record as, .record bs => Ty.beqFields as bs
  | .enum n, .enum m => n == m
  | .nominal n a, .nominal m b => n == m && a.beq b
  | .var n, .var m => n == m
  | .never, .never => true
  | _, _ => false
def Ty.beqList : List Ty → List Ty → Bool
  | [], [] => true
  | a :: as, b :: bs => a.beq b && Ty.beqList as bs
  | _, _ => false
def Ty.beqFields : List (String × Ty) → List (String × Ty) → Bool
  | [], [] => true
  | (n, a) :: as, (m, b) :: bs => n == m && a.beq b && Ty.beqFields as bs
  | _, _ => false
end

instance : BEq Ty := ⟨Ty.beq⟩

inductive Val where
  | bool (b : Bool)
  | int (i : Int)
  | float (f : Float)
  | bytes (base64 : String)
  | str (s : String)
  | unit
  | list (vs : List Val)
  | tuple (vs : List Val)
  | record (fs : List (String × Val))
  | map (es : List (String × Val))
  | some (v : Val)
  | none
  | ok (v : Val)
  | err (v : Val)
  | variant (enum name : String) (payload : Option Val)
  deriving Repr, BEq, Inhabited

inductive Binder where
  | name (n : String)
  | discard
  | tuple (bs : List Binder)
  deriving Repr, BEq, Inhabited

inductive Pat where
  | some (p : Pat)
  | none
  | ok (p : Pat)
  | err (p : Pat)
  | variant (name : String) (inner : Option Pat)
  | literal (v : Val)
  | any (b : Binder)
  deriving Repr, BEq, Inhabited

inductive Conv where
  | toSome (inner : Option Conv)
  | eachField (fs : List (String × Option Conv))
  | widenRecord (fs : List (String × Bool × Option Conv × Ty))
  | eachItem (cs : List (Option Conv))
  | eachElement (c : Conv)
  | variantName
  deriving Repr, BEq, Inhabited

inductive Op where
  | add | sub | mul | div | quot | rem | toFloat | round | floor | ceil
  | eq | neq | lt | le | gt | ge
  | not | and | or
  | str | toJson
  | count | first | rest | nth | take | concat | flatten | contains
  | mapGet | assoc | dissoc | keys | vals | entries | intoMap | range | enumerate | safeInt
  | map | filter | keep | any | all | reduce | sortBy | distinctBy | groupBy
  deriving Repr, BEq, DecidableEq, Inhabited

inductive WrapKind where
  | some | ok | err
  deriving Repr, BEq, DecidableEq, Inhabited

inductive Expr where
  | lit (v : Val) (t : Ty)
  | var (n : String)
  | let_ (b : Binder) (value body : Expr)
  | localFn (n : String) (body : Expr)
  | if_ (c t e : Expr)
  | match_ (s : Expr) (cs : List (Pat × Expr))
  | lam (ps : List Binder) (tys : List Ty) (res : Ty) (body : Expr) (captured : Option String)
  | applyLocal (n : String) (f : Expr) (args : List Expr)
  | prelude (op : Op) (args : List Expr)
  | core (f : String) (args : List Expr)
  | call (cap : String) (arg : Expr)
  | get (r : Expr) (field : String)
  | tupleItem (e : Expr) (index : Nat)
  | mkRecord (fs : List (String × Expr))
  | mkList (es : List Expr) (elem : Ty)
  | mkTuple (es : List Expr)
  | wrap (k : WrapKind) (e : Expr) (t : Ty)
  | mkVariant (enum name : String) (payload : Option Expr)
  | coerce (e : Expr) (c : Conv)
  | attempt (e : Expr)
  | fail (m : Expr)
  | log (args : List Expr)
  | retry (delays : List Nat) (kinds : List String) (e : Expr)
  | waitUntil (intervals : List Nat) (pred : Expr) (e : Expr)
  | sleep (ms : Nat)
  | loop (max : Nat) (bs : List Binder) (init : List Expr) (body : Expr)
  | recur (args : List Expr)
  | par (bs : List Expr)
  | parMap (limit : Nat) (f : Expr) (items : Expr)
  deriving Repr, BEq, Inhabited

inductive Effect where
  | read | write
  deriving Repr, BEq, DecidableEq, Inhabited

structure CapSig where
  effect : Effect
  idempotent : Bool
  input : Ty
  output : Ty
  deriving Repr, Inhabited

structure FunSig where
  params : List Ty
  output : Ty
  deriving Repr, Inhabited

abbrev Enums := List (String × List (String × Option Ty))

inductive DTy where
  | bool
  | int
  | float
  | bytes
  | str
  | unit
  | list (t : DTy)
  | map (v : DTy)
  | tuple (ts : List DTy)
  | option (t : DTy)
  | result (ok err : DTy)
  | record (fs : List (String × DTy)) (isOpen : Bool)
  | enum (name : String)
  | nominal (name : String) (base : DTy)
  | var (n : Nat)
  | never
  deriving Repr, Inhabited

abbrev DecodeEnums := List (String × List (String × Option DTy))

structure Program where
  body : Expr
  result : Ty
  caps : List String
  inputs : List (String × Ty)
  enums : Enums
  capSigs : List (String × CapSig)
  funSigs : List (String × FunSig)
  capOutputs : List (String × DTy)
  inputDecodes : List (String × DTy)
  decodeEnums : DecodeEnums
  deriving Inhabited

def callErrorKinds : List String :=
  ["Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed"]

def callErrorKindEnum : String × List (String × Option Ty) :=
  ("CallErrorKind", callErrorKinds.map fun k => (k, none))

def callError (kind message : String) : Val :=
  .record [("kind", .variant "CallErrorKind" kind none), ("message", .str message)]

def Op.higherOrder : Op → Bool
  | .map | .filter | .keep | .any | .all | .reduce | .sortBy | .distinctBy | .groupBy => true
  | _ => false

def Op.ofString : String → Option Op
  | "+" => some .add | "-" => some .sub | "*" => some .mul | "/" => some .div | "quot" => some .quot
  | "rem" => some .rem | "float" => some .toFloat | "round" => some .round | "floor" => some .floor
  | "ceil" => some .ceil
  | "=" => some .eq | "not=" => some .neq | "<" => some .lt | "<=" => some .le | ">" => some .gt
  | ">=" => some .ge | "not" => some .not | "and" => some .and | "or" => some .or | "str" => some .str
  | "to-json" => some .toJson
  | "count" => some .count | "first" => some .first | "rest" => some .rest | "nth" => some .nth
  | "take" => some .take | "concat" => some .concat | "flatten" => some .flatten
  | "contains?" => some .contains | "map-get" => some .mapGet | "assoc" => some .assoc
  | "dissoc" => some .dissoc | "keys" => some .keys | "vals" => some .vals | "entries" => some .entries
  | "into-map" => some .intoMap | "range" => some .range | "enumerate" => some .enumerate
  | "safe-int" => some .safeInt | "map" => some .map | "filter" => some .filter | "keep" => some .keep
  | "any?" => some .any | "all?" => some .all | "reduce" => some .reduce | "sort-by" => some .sortBy
  | "distinct-by" => some .distinctBy | "group-by" => some .groupBy
  | _ => none

end Sx
