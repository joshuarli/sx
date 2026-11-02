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
  | .fail m => callsOf m
  | .log args => callsOfList args
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

mutual
def callsIn (rep : Bool) : Expr → List (String × Bool)
  | .lit .. => []
  | .var _ => []
  | .let_ _ v body => callsIn rep v ++ callsIn rep body
  | .localFn _ body => callsIn rep body
  | .if_ c t e => callsIn rep c ++ callsIn rep t ++ callsIn rep e
  | .match_ s cs => callsIn rep s ++ callsInClauses rep cs
  | .lam _ _ _ body _ => callsIn true body
  | .applyLocal _ f args => callsInList rep args ++ callsIn rep f
  | .prelude _ args => callsInList rep args
  | .core _ args => callsInList rep args
  | .call c arg => callsIn rep arg ++ [(c, rep)]
  | .get r _ => callsIn rep r
  | .tupleItem e _ => callsIn rep e
  | .mkRecord fs => callsInFields rep fs
  | .mkList es _ => callsInList rep es
  | .mkTuple es => callsInList rep es
  | .wrap _ e _ => callsIn rep e
  | .mkVariant _ _ none => []
  | .mkVariant _ _ (some e) => callsIn rep e
  | .coerce e _ => callsIn rep e
  | .attempt e => callsIn rep e
  | .fail m => callsIn rep m
  | .log args => callsInList rep args
  | .retry _ _ e => callsIn rep e
  | .waitUntil _ p e => callsIn rep e ++ callsIn rep p
  | .sleep _ => []
  | .loop _ _ init body => callsInList rep init ++ callsIn true body
  | .recur args => callsInList rep args
  | .par bs => callsInList rep bs
  | .parMap _ f xs => callsIn rep xs ++ callsIn rep f
def callsInList (rep : Bool) : List Expr → List (String × Bool)
  | [] => []
  | e :: es => callsIn rep e ++ callsInList rep es
def callsInClauses (rep : Bool) : List (Pat × Expr) → List (String × Bool)
  | [] => []
  | (_, e) :: cs => callsIn rep e ++ callsInClauses rep cs
def callsInFields (rep : Bool) : List (String × Expr) → List (String × Bool)
  | [] => []
  | (_, e) :: fs => callsIn rep e ++ callsInFields rep fs
end

end Sx
