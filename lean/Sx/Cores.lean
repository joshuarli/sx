import Sx.Machine
import Sx.JsonCore

namespace Sx

open Decimal

def decimalOf : Val → Option D
  | .str s => (parse s).toOption
  | _ => none

def decimalVal (d : D) : Val := .str d.show

def arithmeticOf {α : Type} (r : Except String α) : Except Failure α :=
  match r with
  | .ok a => .ok a
  | .error message => failWith .arithmetic message

def malformed {α : Type} (function : String) : Except Failure α :=
  internal s!"{function} received arguments that do not match its signature"

def decimalCore (function : String) (args : List Val) : Option (Except Failure Val) :=
  let call2 (f : D → D → Except String D) : Option (Except Failure Val) :=
    some (match args with
      | [a, b] =>
        match decimalOf a, decimalOf b with
        | some x, some y => decimalVal <$> arithmeticOf (f x y)
        | _, _ => malformed function
      | _ => malformed function)
  match function with
  | "decimal.parse" =>
    some (match args with
      | [.str s] => pure (match parse s with | .ok d => .some (decimalVal d) | .error _ => .none)
      | _ => malformed function)
  | "decimal.show" =>
    some (match args with
      | [a] => match decimalOf a with | some d => pure (.str d.show) | none => malformed function
      | _ => malformed function)
  | "decimal.format" =>
    some (match args with
      | [a, .int places] =>
        match decimalOf a with
        | some d => .str <$> arithmeticOf (format d places)
        | none => malformed function
      | _ => malformed function)
  | "decimal.from-int" =>
    some (match args with
      | [.int i] => pure (decimalVal (ofInt i))
      | _ => malformed function)
  | "decimal.add" => call2 sum
  | "decimal.sub" => call2 difference
  | "decimal.mul" => call2 product
  | "decimal.div" =>
    some (match args with
      | [a, b, .int places, .variant _ mode none] =>
        match decimalOf a, decimalOf b, Rounding.ofName mode with
        | some x, some y, some m => decimalVal <$> arithmeticOf (divide x y places m)
        | _, _, _ => malformed function
      | _ => malformed function)
  | "decimal.round" =>
    some (match args with
      | [a, .int places, .variant _ mode none] =>
        match decimalOf a, Rounding.ofName mode with
        | some x, some m => decimalVal <$> arithmeticOf (round x places m)
        | _, _ => malformed function
      | _ => malformed function)
  | "decimal.compare" =>
    some (match args with
      | [a, b] =>
        match decimalOf a, decimalOf b with
        | some x, some y => pure (.int (Decimal.compare x y))
        | _, _ => malformed function
      | _ => malformed function)
  | "decimal.to-float" =>
    some (match args with
      | [a] =>
        match decimalOf a with
        | some d =>
          match toFloat d with
          | some f => pure (.float f)
          | none => internal "the decimal has no finite Float"
        | none => malformed function
      | _ => malformed function)
  | _ => none

def decimalFunctions : List String :=
  ["decimal.parse", "decimal.show", "decimal.format", "decimal.from-int", "decimal.add", "decimal.sub",
   "decimal.mul", "decimal.div", "decimal.round", "decimal.compare", "decimal.to-float"]

def stringsOf : List Val → Option (List String)
  | [] => some []
  | .str s :: rest => (stringsOf rest).map (s :: ·)
  | _ => none

def refusal {α : Type} (r : Except String α) : Except Failure α :=
  match r with
  | .ok a => .ok a
  | .error message => failWith .invalidInput message

def jsonCore (function : String) (args : List Val) : Option (Except Failure Val) :=
  match function, args with
  | "json.get", [.str text, .list path] =>
    (stringsOf path).map fun p =>
      refusal ((JsonCore.get text p).map fun found => match found with | some s => Val.some (.str s) | none => Val.none)
  | "json.keys", [.str text, .list path] =>
    (stringsOf path).map fun p => refusal ((JsonCore.keys text p).map fun ks => Val.list (ks.map Val.str))
  | "json.kind", [.str text, .list path] =>
    (stringsOf path).map fun p => refusal ((JsonCore.kind text p).map fun k => Val.variant "json.Kind" k none)
  | _, _ => none

def jsonFunctions : List String := ["json.get", "json.keys", "json.kind"]

def coreLibrary : Cores := fun function =>
  if decimalFunctions.contains function then
    some fun args => (decimalCore function args).getD (malformed function)
  else if jsonFunctions.contains function then
    some fun args => (jsonCore function args).getD (malformed function)
  else none

end Sx
