import Sx.Float

namespace Sx.Decimal

def maxDigits : Nat := 38
def maxScale : Nat := 38

structure D where
  neg : Bool
  mag : Nat
  scale : Nat
  deriving Repr, BEq

inductive Rounding where
  | halfEven | halfUp | down | up | floor | ceiling
  deriving Repr, BEq

def Rounding.ofName : String → Option Rounding
  | "HalfEven" => some .halfEven
  | "HalfUp" => some .halfUp
  | "Down" => some .down
  | "Up" => some .up
  | "Floor" => some .floor
  | "Ceiling" => some .ceiling
  | _ => none

def stripZeros : Nat → Nat → Nat → Nat × Nat
  | 0, m, s => (m, s)
  | fuel + 1, m, s => if s > 0 ∧ m % 10 = 0 ∧ m ≠ 0 then stripZeros fuel (m / 10) (s - 1) else (m, s)

def canonical (neg : Bool) (mag scale : Nat) : Except String D :=
  let (m, s) := if mag = 0 then (0, 0) else stripZeros scale mag scale
  if s > maxScale ∨ decimalDigits m > maxDigits ∧ m ≠ 0 then
    .error s!"the value needs more than {maxDigits} significant digits or {maxScale} fractional digits"
  else .ok { neg := neg ∧ m ≠ 0, mag := m, scale := s }

def digitsValue : List Char → Nat
  | [] => 0
  | c :: cs => (c.toNat - '0'.toNat) * 10 ^ cs.length + digitsValue cs

def allDigits (cs : List Char) : Bool := !cs.isEmpty && cs.all fun c => '0' ≤ c ∧ c ≤ '9'

def parse (text : String) : Except String D :=
  let chars := text.toList
  let (neg, body) := match chars with
    | '-' :: rest => (true, rest)
    | _ => (false, chars)
  let (whole, fraction) := match body.span (· ≠ '.') with
    | (w, '.' :: f) => (w, some f)
    | (w, _) => (w, none)
  let fractionDigits := fraction.getD []
  if !allDigits whole then .error "write an optional minus sign, digits, and optionally a point and digits"
  else if fraction.isSome ∧ !allDigits fractionDigits then .error "a point needs digits after it"
  else canonical neg (digitsValue (whole ++ fractionDigits)) fractionDigits.length

def D.text (d : D) (places : Nat) : String :=
  let digits := toString d.mag
  let padded := String.ofList (List.replicate ((d.scale + 1) - digits.length) '0') ++ digits
  let cut := padded.length - d.scale
  let whole := padded.take cut
  let fraction := padded.drop cut
  let sign := if d.neg then "-" else ""
  if places = 0 then sign ++ whole
  else sign ++ whole ++ "." ++ fraction ++ String.ofList (List.replicate (places - d.scale) '0')

def D.show (d : D) : String := d.text d.scale

def ofInt (i : Int) : D := { neg := i < 0, mag := i.natAbs, scale := 0 }

def D.signed (d : D) : Int := if d.neg then -(d.mag : Int) else d.mag

def alignedSigned (d : D) (scale : Nat) : Int := d.signed * 10 ^ (scale - d.scale)

def ofSigned (i : Int) (scale : Nat) : Except String D := canonical (i < 0) i.natAbs scale

def sum (a b : D) : Except String D :=
  let s := max a.scale b.scale
  ofSigned (alignedSigned a s + alignedSigned b s) s

def difference (a b : D) : Except String D :=
  let s := max a.scale b.scale
  ofSigned (alignedSigned a s - alignedSigned b s) s

def product (a b : D) : Except String D := ofSigned (a.signed * b.signed) (a.scale + b.scale)

def compare (a b : D) : Int :=
  let s := max a.scale b.scale
  let x := alignedSigned a s
  let y := alignedSigned b s
  if x < y then -1 else if x > y then 1 else 0

def rounded (q r d : Nat) (neg : Bool) (mode : Rounding) : Nat :=
  let increment :=
    if r = 0 then false
    else match mode with
      | .down => false
      | .up => true
      | .floor => neg
      | .ceiling => !neg
      | .halfUp => 2 * r ≥ d
      | .halfEven => 2 * r > d ∨ (2 * r = d ∧ q % 2 = 1)
  if increment then q + 1 else q

def placesOf (places : Int) : Except String Nat :=
  if 0 ≤ places ∧ places ≤ maxScale then .ok places.toNat
  else .error s!"{places} places is outside 0 to {maxScale}"

def roundTo (x : D) (places : Nat) (mode : Rounding) : Except String D :=
  if places ≥ x.scale then .ok x
  else
    let divisor := 10 ^ (x.scale - places)
    canonical x.neg (rounded (x.mag / divisor) (x.mag % divisor) divisor x.neg mode) places

def round (x : D) (places : Int) (mode : Rounding) : Except String D := do
  roundTo x (← placesOf places) mode

def format (x : D) (places : Int) : Except String String := do
  let p ← placesOf places
  let r ← roundTo x p .halfEven
  pure (r.text p)

def divide (a b : D) (places : Int) (mode : Rounding) : Except String D := do
  let p ← placesOf places
  if b.mag = 0 then throw "division by zero"
  let dividend := a.mag * 10 ^ (b.scale + p)
  let divisor := b.mag * 10 ^ a.scale
  let neg := a.neg != b.neg
  canonical neg (rounded (dividend / divisor) (dividend % divisor) divisor neg mode) p

def toFloat (x : D) : Option Float := floatOfRat x.neg x.mag (10 ^ x.scale)

def canonicalText (text : String) : Except String String := do
  pure (← parse text).show

end Sx.Decimal

namespace Sx

def nominalCanon : String → Option (String → Except String String)
  | "decimal.Decimal" => some Decimal.canonicalText
  | _ => none

end Sx
