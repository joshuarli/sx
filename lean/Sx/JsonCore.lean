import Sx.Value

namespace Sx.JsonCore

def maxDepth : Nat := 1000

inductive J where
  | null
  | bool (b : Bool)
  | num (text : String)
  | str (text : String)
  | arr (items : List J)
  | obj (entries : List (String × J))
  deriving Inhabited

inductive Open where
  | arr (items : List J)
  | obj (entries : List (String × J)) (key : String)

inductive Mode where
  | value
  | attach (done : J)

def hexOf (n : Nat) : String := String.ofList (Nat.toDigits 16 n)

def debugChar (c : Char) : String :=
  if c == '\'' then "'\\''"
  else if c == '\\' then "'\\\\'"
  else if c == '\n' then "'\\n'"
  else if c == '\r' then "'\\r'"
  else if c == '\t' then "'\\t'"
  else if c == '\u0000' then "'\\0'"
  else if c.toNat < 0x20 ∨ c.toNat == 0x7f then "'\\u{" ++ hexOf c.toNat ++ "}'"
  else "'" ++ c.toString ++ "'"

def debugStr (s : String) : String :=
  "\"" ++ String.join (s.toList.map fun c =>
    if c == '"' then "\\\""
    else if c == '\\' then "\\\\"
    else if c == '\n' then "\\n"
    else if c == '\r' then "\\r"
    else if c == '\t' then "\\t"
    else if c == '\u0000' then "\\0"
    else if c.toNat < 0x20 ∨ c.toNat == 0x7f then "\\u{" ++ hexOf c.toNat ++ "}"
    else c.toString) ++ "\""

def failAt {α : Type} (cs : Array Char) (i : Nat) (message : String) : Except String α :=
  let before := cs.extract 0 i
  let line := (before.toList.filter (· == '\n')).length + 1
  let column := (before.toList.reverse.takeWhile (· != '\n')).length + 1
  .error s!"line {line} column {column}: {message}"

def found (cs : Array Char) (i : Nat) : String :=
  match cs[i]? with
  | none => "the end of the text"
  | some c => debugChar c

def isBlank (c : Char) : Bool := c == ' ' || c == '\t' || c == '\n' || c == '\r'

def blanksFrom (cs : Array Char) : Nat → Nat → Nat
  | 0, i => i
  | f + 1, i =>
    match cs[i]? with
    | some c => if isBlank c then blanksFrom cs f (i + 1) else i
    | none => i

def blanks (cs : Array Char) (i : Nat) : Nat := blanksFrom cs cs.size i

def digitsFrom (cs : Array Char) : Nat → Nat → Nat
  | 0, i => i
  | f + 1, i =>
    match cs[i]? with
    | some c => if c.isDigit then digitsFrom cs f (i + 1) else i
    | none => i

def digitsEnd (cs : Array Char) (i : Nat) : Nat := digitsFrom cs cs.size i

def hexValue (c : Char) : Option Nat :=
  if c.isDigit then some (c.toNat - '0'.toNat)
  else if 'a' ≤ c ∧ c ≤ 'f' then some (c.toNat - 'a'.toNat + 10)
  else if 'A' ≤ c ∧ c ≤ 'F' then some (c.toNat - 'A'.toNat + 10)
  else none

def hex4 (cs : Array Char) (i : Nat) : Option Nat := do
  let a ← cs[i]? >>= hexValue
  let b ← cs[i + 1]? >>= hexValue
  let c ← cs[i + 2]? >>= hexValue
  let d ← cs[i + 3]? >>= hexValue
  pure (((a * 16 + b) * 16 + c) * 16 + d)

def escapeNeedsFour : String := "a \\u escape needs four hexadecimal digits"
def highNeedsLow : String := "a high surrogate escape must be followed by a low surrogate escape"

def stringFrom (cs : Array Char) (opening : Nat) : Nat → Nat → String → Except String (String × Nat)
  | 0, _, _ => failAt cs opening "the string is not closed"
  | f + 1, i, acc =>
    match cs[i]? with
    | none => failAt cs opening "the string is not closed"
    | some '"' => pure (acc, i + 1)
    | some '\\' =>
      let simple (c : Char) := stringFrom cs opening f (i + 2) (acc.push c)
      match cs[i + 1]? with
      | none => failAt cs opening "the string is not closed"
      | some '"' => simple '"'
      | some '\\' => simple '\\'
      | some '/' => simple '/'
      | some 'b' => simple '\u0008'
      | some 'f' => simple '\u000c'
      | some 'n' => simple '\n'
      | some 'r' => simple '\r'
      | some 't' => simple '\t'
      | some 'u' =>
        match hex4 cs (i + 2) with
        | none => failAt cs i escapeNeedsFour
        | some first =>
          if 0xD800 ≤ first ∧ first ≤ 0xDBFF then
            if cs[i + 6]? == some '\\' ∧ cs[i + 7]? == some 'u' then
              match hex4 cs (i + 8) with
              | none => failAt cs (i + 6) escapeNeedsFour
              | some second =>
                if 0xDC00 ≤ second ∧ second ≤ 0xDFFF then
                  let code := 0x10000 + (first - 0xD800) * 1024 + (second - 0xDC00)
                  stringFrom cs opening f (i + 12) (acc.push (Char.ofNat code))
                else failAt cs i highNeedsLow
            else failAt cs i highNeedsLow
          else if 0xDC00 ≤ first ∧ first ≤ 0xDFFF then
            failAt cs i "a low surrogate escape must follow a high surrogate escape"
          else stringFrom cs opening f (i + 6) (acc.push (Char.ofNat first))
      | some c => failAt cs i s!"the escape \\{c} is not defined in JSON"
    | some c =>
      if c.toNat < 0x20 then failAt cs i "a control character in a string must be escaped"
      else stringFrom cs opening f (i + 1) (acc.push c)

def sliceText (cs : Array Char) (start stop : Nat) : String := String.ofList (cs.extract start stop).toList

def numberFrom (cs : Array Char) (start : Nat) : Except String (J × Nat) := do
  let mut j := start
  if cs[j]? == some '-' then
    j := j + 1
    unless (cs[j]?.map Char.isDigit).getD false do
      failAt cs j s!"a number needs a digit after '-', found {found cs j}"
  if cs[j]? == some '0' then
    j := j + 1
    if (cs[j]?.map Char.isDigit).getD false then
      failAt cs (j - 1) "a number cannot have leading zeros"
  else
    j := digitsEnd cs j
  if cs[j]? == some '.' then
    j := j + 1
    let k := digitsEnd cs j
    if k == j then
      failAt cs j s!"a number needs digits after the decimal point, found {found cs j}"
    j := k
  if cs[j]? == some 'e' || cs[j]? == some 'E' then
    j := j + 1
    if cs[j]? == some '+' || cs[j]? == some '-' then
      j := j + 1
    let k := digitsEnd cs j
    if k == j then
      failAt cs j s!"a number needs digits in its exponent, found {found cs j}"
    j := k
  pure (.num (sliceText cs start j), j)

def alnumEnd (cs : Array Char) : Nat → Nat → Nat
  | 0, i => i
  | f + 1, i =>
    match cs[i]? with
    | some c => if c.isAlphanum then alnumEnd cs f (i + 1) else i
    | none => i

def wordFrom (cs : Array Char) (start : Nat) : Except String (J × Nat) :=
  let stop := alnumEnd cs cs.size start
  match sliceText cs start stop with
  | "true" => pure (.bool true, stop)
  | "false" => pure (.bool false, stop)
  | "null" => pure (.null, stop)
  | other => failAt cs start s!"expected a value, found {debugStr other}"

def keyFrom (cs : Array Char) (i : Nat) (known : List String) : Except String (String × Nat) := do
  unless cs[i]? == some '"' do
    failAt cs i s!"expected a string key, found {found cs i}"
  let (key, after) ← stringFrom cs i (cs.size + 1) (i + 1) ""
  if known.contains key then
    failAt cs i s!"the key {debugStr key} appears twice in one object"
  let j := blanks cs after
  unless cs[j]? == some ':' do
    failAt cs j s!"expected ':' after the key, found {found cs j}"
  pure (key, j + 1)

def drive (cs : Array Char) : Nat → Nat → List Open → Mode → Except String J
  | 0, _, _, _ => .error "the parser ran out of fuel"
  | f + 1, i, stack, .value =>
    let i := blanks cs i
    match cs[i]? with
    | some '{' =>
      if stack.length + 1 > maxDepth then failAt cs i s!"the value nests deeper than {maxDepth} levels"
      else
        let j := blanks cs (i + 1)
        if cs[j]? == some '}' then drive cs f (j + 1) stack (.attach (.obj []))
        else
          match keyFrom cs j [] with
          | .error e => .error e
          | .ok (key, after) => drive cs f after (.obj [] key :: stack) .value
    | some '[' =>
      if stack.length + 1 > maxDepth then failAt cs i s!"the value nests deeper than {maxDepth} levels"
      else
        let j := blanks cs (i + 1)
        if cs[j]? == some ']' then drive cs f (j + 1) stack (.attach (.arr []))
        else drive cs f j (.arr [] :: stack) .value
    | some '"' =>
      match stringFrom cs i (cs.size + 1) (i + 1) "" with
      | .error e => .error e
      | .ok (s, after) => drive cs f after stack (.attach (.str s))
    | some c =>
      if c == '-' || c.isDigit then
        match numberFrom cs i with
        | .error e => .error e
        | .ok (n, after) => drive cs f after stack (.attach n)
      else if c.isAlpha then
        match wordFrom cs i with
        | .error e => .error e
        | .ok (w, after) => drive cs f after stack (.attach w)
      else failAt cs i s!"expected a value, found {found cs i}"
    | none => failAt cs i s!"expected a value, found {found cs i}"
  | f + 1, i, stack, .attach done =>
    let i := blanks cs i
    match stack with
    | [] => if i < cs.size then failAt cs i "unexpected content after the value" else pure done
    | .arr items :: rest =>
      let items := done :: items
      match cs[i]? with
      | some ',' =>
        let j := blanks cs (i + 1)
        if cs[j]? == some ']' then failAt cs i "a trailing comma is not allowed"
        else drive cs f j (.arr items :: rest) .value
      | some ']' => drive cs f (i + 1) rest (.attach (.arr items.reverse))
      | _ => failAt cs i s!"expected ',' or ']' in an array, found {found cs i}"
    | .obj entries key :: rest =>
      let entries := (key, done) :: entries
      match cs[i]? with
      | some ',' =>
        let j := blanks cs (i + 1)
        if cs[j]? == some '}' then failAt cs i "a trailing comma is not allowed"
        else
          match keyFrom cs j (entries.map Prod.fst) with
          | .error e => .error e
          | .ok (next, after) => drive cs f after (.obj entries next :: rest) .value
      | some '}' => drive cs f (i + 1) rest (.attach (.obj entries.reverse))
      | _ => failAt cs i s!"expected ',' or '}' in an object, found {found cs i}"

def parse (text : String) : Except String J :=
  let cs := text.toList.toArray
  if cs[0]? == some '\uFEFF' then failAt cs 0 "a byte order mark is not allowed"
  else drive cs (2 * cs.size + 10) 0 [] .value

def indexOf (part : String) : Option Nat :=
  if part.isEmpty ∨ (part.length > 1 ∧ part.startsWith "0") ∨ !(part.all Char.isDigit) then none
  else part.toNat?

def locate : J → List String → Option J
  | j, [] => some j
  | .obj entries, part :: rest => (entries.lookup part).bind fun child => locate child rest
  | .arr items, part :: rest => (indexOf part >>= fun n => items[n]?).bind fun child => locate child rest
  | _, _ :: _ => none

def shownPath (path : List String) : String :=
  if path.isEmpty then "the document root" else "the path " ++ "/".intercalate (path.map debugStr)

def kindName : Option J → String
  | none => "Absent"
  | some .null => "Null"
  | some (.bool _) => "Boolean"
  | some (.num _) => "Number"
  | some (.str _) => "String"
  | some (.arr _) => "Array"
  | some (.obj _) => "Object"

def get (text : String) (path : List String) : Except String (Option String) := do
  let root ← parse text
  match locate root path with
  | none | some .null => pure none
  | some (.bool b) => pure (some (if b then "true" else "false"))
  | some (.num t) => pure (some t)
  | some (.str s) => pure (some s)
  | some (.arr _) => .error s!"{shownPath path} is an array, not a scalar"
  | some (.obj _) => .error s!"{shownPath path} is an object, not a scalar"

def keys (text : String) (path : List String) : Except String (List String) := do
  let root ← parse text
  match locate root path with
  | none | some .null => pure []
  | some (.obj entries) => pure (entries.map Prod.fst)
  | some (.arr items) => pure ((List.range items.length).map toString)
  | some _ => .error s!"{shownPath path} is a scalar, so it has no keys"

def kind (text : String) (path : List String) : Except String String := do
  let root ← parse text
  pure (kindName (locate root path))

end Sx.JsonCore
