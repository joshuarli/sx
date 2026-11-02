import Sx.Eval
import Sx.Typing

namespace Sx

@[simp] theorem except_ok_bind {ε α β : Type} (a : α) (f : α → Except ε β) : (Except.ok a >>= f) = f a := rfl

@[simp] theorem except_error_bind {ε α β : Type} (e : ε) (f : α → Except ε β) :
    (Except.error e >>= f) = Except.error e := rfl

@[simp] theorem except_pure {ε α : Type} (a : α) : (pure a : Except ε α) = Except.ok a := rfl

def SafeE {α : Type} (P : α → Prop) : Except Failure α → Prop
  | .ok a => P a
  | .error f => f.kind ≠ .internal

theorem SafeE.pure {α : Type} {P : α → Prop} {a : α} (h : P a) : SafeE P (Pure.pure a) := h

theorem SafeE.bind {α β : Type} {P : α → Prop} {Q : β → Prop} {x : Except Failure α} {f : α → Except Failure β}
    (hx : SafeE P x) (hf : ∀ a, P a → SafeE Q (f a)) : SafeE Q (x >>= f) := by
  cases x with
  | error e => exact hx
  | ok a => exact hf a hx

theorem SafeE.fail {α : Type} {P : α → Prop} {k : FailKind} {m : String} (h : k ≠ .internal) :
    SafeE P (failWith k m : Except Failure α) := h

theorem SafeE.mono {α : Type} {P Q : α → Prop} {x : Except Failure α} (hx : SafeE P x) (h : ∀ a, P a → Q a) :
    SafeE Q x := by
  cases x with
  | error e => exact hx
  | ok a => exact h a hx

theorem fieldLookup_eq {κ α : Type} [BEq κ] [LawfulBEq κ] (n : κ) (v : α) (rest : List (κ × α)) :
    fieldLookup ((n, v) :: rest) n = some v := by
  simp [fieldLookup]

theorem fieldLookup_ne {κ α : Type} [BEq κ] [LawfulBEq κ] {m n : κ} (v : α) (rest : List (κ × α)) (h : m ≠ n) :
    fieldLookup ((m, v) :: rest) n = fieldLookup rest n := by
  simp [fieldLookup, beq_eq_false_iff_ne.mpr h]

theorem fieldLookup_mem {κ α : Type} [BEq κ] [LawfulBEq κ] {l : List (κ × α)} {n : κ} {v : α}
    (h : fieldLookup l n = some v) : (n, v) ∈ l := by
  induction l with
  | nil => simp [fieldLookup] at h
  | cons p rest ih =>
    obtain ⟨m, w⟩ := p
    by_cases hm : m = n
    · subst hm; rw [fieldLookup_eq] at h; cases h; simp
    · rw [fieldLookup_ne _ _ hm] at h; exact List.mem_cons_of_mem _ (ih h)

theorem ET.lookup_val {E : Enums} {ρ : Env} {Γ : TyEnv} (h : ET E ρ Γ) {n : String} {t : Ty}
    (hl : fieldLookup Γ n = some (.val t)) : ∃ v, fieldLookup ρ n = some (.val v) ∧ VT E v t := by
  induction h with
  | nil => simp [fieldLookup] at hl
  | @val v t' ρ Γ m hv _ ih =>
    by_cases hm : m = n
    · subst hm; rw [fieldLookup_eq] at hl ⊢; cases hl; exact ⟨v, rfl, hv⟩
    · rw [fieldLookup_ne _ _ hm] at hl ⊢; exact ih hl
  | @fn ρd Γd ρ Γ m _ _ _ ih =>
    by_cases hm : m = n
    · subst hm; rw [fieldLookup_eq] at hl; cases hl
    · rw [fieldLookup_ne _ _ hm] at hl ⊢; exact ih hl

theorem ET.lookup_fn {E : Enums} {ρ : Env} {Γ : TyEnv} (h : ET E ρ Γ) {n : String} {Γd : TyEnv}
    (hl : fieldLookup Γ n = some (.fn Γd)) : ∃ ρd, fieldLookup ρ n = some (.scope ρd) ∧ ET E ρd Γd := by
  induction h with
  | nil => simp [fieldLookup] at hl
  | @val v t' ρ Γ m _ _ ih =>
    by_cases hm : m = n
    · subst hm; rw [fieldLookup_eq] at hl; cases hl
    · rw [fieldLookup_ne _ _ hm] at hl ⊢; exact ih hl
  | @fn ρd Γd' ρ Γ m hd _ _ ih =>
    by_cases hm : m = n
    · subst hm; rw [fieldLookup_eq] at hl ⊢; cases hl; exact ⟨ρd, rfl, hd⟩
    · rw [fieldLookup_ne _ _ hm] at hl ⊢; exact ih hl

theorem All2.length_eq {α β : Type} {R : α → β → Prop} {as : List α} {bs : List β} (h : All2 R as bs) :
    as.length = bs.length := by
  induction h with
  | nil => rfl
  | cons _ _ ih => simp [ih]

mutual
theorem bind_typed {E : Enums} {Γ Γ' : TyEnv} {ρ : Env} :
    ∀ (b : Binder) (t : Ty) (v : Val), bindTy Γ b t = some Γ' → VT E v t → ET E ρ Γ →
      ∃ ρ', bind ρ b v = .ok ρ' ∧ ET E ρ' Γ'
  | .name n, t, v, hb, hv, he => by
    simp [bindTy] at hb; subst hb
    exact ⟨_, by simp [bind], .val hv he⟩
  | .discard, t, v, hb, hv, he => by
    simp [bindTy] at hb; subst hb
    exact ⟨_, by simp [bind], he⟩
  | .tuple bs, t, v, hb, hv, he => by
    cases t with
    | tuple ts =>
      simp [bindTy] at hb
      cases hv with
      | tuple hvs =>
        have := bindAll_typed bs ts _ hb hvs he
        simpa [bind] using this
    | _ => simp [bindTy] at hb
theorem bindAll_typed {E : Enums} {Γ Γ' : TyEnv} {ρ : Env} :
    ∀ (bs : List Binder) (ts : List Ty) (vs : List Val), bindAllTy Γ bs ts = some Γ' → All2 (VT E) vs ts →
      ET E ρ Γ → ∃ ρ', bindAll ρ bs vs = .ok ρ' ∧ ET E ρ' Γ'
  | [], [], [], hb, _, he => by
    simp [bindAllTy] at hb; subst hb
    exact ⟨_, by simp [bindAll], he⟩
  | b :: bs, t :: ts, v :: vs, hb, hvs, he => by
    simp only [bindAllTy] at hb
    split at hb
    · rename_i Γm hm
      cases hvs with
      | cons hv hvs =>
        obtain ⟨ρm, hρm, hem⟩ := bind_typed b t v hm hv he
        obtain ⟨ρ', hρ', he'⟩ := bindAll_typed (Γ := Γm) bs ts vs hb hvs hem
        refine ⟨ρ', ?_, he'⟩
        simp [bindAll, hρm, hρ']
    · simp at hb
  | [], _ :: _, _, hb, _, _ => by simp [bindAllTy] at hb
  | _ :: _, [], _, hb, _, _ => by simp [bindAllTy] at hb
  | [], [], _ :: _, _, hvs, _ => by cases hvs
  | _ :: _, _ :: _, [], _, hvs, _ => by cases hvs
end

end Sx

namespace Sx

mutual
theorem Ty.eq_of_beq : ∀ (a b : Ty), Ty.beq a b = true → a = b
  | .bool, .bool, _ => rfl
  | .int, .int, _ => rfl
  | .str, .str, _ => rfl
  | .float, .float, _ => rfl
  | .bytes, .bytes, _ => rfl
  | .unit, .unit, _ => rfl
  | .never, .never, _ => rfl
  | .list a, .list b, h => by simp only [Ty.beq] at h; rw [Ty.eq_of_beq a b h]
  | .map a, .map b, h => by simp only [Ty.beq] at h; rw [Ty.eq_of_beq a b h]
  | .option a, .option b, h => by simp only [Ty.beq] at h; rw [Ty.eq_of_beq a b h]
  | .result a a', .result b b', h => by
    simp only [Ty.beq, Bool.and_eq_true] at h
    rw [Ty.eq_of_beq a b h.1, Ty.eq_of_beq a' b' h.2]
  | .tuple as, .tuple bs, h => by simp only [Ty.beq] at h; rw [Ty.eq_of_beqList as bs h]
  | .record as, .record bs, h => by simp only [Ty.beq] at h; rw [Ty.eq_of_beqFields as bs h]
  | .enum n, .enum m, h => by simp only [Ty.beq, beq_iff_eq] at h; rw [h]
  | .nominal n a, .nominal m b, h => by
    simp only [Ty.beq, Bool.and_eq_true, beq_iff_eq] at h
    rw [h.1, Ty.eq_of_beq a b h.2]
  | .var n, .var m, h => by simp only [Ty.beq, beq_iff_eq] at h; rw [h]
  | .bool, .int, h | .bool, .float, h | .bool, .bytes, h | .bool, .str, h | .bool, .unit, h | .bool, .list _, h | .bool, .map _, h | .bool, .tuple _, h | .bool, .option _, h | .bool, .result _ _, h | .bool, .record _, h | .bool, .enum _, h | .bool, .nominal _ _, h | .bool, .var _, h | .bool, .never, h => by simp [Ty.beq] at h
  | .int, .bool, h | .int, .float, h | .int, .bytes, h | .int, .str, h | .int, .unit, h | .int, .list _, h | .int, .map _, h | .int, .tuple _, h | .int, .option _, h | .int, .result _ _, h | .int, .record _, h | .int, .enum _, h | .int, .nominal _ _, h | .int, .var _, h | .int, .never, h => by simp [Ty.beq] at h
  | .float, .bool, h | .float, .int, h | .float, .bytes, h | .float, .str, h | .float, .unit, h | .float, .list _, h | .float, .map _, h | .float, .tuple _, h | .float, .option _, h | .float, .result _ _, h | .float, .record _, h | .float, .enum _, h | .float, .nominal _ _, h | .float, .var _, h | .float, .never, h => by simp [Ty.beq] at h
  | .bytes, .bool, h | .bytes, .int, h | .bytes, .float, h | .bytes, .str, h | .bytes, .unit, h | .bytes, .list _, h | .bytes, .map _, h | .bytes, .tuple _, h | .bytes, .option _, h | .bytes, .result _ _, h | .bytes, .record _, h | .bytes, .enum _, h | .bytes, .nominal _ _, h | .bytes, .var _, h | .bytes, .never, h => by simp [Ty.beq] at h
  | .str, .bool, h | .str, .int, h | .str, .float, h | .str, .bytes, h | .str, .unit, h | .str, .list _, h | .str, .map _, h | .str, .tuple _, h | .str, .option _, h | .str, .result _ _, h | .str, .record _, h | .str, .enum _, h | .str, .nominal _ _, h | .str, .var _, h | .str, .never, h => by simp [Ty.beq] at h
  | .unit, .bool, h | .unit, .int, h | .unit, .float, h | .unit, .bytes, h | .unit, .str, h | .unit, .list _, h | .unit, .map _, h | .unit, .tuple _, h | .unit, .option _, h | .unit, .result _ _, h | .unit, .record _, h | .unit, .enum _, h | .unit, .nominal _ _, h | .unit, .var _, h | .unit, .never, h => by simp [Ty.beq] at h
  | .list _, .bool, h | .list _, .int, h | .list _, .float, h | .list _, .bytes, h | .list _, .str, h | .list _, .unit, h | .list _, .map _, h | .list _, .tuple _, h | .list _, .option _, h | .list _, .result _ _, h | .list _, .record _, h | .list _, .enum _, h | .list _, .nominal _ _, h | .list _, .var _, h | .list _, .never, h => by simp [Ty.beq] at h
  | .map _, .bool, h | .map _, .int, h | .map _, .float, h | .map _, .bytes, h | .map _, .str, h | .map _, .unit, h | .map _, .list _, h | .map _, .tuple _, h | .map _, .option _, h | .map _, .result _ _, h | .map _, .record _, h | .map _, .enum _, h | .map _, .nominal _ _, h | .map _, .var _, h | .map _, .never, h => by simp [Ty.beq] at h
  | .tuple _, .bool, h | .tuple _, .int, h | .tuple _, .float, h | .tuple _, .bytes, h | .tuple _, .str, h | .tuple _, .unit, h | .tuple _, .list _, h | .tuple _, .map _, h | .tuple _, .option _, h | .tuple _, .result _ _, h | .tuple _, .record _, h | .tuple _, .enum _, h | .tuple _, .nominal _ _, h | .tuple _, .var _, h | .tuple _, .never, h => by simp [Ty.beq] at h
  | .option _, .bool, h | .option _, .int, h | .option _, .float, h | .option _, .bytes, h | .option _, .str, h | .option _, .unit, h | .option _, .list _, h | .option _, .map _, h | .option _, .tuple _, h | .option _, .result _ _, h | .option _, .record _, h | .option _, .enum _, h | .option _, .nominal _ _, h | .option _, .var _, h | .option _, .never, h => by simp [Ty.beq] at h
  | .result _ _, .bool, h | .result _ _, .int, h | .result _ _, .float, h | .result _ _, .bytes, h | .result _ _, .str, h | .result _ _, .unit, h | .result _ _, .list _, h | .result _ _, .map _, h | .result _ _, .tuple _, h | .result _ _, .option _, h | .result _ _, .record _, h | .result _ _, .enum _, h | .result _ _, .nominal _ _, h | .result _ _, .var _, h | .result _ _, .never, h => by simp [Ty.beq] at h
  | .record _, .bool, h | .record _, .int, h | .record _, .float, h | .record _, .bytes, h | .record _, .str, h | .record _, .unit, h | .record _, .list _, h | .record _, .map _, h | .record _, .tuple _, h | .record _, .option _, h | .record _, .result _ _, h | .record _, .enum _, h | .record _, .nominal _ _, h | .record _, .var _, h | .record _, .never, h => by simp [Ty.beq] at h
  | .enum _, .bool, h | .enum _, .int, h | .enum _, .float, h | .enum _, .bytes, h | .enum _, .str, h | .enum _, .unit, h | .enum _, .list _, h | .enum _, .map _, h | .enum _, .tuple _, h | .enum _, .option _, h | .enum _, .result _ _, h | .enum _, .record _, h | .enum _, .nominal _ _, h | .enum _, .var _, h | .enum _, .never, h => by simp [Ty.beq] at h
  | .nominal _ _, .bool, h | .nominal _ _, .int, h | .nominal _ _, .float, h | .nominal _ _, .bytes, h | .nominal _ _, .str, h | .nominal _ _, .unit, h | .nominal _ _, .list _, h | .nominal _ _, .map _, h | .nominal _ _, .tuple _, h | .nominal _ _, .option _, h | .nominal _ _, .result _ _, h | .nominal _ _, .record _, h | .nominal _ _, .enum _, h | .nominal _ _, .var _, h | .nominal _ _, .never, h => by simp [Ty.beq] at h
  | .var _, .bool, h | .var _, .int, h | .var _, .float, h | .var _, .bytes, h | .var _, .str, h | .var _, .unit, h | .var _, .list _, h | .var _, .map _, h | .var _, .tuple _, h | .var _, .option _, h | .var _, .result _ _, h | .var _, .record _, h | .var _, .enum _, h | .var _, .nominal _ _, h | .var _, .never, h => by simp [Ty.beq] at h
  | .never, .bool, h | .never, .int, h | .never, .float, h | .never, .bytes, h | .never, .str, h | .never, .unit, h | .never, .list _, h | .never, .map _, h | .never, .tuple _, h | .never, .option _, h | .never, .result _ _, h | .never, .record _, h | .never, .enum _, h | .never, .nominal _ _, h | .never, .var _, h => by simp [Ty.beq] at h
theorem Ty.eq_of_beqList : ∀ (as bs : List Ty), Ty.beqList as bs = true → as = bs
  | [], [], _ => rfl
  | a :: as, b :: bs, h => by
    simp only [Ty.beqList, Bool.and_eq_true] at h
    rw [Ty.eq_of_beq a b h.1, Ty.eq_of_beqList as bs h.2]
  | [], _ :: _, h => by simp [Ty.beqList] at h
  | _ :: _, [], h => by simp [Ty.beqList] at h
theorem Ty.eq_of_beqFields : ∀ (as bs : List (String × Ty)), Ty.beqFields as bs = true → as = bs
  | [], [], _ => rfl
  | (n, a) :: as, (m, b) :: bs, h => by
    simp only [Ty.beqFields, Bool.and_eq_true, beq_iff_eq] at h
    rw [h.1.1, Ty.eq_of_beq a b h.1.2, Ty.eq_of_beqFields as bs h.2]
  | [], _ :: _, h => by simp [Ty.beqFields] at h
  | _ :: _, [], h => by simp [Ty.beqFields] at h
end

theorem Ty.eq_of_beq' {a b : Ty} (h : (a == b) = true) : a = b := Ty.eq_of_beq a b h

def Aligned {α β : Type} (P : α → β → Prop) (l : List (String × α)) (l' : List (String × β)) : Prop :=
  l.map Prod.fst = l'.map Prod.fst ∧ All2 P (l.map Prod.snd) (l'.map Prod.snd)

theorem Aligned.nil {α β : Type} {P : α → β → Prop} : Aligned P [] [] := ⟨rfl, .nil⟩

theorem Aligned.cons {α β : Type} {P : α → β → Prop} {p : String × α} {q : String × β} {l l'}
    (h1 : p.1 = q.1) (h2 : P p.2 q.2) (h : Aligned P l l') : Aligned P (p :: l) (q :: l') :=
  ⟨by simp [h1, h.1], .cons h2 h.2⟩

theorem Aligned.uncons {α β : Type} {P : α → β → Prop} {p : String × α} {q : String × β} {l l'}
    (h : Aligned P (p :: l) (q :: l')) : p.1 = q.1 ∧ P p.2 q.2 ∧ Aligned P l l' := by
  obtain ⟨h1, h2⟩ := h
  simp at h1
  cases h2 with
  | cons hp hr => exact ⟨h1.1, hp, h1.2, hr⟩

theorem insertField_aligned {α β : Type} {P : α → β → Prop} {p : String × α} {q : String × β}
    (h1 : p.1 = q.1) (h2 : P p.2 q.2) :
    ∀ (l : List (String × α)) (l' : List (String × β)), Aligned P l l' →
      Aligned P (insertField p l) (insertField q l')
  | [], [], _ => Aligned.cons h1 h2 Aligned.nil
  | x :: xs, y :: ys, h => by
    obtain ⟨hx, hP, hr⟩ := h.uncons
    simp only [insertField]
    rw [h1, hx]
    split
    · exact Aligned.cons h1 h2 (Aligned.cons hx hP hr)
    · exact Aligned.cons hx hP (insertField_aligned h1 h2 xs ys hr)
  | [], _ :: _, h => by have := h.1; simp at this
  | _ :: _, [], h => by have := h.1; simp at this

theorem sortFields_aligned {α β : Type} {P : α → β → Prop} :
    ∀ (l : List (String × α)) (l' : List (String × β)), Aligned P l l' → Aligned P (sortFields l) (sortFields l')
  | [], [], h => h
  | p :: ps, q :: qs, h => by
    obtain ⟨h1, hP, hr⟩ := h.uncons
    exact insertField_aligned h1 hP _ _ (sortFields_aligned ps qs hr)
  | [], _ :: _, h => by have := h.1; simp at this
  | _ :: _, [], h => by have := h.1; simp at this

end Sx

namespace Sx

theorem All2.get {α β : Type} {R : α → β → Prop} : ∀ {as : List α} {bs : List β}, All2 R as bs →
    ∀ (i : Nat) (h1 : i < as.length) (h2 : i < bs.length), R as[i] bs[i]
  | _, _, .cons hr _, 0, _, _ => hr
  | _, _, .cons _ hrest, i + 1, h1, h2 => All2.get hrest i (by simpa using h1) (by simpa using h2)

end Sx
