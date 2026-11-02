import Sx.Proofs.Basic

namespace Sx

mutual
theorem Ty.beq_refl : ∀ (a : Ty), Ty.beq a a = true
  | .bool | .int | .float | .bytes | .str | .unit | .never => by simp [Ty.beq]
  | .list a | .map a | .option a => by simp [Ty.beq, Ty.beq_refl a]
  | .result a b => by simp [Ty.beq, Ty.beq_refl a, Ty.beq_refl b]
  | .tuple as => by simp [Ty.beq, Ty.beqList_refl as]
  | .record fs => by simp [Ty.beq, Ty.beqFields_refl fs]
  | .enum n | .var n => by simp [Ty.beq]
  | .nominal n a => by simp [Ty.beq, Ty.beq_refl a]
theorem Ty.beqList_refl : ∀ (as : List Ty), Ty.beqList as as = true
  | [] => by simp [Ty.beqList]
  | a :: as => by simp [Ty.beqList, Ty.beq_refl a, Ty.beqList_refl as]
theorem Ty.beqFields_refl : ∀ (fs : List (String × Ty)), Ty.beqFields fs fs = true
  | [] => by simp [Ty.beqFields]
  | (n, a) :: fs => by simp [Ty.beqFields, Ty.beq_refl a, Ty.beqFields_refl fs]
end

instance : LawfulBEq Ty where
  eq_of_beq h := Ty.eq_of_beq _ _ h
  rfl := Ty.beq_refl _

end Sx
