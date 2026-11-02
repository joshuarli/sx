namespace Sx

def floatParts (x : Float) : Option (Bool × Nat × Int) :=
  let b := x.toBits.toNat
  let neg := b / 2 ^ 63 == 1
  let e : Nat := (b / 2 ^ 52) % 2048
  let f := b % 2 ^ 52
  if e == 2047 then none
  else if e == 0 then some (neg, f, -1074)
  else some (neg, f + 2 ^ 52, (e : Int) - 1075)

def floatRat (x : Float) : Option (Bool × Nat × Nat) :=
  match floatParts x with
  | some (neg, m, e) => some (neg, if e ≥ 0 then m * 2 ^ e.toNat else m, if e ≥ 0 then 1 else 2 ^ (-e).toNat)
  | none => none

def scaledQuotient (num den : Nat) (k : Int) : Nat × Nat × Nat :=
  let n := if k < 0 then num * 2 ^ (-k).toNat else num
  let d := if k > 0 then den * 2 ^ k.toNat else den
  (n / d, n % d, d)

def findScale (num den : Nat) : Nat → Int → Int
  | 0, k => k
  | fuel + 1, k =>
    let (q, _, _) := scaledQuotient num den k
    if q ≥ 2 ^ 53 then findScale num den fuel (k + 1)
    else if q < 2 ^ 52 then findScale num den fuel (k - 1)
    else k

def floatOfRat (neg : Bool) (num den : Nat) : Option Float :=
  if num = 0 then some (if neg then -0.0 else 0.0)
  else
    let guess : Int := (Nat.log2 num : Int) - (Nat.log2 den : Int) - 52
    let k := max (findScale num den 8 guess) (-1074)
    let (q, r, d) := scaledQuotient num den k
    let q := if 2 * r > d ∨ (2 * r = d ∧ q % 2 = 1) then q + 1 else q
    let (q, k) := if q = 2 ^ 53 then (2 ^ 52, k + 1) else (q, k)
    if k > 971 then none
    else
      let bits := (if neg then 2 ^ 63 else 0) +
        (if q ≥ 2 ^ 52 then (k + 1075).toNat * 2 ^ 52 + (q - 2 ^ 52) else q)
      some (Float.ofBits (UInt64.ofNat bits))

def floatOfInt (i : Int) : Float :=
  (floatOfRat (i < 0) i.natAbs 1).getD 0.0

def decimalDigits (n : Nat) : Nat := (toString n).length

def tenPow (k : Int) : Nat × Nat := if k ≥ 0 then (10 ^ k.toNat, 1) else (1, 10 ^ (-k).toNat)

def roundHalfEven (n d : Nat) : Nat :=
  let q := n / d
  let r := n % d
  if 2 * r > d ∨ (2 * r = d ∧ q % 2 = 1) then q + 1 else q

def shortestDigits (x : Float) : Option (Nat × Int) :=
  match floatParts x with
  | none => none
  | some (_, 0, _) => some (0, 0)
  | some (_, m, e) =>
    let lowerCloser := m = 2 ^ 52 ∧ e > -1074
    let unit : Nat × Nat := if e - 2 ≥ 0 then (2 ^ (e - 2).toNat, 1) else (1, 2 ^ (2 - e).toNat)
    let hiN := (4 * m + 2) * unit.1
    let loN := (if lowerCloser then 4 * m - 1 else 4 * m - 2) * unit.1
    let xN := 4 * m * unit.1
    let den := unit.2
    let inclusive := m % 2 = 0
    let inside (c : Nat) (s : Int) : Bool :=
      let (a, b) := tenPow (-s)
      let v := c * a * den
      let lo := loN * b
      let hi := hiN * b
      if inclusive then lo ≤ v ∧ v ≤ hi else lo < v ∧ v < hi
    let guess : Int := (decimalDigits xN : Int) - (decimalDigits den : Int)
    let below (k : Int) : Bool := let (a, b) := tenPow k; a * den ≤ xN * b
    let e10 := if !below guess then guess - 1 else if below (guess + 1) then guess + 1 else guess
    let rec go : Nat → Option (Nat × Int)
      | 0 => none
      | fuel + 1 =>
        let p : Nat := 18 - fuel
        let s : Int := (p : Int) - 1 - e10
        let (a, b) := tenPow s
        let c := roundHalfEven (xN * a) (den * b)
        if inside c s then some (c, -s)
        else if c > 0 ∧ inside (c - 1) s then some (c - 1, -s)
        else if inside (c + 1) s then some (c + 1, -s)
        else go fuel
    go 18

def stripZeros : Nat × Int → Nat → Nat × Int
  | (c, e), 0 => (c, e)
  | (c, e), fuel + 1 => if c ≠ 0 ∧ c % 10 = 0 then stripZeros (c / 10, e + 1) fuel else (c, e)

def floatText (x : Float) : String :=
  match shortestDigits x with
  | none => "NaN"
  | some (0, _) => "0"
  | some d =>
    let (c, e) := stripZeros d 40
    let coefficient := toString c
    let count : Int := coefficient.length
    let point : Int := count + e
    let sign := if x < 0.0 then "-" else ""
    let body :=
      if count ≤ point ∧ point ≤ 21 then coefficient ++ String.ofList (List.replicate (point - count).toNat '0')
      else if 0 < point ∧ point ≤ 21 then
        String.ofList (coefficient.toList.take point.toNat) ++ "." ++ String.ofList (coefficient.toList.drop point.toNat)
      else if -6 < point ∧ point ≤ 0 then "0." ++ String.ofList (List.replicate (-point).toNat '0') ++ coefficient
      else
        let shown := point - 1
        let rest := String.ofList (coefficient.toList.drop 1)
        let mantissa := String.ofList (coefficient.toList.take 1) ++ (if count > 1 then "." ++ rest else "")
        mantissa ++ "e" ++ (if shown ≥ 0 then "+" else "-") ++ toString shown.natAbs
    sign ++ body

def floatFloor (x : Float) : Option Int :=
  match floatRat x with
  | some (neg, n, d) => some (if neg then -(((n + d - 1) / d : Nat) : Int) else ((n / d : Nat) : Int))
  | none => none

def floatCeil (x : Float) : Option Int :=
  match floatRat x with
  | some (neg, n, d) => some (if neg then -((n / d : Nat) : Int) else (((n + d - 1) / d : Nat) : Int))
  | none => none

def floatRound (x : Float) : Option Int :=
  match floatRat x with
  | some (neg, n, d) =>
    let whole := n / d
    let up := 2 * (n % d) ≥ d
    let magnitude := if up then whole + 1 else whole
    some (if neg then -(magnitude : Int) else magnitude)
  | none => none

def base64Char (c : Char) : Option Nat :=
  if 'A' ≤ c ∧ c ≤ 'Z' then some (c.toNat - 'A'.toNat)
  else if 'a' ≤ c ∧ c ≤ 'z' then some (c.toNat - 'a'.toNat + 26)
  else if '0' ≤ c ∧ c ≤ '9' then some (c.toNat - '0'.toNat + 52)
  else if c = '+' then some 62
  else if c = '/' then some 63
  else none

def canonicalBase64 (s : String) : Bool :=
  let cs := s.toList
  let n := cs.length
  let pads := (cs.reverse.takeWhile (· = '=')).length
  let body := cs.take (n - pads)
  decide (n % 4 = 0) && decide (pads ≤ 2) && body.all (fun c => (base64Char c).isSome) &&
    (match body.getLast? >>= base64Char with
      | some v => if pads = 2 then v % 16 == 0 else if pads = 1 then v % 4 == 0 else true
      | none => pads == 0)

def base64Length (s : String) : Nat :=
  let cs := s.toList
  let pads := (cs.reverse.takeWhile (· = '=')).length
  cs.length / 4 * 3 - pads

end Sx
