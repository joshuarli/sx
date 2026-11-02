import Sx.Proofs.Lawful
import Sx.Proofs.Safety
import Sx.Check

namespace Sx

mutual
theorem checkVal_sound {E : Enums} (v : Val) (t : Ty) (h : checkVal E v t = true) : VT E v t := by
  match t with
  | .nominal n base => exact .nominal (checkVal_sound v base (by cases v <;> simpa [checkVal] using h))
  | .bool => cases v <;> simp [checkVal] at h; exact .bool
  | .int => cases v <;> simp [checkVal] at h; exact .int
  | .str => cases v <;> simp [checkVal] at h; exact .str
  | .unit => cases v <;> simp [checkVal] at h; exact .unit
  | .var _ => cases v <;> simp [checkVal] at h
  | .never => cases v <;> simp [checkVal] at h
  | .list t' =>
    cases v <;> simp only [checkVal] at h <;> (try contradiction)
    rename_i vs
    exact .list (checkVals_sound vs t' h)
  | .map t' =>
    cases v <;> simp only [checkVal] at h <;> (try contradiction)
    rename_i es
    exact .map (checkEntries_sound es t' h)
  | .tuple ts =>
    cases v <;> simp only [checkVal] at h <;> (try contradiction)
    rename_i vs
    exact .tuple (checkItems_sound vs ts h)
  | .record tfs =>
    cases v <;> simp only [checkVal] at h <;> (try contradiction)
    rename_i fs
    obtain ⟨hn, ha⟩ := checkFieldVals_sound fs tfs h
    exact .record hn ha
  | .option t' =>
    cases v <;> simp only [checkVal] at h <;> (try contradiction)
    · exact .some (checkVal_sound _ t' h)
    · exact .none
  | .result a b =>
    cases v <;> simp only [checkVal] at h <;> (try contradiction)
    · exact .ok (checkVal_sound _ a h)
    · exact .err (checkVal_sound _ b h)
  | .enum e' =>
    match v with
    | .variant e n none =>
      simp only [checkVal, Bool.and_eq_true, beq_iff_eq] at h
      obtain ⟨rfl, hp⟩ := h
      exact .variant hp
    | .variant e n (some p) =>
      simp only [checkVal, Bool.and_eq_true, beq_iff_eq] at h
      obtain ⟨rfl, hp⟩ := h
      split at hp
      · rename_i pt hpt
        exact .variantPayload hpt (checkVal_sound p pt hp)
      · cases hp
    | .bool _ | .int _ | .str _ | .unit | .list _ | .tuple _ | .record _ | .map _ | .some _ | .none | .ok _
    | .err _ => simp [checkVal] at h
termination_by (sizeOf v, sizeOf t)
theorem checkVals_sound {E : Enums} (vs : List Val) (t : Ty) (h : checkVals E vs t = true) : ∀ v ∈ vs, VT E v t := by
  match vs with
  | [] => simp
  | v :: vs =>
    simp only [checkVals, Bool.and_eq_true] at h
    intro x hx
    simp at hx
    rcases hx with rfl | hx
    · exact checkVal_sound x t h.1
    · exact checkVals_sound vs t h.2 x hx
termination_by (sizeOf vs, sizeOf t)
theorem checkItems_sound {E : Enums} (vs : List Val) (ts : List Ty) (h : checkItems E vs ts = true) :
    All2 (VT E) vs ts := by
  match vs, ts with
  | [], [] => exact .nil
  | v :: vs, t :: ts =>
    simp only [checkItems, Bool.and_eq_true] at h
    exact .cons (checkVal_sound v t h.1) (checkItems_sound vs ts h.2)
  | [], _ :: _ => simp [checkItems] at h
  | _ :: _, [] => simp [checkItems] at h
termination_by (sizeOf vs, sizeOf ts)
theorem checkFieldVals_sound {E : Enums} (fs : List (String × Val)) (tfs : List (String × Ty))
    (h : checkFieldVals E fs tfs = true) :
    fs.map Prod.fst = tfs.map Prod.fst ∧ All2 (VT E) (fs.map Prod.snd) (tfs.map Prod.snd) := by
  match fs, tfs with
  | [], [] => exact ⟨rfl, .nil⟩
  | (n, v) :: fs, (m, t) :: tfs =>
    simp only [checkFieldVals, Bool.and_eq_true, beq_iff_eq] at h
    obtain ⟨⟨rfl, hv⟩, hr⟩ := h
    obtain ⟨hn, ha⟩ := checkFieldVals_sound fs tfs hr
    exact ⟨by simp [hn], .cons (checkVal_sound v t hv) ha⟩
  | [], _ :: _ => simp [checkFieldVals] at h
  | _ :: _, [] => simp [checkFieldVals] at h
termination_by (sizeOf fs, sizeOf tfs)
theorem checkEntries_sound {E : Enums} (es : List (String × Val)) (t : Ty) (h : checkEntries E es t = true) :
    ∀ e ∈ es, VT E e.2 t := by
  match es with
  | [] => simp
  | (k, v) :: es =>
    simp only [checkEntries, Bool.and_eq_true] at h
    intro x hx
    simp at hx
    rcases hx with rfl | hx
    · exact checkVal_sound v t h.1
    · exact checkEntries_sound es t h.2 x hx
termination_by (sizeOf es, sizeOf t)
end

end Sx

namespace Sx

theorem expect_some {exp r : Option Ty} {t : Ty} (h : expect exp r = some t) :
    r = some t ∧ ∀ u, exp = some u → t = u := by
  cases exp with
  | none => simp [expect] at h; exact ⟨h, by simp⟩
  | some u =>
    cases r with
    | none => simp [expect] at h
    | some r =>
      simp only [expect] at h
      split at h
      · rename_i hr; cases h; exact ⟨by rw [eq_of_beq hr], fun _ hu => by cases hu; rfl⟩
      · cases h

theorem some_of_isSome {α : Type} {o : Option α} (h : o.isSome = true) : ∃ x, o = some x :=
  Option.isSome_iff_exists.mp h

theorem listElem_some {o : Option Ty} {a : Ty} (h : listElem o = some a) : o = some (.list a) := by
  unfold listElem at h; split at h <;> simp_all

theorem eq_of_beq_some {x : Option Ty} {t : Ty} (h : (x == some t) = true) : x = some t := eq_of_beq h

end Sx

namespace Sx

mutual
theorem tc_sound {S : Sig} : ∀ (e : Expr) (Γ : TyEnv) (L : Option (List Ty)) (exp : Option Ty) (t : Ty),
    tc S Γ L e exp = some t → HasType S Γ L e t ∧ ∀ u, exp = some u → t = u
  | .recur args, Γ, L, exp, t, h => by
    cases exp with
    | none => simp [tc] at h
    | some t0 =>
      cases L with
      | none => simp [tc] at h
      | some ts =>
        simp only [tc] at h
        split at h
        · rename_i hc; cases h
          exact ⟨.recur (checkAll_sound args Γ ts hc), fun u hu => by cases hu; rfl⟩
        · cases h
  | .let_ b v body, Γ, L, exp, t, h => by
    simp only [tc] at h
    split at h
    · rename_i tv hv
      split at h
      · rename_i Γ' hb
        obtain ⟨hbody, hexp⟩ := tc_sound body Γ' L exp t h
        exact ⟨.let_ (tc_sound v Γ none none tv hv).1 hb hbody, hexp⟩
      · cases h
    · cases h
  | .localFn n body, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hb, hexp⟩ := tc_sound body _ L exp t h
    exact ⟨.localFn hb, hexp⟩
  | .if_ c th el, Γ, L, exp, t, h => by
    simp only [tc] at h
    split at h
    · rename_i hc
      obtain ⟨x, hx⟩ := some_of_isSome hc
      obtain ⟨hct, hcu⟩ := tc_sound c Γ none (some .bool) x hx
      rw [hcu .bool rfl] at hct
      cases exp with
      | some t0 =>
        simp only at h
        split at h
        · rename_i hb
          simp only [Bool.and_eq_true] at hb
          obtain ⟨y, hy⟩ := some_of_isSome hb.1
          obtain ⟨z, hz⟩ := some_of_isSome hb.2
          obtain ⟨hty, hyu⟩ := tc_sound th Γ L (some t0) y hy
          obtain ⟨htz, hzu⟩ := tc_sound el Γ L (some t0) z hz
          rw [hyu t0 rfl] at hty; rw [hzu t0 rfl] at htz
          cases h
          exact ⟨.if_ hct hty htz, fun u hu => by cases hu; rfl⟩
        · cases h
      | none =>
        simp only at h
        split at h
        · rename_i t0 h0
          split at h
          · rename_i he
            obtain ⟨z, hz⟩ := some_of_isSome he
            obtain ⟨htz, hzu⟩ := tc_sound el Γ L (some t0) z hz
            rw [hzu t0 rfl] at htz
            simp only [Option.some.injEq] at h; rw [← h]
            exact ⟨.if_ hct (tc_sound th Γ L none t0 h0).1 htz, fun u hu => by cases hu⟩
          · cases h
        · split at h
          · rename_i t0 h0
            split at h
            · rename_i he
              obtain ⟨z, hz⟩ := some_of_isSome he
              obtain ⟨htz, hzu⟩ := tc_sound th Γ L (some t0) z hz
              rw [hzu t0 rfl] at htz
              simp only [Option.some.injEq] at h; rw [← h]
              exact ⟨.if_ hct htz (tc_sound el Γ L none t0 h0).1, fun u hu => by cases hu⟩
            · cases h
          · cases h
    · cases h
  | .match_ sc cs, Γ, L, exp, t, h => by
    simp only [tc] at h
    split at h
    · rename_i ts hs
      have hst := (tc_sound sc Γ none none ts hs).1
      split at h
      · rename_i hx
        cases exp with
        | some t0 =>
          simp only at h
          split at h
          · rename_i hcl; simp only [Option.some.injEq] at h; subst h
            exact ⟨.match_ hst (checkClauses_sound cs Γ L ts t0 hcl) hx, fun u hu => by cases hu; rfl⟩
          · cases h
        | none =>
          simp only at h
          split at h
          · rename_i t0 _
            split at h
            · rename_i hcl; simp only [Option.some.injEq] at h; rw [← h]
              exact ⟨.match_ hst (checkClauses_sound cs Γ L ts t0 hcl) hx, fun u hu => by cases hu⟩
            · cases h
          · cases h
      · cases h
    · cases h
  | .loop m bs init body, Γ, L, exp, t, h => by
    simp only [tc] at h
    split at h
    · rename_i ts hts
      split at h
      · rename_i Γ' hb
        obtain ⟨hbody, hexp⟩ := tc_sound body Γ' (some ts) exp t h
        exact ⟨.loop (inferAll_sound init Γ ts hts) hb hbody, hexp⟩
      · cases h
    · cases h
  | .lit v ty, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i hv; cases hr; exact ⟨.lit (checkVal_sound v ty hv), hexp⟩
    · cases hr
  | .var n, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i ty hl; cases hr; exact ⟨.var hl, hexp⟩
    · cases hr
  | .applyLocal n (.lam ps tys res body cap) args, Γ, L, exp, t, h => by
    unfold tc at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i Γd hl
      split at hr
      · rename_i Γ' hb
        split at hr
        · rename_i hc
          simp only [Bool.and_eq_true] at hc
          obtain ⟨x, hx⟩ := some_of_isSome hc.1
          obtain ⟨hbt, hbu⟩ := tc_sound body Γ' none (some res) x hx
          rw [hbu res rfl] at hbt
          cases hr
          exact ⟨.applyLocal hl hb hbt (checkAll_sound args Γ tys hc.2), hexp⟩
        · cases hr
      · cases hr
    · cases hr
  | .prelude op args, Γ, L, exp, t, h => by
    unfold tc at h
    obtain ⟨hr, hexp⟩ := expect_some h
    refine ⟨?_, hexp⟩
    split at hr
    · rename_i hho
      split at hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i b hb; cases hr
            exact .map (lamResult_sound f Γ [a] b hb) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i hb; cases hr
            exact .filter (lamResult_sound f Γ [a] .bool (eq_of_beq_some hb)) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i b hb; cases hr
            exact .keep (lamResult_sound f Γ [a] _ hb) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i hb; cases hr
            exact .any (lamResult_sound f Γ [a] .bool (eq_of_beq_some hb)) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i hb; cases hr
            exact .all (lamResult_sound f Γ [a] .bool (eq_of_beq_some hb)) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f init xs
        split at hr
        · rename_i acc a hacc ha
          split at hr
          · rename_i hb; simp only [Option.some.injEq] at hr; subst hr
            exact .reduce (lamResult_sound f Γ [acc, a] acc (eq_of_beq_some hb))
              (tc_sound init Γ none none acc hacc).1 (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i k hk
            split at hr
            · rename_i hsk; cases hr
              exact .sortBy (lamResult_sound f Γ [a] k hk) hsk (tc_sound xs Γ none none _ (listElem_some ha)).1
            · cases hr
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i k hk; cases hr
            exact .distinctBy (lamResult_sound f Γ [a] k hk) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · rename_i f xs
        split at hr
        · rename_i a ha
          split at hr
          · rename_i k hk; cases hr
            exact .groupBy (lamResult_sound f Γ [a] k hk) (tc_sound xs Γ none none _ (listElem_some ha)).1
          · cases hr
        · cases hr
      · cases hr
    · rename_i hho
      split at hr
      · rename_i ts hts
        exact .prelude (inferAll_sound args Γ ts hts) hr (by simpa using hho)
      · cases hr
  | .core fname args, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i sig hsig
      split at hr
      · rename_i hc; cases hr
        exact ⟨.core hsig (checkAll_sound args Γ sig.params hc), hexp⟩
      · cases hr
    · cases hr
  | .call c arg, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i sig hsig
      split at hr
      · rename_i hc
        obtain ⟨x, hx⟩ := some_of_isSome hc
        obtain ⟨hat, hau⟩ := tc_sound arg Γ none (some sig.input) x hx
        rw [hau _ rfl] at hat
        cases hr
        exact ⟨.call hsig hat, hexp⟩
      · cases hr
    · cases hr
  | .get r fld, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i fs hfs
      exact ⟨.get (tc_sound r Γ none none _ hfs).1 hr, hexp⟩
    · cases hr
  | .mkRecord fs, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i tfs htfs; cases hr
      exact ⟨.mkRecord (inferFields_sound fs Γ tfs htfs), hexp⟩
    · cases hr
  | .mkList es ty, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i hc; cases hr
      exact ⟨.mkList (checkEach_sound es Γ ty hc), hexp⟩
    · cases hr
  | .mkTuple es, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i ts hts; cases hr
      exact ⟨.mkTuple (inferAll_sound es Γ ts hts), hexp⟩
    · cases hr
  | .wrap k e ty, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    refine ⟨?_, hexp⟩
    split at hr
    · rename_i a
      split at hr
      · rename_i hc
        obtain ⟨x, hx⟩ := some_of_isSome hc
        obtain ⟨het, heu⟩ := tc_sound e Γ none (some a) x hx
        rw [heu _ rfl] at het
        cases hr; exact .wrapSome het
      · cases hr
    · rename_i a b
      split at hr
      · rename_i hc
        obtain ⟨x, hx⟩ := some_of_isSome hc
        obtain ⟨het, heu⟩ := tc_sound e Γ none (some a) x hx
        rw [heu _ rfl] at het
        cases hr; exact .wrapOk het
      · cases hr
    · rename_i a b
      split at hr
      · rename_i hc
        obtain ⟨x, hx⟩ := some_of_isSome hc
        obtain ⟨het, heu⟩ := tc_sound e Γ none (some b) x hx
        rw [heu _ rfl] at het
        cases hr; exact .wrapErr het
      · cases hr
    · cases hr
  | .mkVariant en n none, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i hv; cases hr; exact ⟨.mkVariant (eq_of_beq hv), hexp⟩
    · cases hr
  | .mkVariant en n (some p), Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i pt hpt
      split at hr
      · rename_i hc
        obtain ⟨x, hx⟩ := some_of_isSome hc
        obtain ⟨hpt', hpu⟩ := tc_sound p Γ none (some pt) x hx
        rw [hpu _ rfl] at hpt'
        cases hr; exact ⟨.mkVariantPayload hpt hpt', hexp⟩
      · cases hr
    · cases hr
  | .coerce e c, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i s hs
      exact ⟨.coerce (tc_sound e Γ none none s hs).1 hr, hexp⟩
    · cases hr
  | .attempt e, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i s hs; cases hr
      exact ⟨.attempt (tc_sound e Γ none none s hs).1, hexp⟩
    · cases hr
  | .retry ds ks e, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    exact ⟨.retry (tc_sound e Γ none none t hr).1, hexp⟩
  | .waitUntil is p e, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i s hs
      split at hr
      · rename_i hp; simp only [Option.some.injEq] at hr; subst hr
        exact ⟨.waitUntil (lamResult_sound p Γ [s] .bool (eq_of_beq_some hp)) (tc_sound e Γ none none s hs).1, hexp⟩
      · cases hr
    · cases hr
  | .par bs, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i ts hts; cases hr
      exact ⟨.par (inferAll_sound bs Γ ts hts), hexp⟩
    · cases hr
  | .parMap lim f xs, Γ, L, exp, t, h => by
    simp only [tc] at h
    obtain ⟨hr, hexp⟩ := expect_some h
    split at hr
    · rename_i a ha
      split at hr
      · rename_i b hb; cases hr
        exact ⟨.parMap (lamResult_sound f Γ [a] b hb) (tc_sound xs Γ none none _ (listElem_some ha)).1, hexp⟩
      · cases hr
    · cases hr
  | .lam .., _, _, _, _, h => by simp [tc] at h
  | .applyLocal _ (.lit ..) _, _, _, _, _, h | .applyLocal _ (.var _) _, _, _, _, _, h
  | .applyLocal _ (.let_ ..) _, _, _, _, _, h | .applyLocal _ (.localFn ..) _, _, _, _, _, h
  | .applyLocal _ (.if_ ..) _, _, _, _, _, h | .applyLocal _ (.match_ ..) _, _, _, _, _, h
  | .applyLocal _ (.applyLocal ..) _, _, _, _, _, h | .applyLocal _ (.prelude ..) _, _, _, _, _, h
  | .applyLocal _ (.core ..) _, _, _, _, _, h | .applyLocal _ (.call ..) _, _, _, _, _, h
  | .applyLocal _ (.get ..) _, _, _, _, _, h | .applyLocal _ (.mkRecord _) _, _, _, _, _, h
  | .applyLocal _ (.mkList ..) _, _, _, _, _, h | .applyLocal _ (.mkTuple _) _, _, _, _, _, h
  | .applyLocal _ (.wrap ..) _, _, _, _, _, h | .applyLocal _ (.mkVariant ..) _, _, _, _, _, h
  | .applyLocal _ (.coerce ..) _, _, _, _, _, h | .applyLocal _ (.attempt _) _, _, _, _, _, h
  | .applyLocal _ (.retry ..) _, _, _, _, _, h | .applyLocal _ (.waitUntil ..) _, _, _, _, _, h
  | .applyLocal _ (.loop ..) _, _, _, _, _, h | .applyLocal _ (.recur _) _, _, _, _, _, h
  | .applyLocal _ (.par _) _, _, _, _, _, h | .applyLocal _ (.parMap ..) _, _, _, _, _, h => by simp [tc] at h
theorem inferAll_sound {S : Sig} : ∀ (es : List Expr) (Γ : TyEnv) (ts : List Ty),
    inferAll S Γ es = some ts → HasTypes S Γ es ts
  | [], Γ, ts, h => by simp [inferAll] at h; subst h; exact .nil
  | e :: es, Γ, ts, h => by
    simp only [inferAll] at h
    split at h
    · rename_i t ts' ht hts; cases h
      exact .cons (tc_sound e Γ none none t ht).1 (inferAll_sound es Γ ts' hts)
    · cases h
theorem checkAll_sound {S : Sig} : ∀ (es : List Expr) (Γ : TyEnv) (ts : List Ty),
    checkAll S Γ es ts = true → HasTypes S Γ es ts
  | [], Γ, [], _ => .nil
  | e :: es, Γ, t :: ts, h => by
    simp only [checkAll, Bool.and_eq_true] at h
    obtain ⟨x, hx⟩ := some_of_isSome h.1
    obtain ⟨het, heu⟩ := tc_sound e Γ none (some t) x hx
    rw [heu t rfl] at het
    exact .cons het (checkAll_sound es Γ ts h.2)
  | [], Γ, _ :: _, h => by simp [checkAll] at h
  | _ :: _, Γ, [], h => by simp [checkAll] at h
theorem checkEach_sound {S : Sig} : ∀ (es : List Expr) (Γ : TyEnv) (t : Ty),
    checkEach S Γ es t = true → AllT S Γ es t
  | [], Γ, t, _ => .nil
  | e :: es, Γ, t, h => by
    simp only [checkEach, Bool.and_eq_true] at h
    obtain ⟨x, hx⟩ := some_of_isSome h.1
    obtain ⟨het, heu⟩ := tc_sound e Γ none (some t) x hx
    rw [heu t rfl] at het
    exact .cons het (checkEach_sound es Γ t h.2)
theorem inferFields_sound {S : Sig} : ∀ (fs : List (String × Expr)) (Γ : TyEnv) (tfs : List (String × Ty)),
    inferFields S Γ fs = some tfs → Fields S Γ fs tfs
  | [], Γ, tfs, h => by simp [inferFields] at h; subst h; exact .nil
  | (n, e) :: fs, Γ, tfs, h => by
    simp only [inferFields] at h
    split at h
    · rename_i t tfs' ht hts; cases h
      exact .cons (tc_sound e Γ none none t ht).1 (inferFields_sound fs Γ tfs' hts)
    · cases h
theorem checkClauses_sound {S : Sig} : ∀ (cs : List (Pat × Expr)) (Γ : TyEnv) (L : Option (List Ty)) (ts t : Ty),
    checkClauses S Γ L cs ts t = true → Clauses S Γ L cs ts t
  | [], Γ, L, ts, t, _ => .nil
  | (p, body) :: cs, Γ, L, ts, t, h => by
    simp only [checkClauses] at h
    split at h
    · rename_i Γ' hp
      simp only [Bool.and_eq_true] at h
      obtain ⟨x, hx⟩ := some_of_isSome h.1
      obtain ⟨hbt, hbu⟩ := tc_sound body Γ' L (some t) x hx
      rw [hbu t rfl] at hbt
      exact .cons hp hbt (checkClauses_sound cs Γ L ts t h.2)
    · cases h
theorem lamResult_sound {S : Sig} : ∀ (f : Expr) (Γ : TyEnv) (args : List Ty) (r : Ty),
    lamResult S Γ f args = some r → LamT S Γ f args r
  | .lam ps tys res body cap, Γ, args, r, h => by
    simp only [lamResult] at h
    split at h
    · rename_i htys
      have htys := eq_of_beq htys
      subst htys
      split at h
      · rename_i Γb hb
        split at h
        · rename_i Γ' hbind
          split at h
          · rename_i hc
            obtain ⟨x, hx⟩ := some_of_isSome hc
            obtain ⟨hbt, hbu⟩ := tc_sound body Γ' none (some res) x hx
            rw [hbu res rfl] at hbt
            cases h
            exact .mk hb hbind hbt
          · cases h
        · cases h
      · cases h
    · cases h
  | .lit .., _, _, _, h | .var _, _, _, _, h | .let_ .., _, _, _, h | .localFn .., _, _, _, h
  | .if_ .., _, _, _, h | .match_ .., _, _, _, h | .applyLocal .., _, _, _, h | .prelude .., _, _, _, h
  | .core .., _, _, _, h | .call .., _, _, _, h | .get .., _, _, _, h | .mkRecord _, _, _, _, h
  | .mkList .., _, _, _, h | .mkTuple _, _, _, _, h | .wrap .., _, _, _, h | .mkVariant .., _, _, _, h
  | .coerce .., _, _, _, h | .attempt _, _, _, _, h | .retry .., _, _, _, h | .waitUntil .., _, _, _, h
  | .loop .., _, _, _, h | .recur _, _, _, _, h | .par _, _, _, _, h | .parMap .., _, _, _, h => by
    simp [lamResult] at h
end

theorem check_sound {S : Sig} {Γ : TyEnv} {L : Option (List Ty)} {e : Expr} {t : Ty}
    (h : check S Γ L e t = true) : HasType S Γ L e t := by
  obtain ⟨x, hx⟩ := some_of_isSome h
  obtain ⟨ht, hu⟩ := tc_sound e Γ L (some t) x hx
  rw [hu t rfl] at ht
  exact ht

theorem checkProgram_typed {p : Program} (h : checkProgram p = .ok ()) : HasType p.sig p.inputEnv none p.body p.result := by
  unfold checkProgram at h
  split at h
  · cases h
  · rename_i hc
    exact check_sound (by simpa using hc)

theorem checked_program_type_safety (p : Program) (hchecked : checkProgram p = .ok ()) (P : PureCtx)
    (hcores : CoresOk p.sig P.cores) (hpure : isPure p.body = true) (fuel : Nat) (ρ : Env)
    (hinputs : ET p.sig.enums ρ p.inputEnv) :
    match evalP P fuel ρ p.body with
    | .ok v => VT p.sig.enums v p.result
    | .error f => f.kind ≠ .internal :=
  pure_type_safety p.sig P hcores fuel ρ p.inputEnv p.body p.result hpure (checkProgram_typed hchecked) hinputs

end Sx
