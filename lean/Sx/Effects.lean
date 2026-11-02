import Sx.Syntax

namespace Sx

mutual
def callsOf : Expr → List String
  | .lit .. => []
  | .var _ => []
  | .let_ _ v body => callsOf v ++ callsOf body
  | .localFn _ body => callsOf body
  | .if_ c t e => callsOf c ++ callsOf t ++ callsOf e
  | .match_ s cs => callsOf s ++ callsOfClauses cs
  | .lam _ _ _ body _ => callsOf body
  | .applyLocal _ f args => callsOf f ++ callsOfList args
  | .prelude _ args => callsOfList args
  | .core _ args => callsOfList args
  | .call c arg => c :: callsOf arg
  | .get r _ => callsOf r
  | .tupleItem e _ => callsOf e
  | .mkRecord fs => callsOfFields fs
  | .mkList es _ => callsOfList es
  | .mkTuple es => callsOfList es
  | .wrap _ e _ => callsOf e
  | .mkVariant _ _ none => []
  | .mkVariant _ _ (some e) => callsOf e
  | .coerce e _ => callsOf e
  | .attempt e => callsOf e
  | .retry _ _ e => callsOf e
  | .waitUntil _ p e => callsOf p ++ callsOf e
  | .sleep _ => []
  | .loop _ _ init body => callsOfList init ++ callsOf body
  | .recur args => callsOfList args
  | .par bs => callsOfList bs
  | .parMap _ f xs => callsOf f ++ callsOf xs
def callsOfList : List Expr → List String
  | [] => []
  | e :: es => callsOf e ++ callsOfList es
def callsOfClauses : List (Pat × Expr) → List String
  | [] => []
  | (_, e) :: cs => callsOf e ++ callsOfClauses cs
def callsOfFields : List (String × Expr) → List String
  | [] => []
  | (_, e) :: fs => callsOf e ++ callsOfFields fs
end

end Sx
