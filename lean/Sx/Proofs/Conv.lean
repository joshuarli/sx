import Sx.Proofs.Lawful

namespace Sx

theorem convertEach_of {E : Enums} {c : Conv} {t u : Ty}
    (h : ∀ v, VT E v t → ∃ w, convert v c = .ok w ∧ VT E w u) :
    ∀ (vs : List Val), (∀ v ∈ vs, VT E v t) → ∃ ws, vs.mapM (fun v => convert v c) = .ok ws ∧ ∀ w ∈ ws, VT E w u
  | [], _ => ⟨[], rfl, by simp⟩
  | v :: vs, hvs => by
    obtain ⟨w, hw, hwt⟩ := h v (hvs v (by simp))
    obtain ⟨ws, hws, hwst⟩ := convertEach_of h vs (fun x hx => hvs x (by simp [hx]))
    refine ⟨w :: ws, ?_, ?_⟩
    · simp [List.mapM_cons, hw, hws]
    · intro x hx; simp at hx; rcases hx with rfl | hx; exact hwt; exact hwst x hx

theorem nullary_no_payload {E : Enums} {e n : String} {pt : Ty} (hn : nullaryEnum E e = true)
    (hp : variantPayload E e n = some (some pt)) : False := by
  simp only [nullaryEnum] at hn
  simp only [variantPayload] at hp
  split at hn
  · rename_i vs hvs
    rw [hvs] at hp
    rw [List.all_eq_true] at hn
    have := hn _ (fieldLookup_mem hp)
    simp at this
  · simp at hn

theorem record_lookup {E : Enums} :
    ∀ (fs : List (String × Val)) (tfs : List (String × Ty)) (n : String) (t : Ty),
      fs.map Prod.fst = tfs.map Prod.fst → All2 (VT E) (fs.map Prod.snd) (tfs.map Prod.snd) →
      fieldLookup tfs n = some t → ∃ w, fieldLookup fs n = some w ∧ VT E w t
  | [], [], _, _, _, _, h => by simp [fieldLookup] at h
  | (k, v) :: fs, (k', t') :: tfs, n, t, hn, ha, h => by
    simp at hn; obtain ⟨rfl, hn⟩ := hn
    cases ha with
    | cons hv ha =>
      by_cases hk : k = n
      · subst hk; rw [fieldLookup_eq] at h ⊢; cases h; exact ⟨v, rfl, hv⟩
      · rw [fieldLookup_ne _ _ hk] at h ⊢; exact record_lookup fs tfs n t hn ha h
  | [], _ :: _, _, _, hn, _, _ => by simp at hn
  | _ :: _, [], _, _, hn, _, _ => by simp at hn

mutual
theorem convert_typed {E : Enums} :
    ∀ (c : Conv) (s t : Ty) (v : Val), convTy E c s = some t → VT E v s → ∃ w, convert v c = .ok w ∧ VT E w t
  | .variantName, s, t, v, hc, hv => by
    cases s with
    | enum e =>
      simp only [convTy] at hc
      split at hc
      · rename_i hn
        cases hc
        cases hv with
        | @variant _ m _ => exact ⟨.str m, by simp [convert], .str⟩
        | variantPayload hp _ => exact (nullary_no_payload hn hp).elim
      · cases hc
    | _ => simp [convTy] at hc
  | .toSome none, s, t, v, hc, hv => by
    simp [convTy] at hc; subst hc
    cases v <;> exact ⟨_, by simp [convert], .some hv⟩
  | .toSome (some c), s, t, v, hc, hv => by
    simp only [convTy] at hc
    split at hc
    · rename_i u hu
      cases hc
      obtain ⟨w, hw, hwt⟩ := convert_typed c s u v hu hv
      refine ⟨.some w, ?_, .some hwt⟩
      cases v <;> simp [convert, hw]
    · cases hc
  | .eachField cs, s, t, v, hc, hv => by
    cases s with
    | record fs =>
      simp only [convTy] at hc
      split at hc
      · rename_i fs' hfs
        cases hc
        cases hv with
        | record hn ha =>
          obtain ⟨wfs, hw, hwn, hwa⟩ := convertFields_typed cs fs fs' _ hfs hn ha
          exact ⟨_, by simp [convert, hw], .record hwn hwa⟩
      · cases hc
    | _ => simp [convTy] at hc
  | .widenRecord ws, s, t, v, hc, hv => by
    cases s with
    | record fs =>
      simp only [convTy] at hc
      split at hc
      · rename_i fs' hfs
        cases hc
        cases hv with
        | @record vfs _ hn ha =>
          obtain ⟨wfs, hw, hwn, hwa⟩ := widenFields_typed vfs fs hn ha ws fs' hfs
          exact ⟨_, by simp [convert, hw], .record hwn hwa⟩
      · cases hc
    | _ => simp [convTy] at hc
  | .eachElement c, s, t, v, hc, hv => by
    cases s with
    | list a =>
      simp only [convTy] at hc
      split at hc
      · rename_i u hu
        cases hc
        cases hv with
        | list hvs =>
          obtain ⟨ws, hws, hwt⟩ := convertEach_of (fun x hx => convert_typed c a u x hu hx) _ hvs
          exact ⟨_, by simp [convert, convertEach, hws], .list hwt⟩
      · cases hc
    | _ => simp [convTy] at hc
  | .eachItem cs, s, t, v, hc, hv => by
    cases s with
    | tuple ts =>
      simp only [convTy] at hc
      split at hc
      · rename_i ts' hts
        cases hc
        cases hv with
        | tuple hvs =>
          obtain ⟨ws, hws, hwt⟩ := convertItems_typed cs ts ts' _ hts hvs
          exact ⟨_, by simp [convert, hws], .tuple hwt⟩
      · cases hc
    | _ => simp [convTy] at hc
theorem convertFields_typed {E : Enums} :
    ∀ (cs : List (String × Option Conv)) (fs fs' : List (String × Ty)) (vfs : List (String × Val)),
      convFieldsTy E cs fs = some fs' → vfs.map Prod.fst = fs.map Prod.fst →
      All2 (VT E) (vfs.map Prod.snd) (fs.map Prod.snd) →
      ∃ wfs, convertFields vfs cs = .ok wfs ∧ wfs.map Prod.fst = fs'.map Prod.fst ∧
        All2 (VT E) (wfs.map Prod.snd) (fs'.map Prod.snd)
  | [], fs, fs', vfs, hc, hn, ha => by
    cases fs with
    | nil =>
      simp [convFieldsTy] at hc; subst hc
      cases vfs with
      | nil => exact ⟨[], by simp [convertFields], rfl, .nil⟩
      | cons _ _ => simp at hn
    | cons _ _ => simp [convFieldsTy] at hc
  | (n, oc) :: cs, fs, fs', vfs, hc, hn, ha => by
    cases fs with
    | nil => cases oc <;> simp [convFieldsTy] at hc
    | cons f fs =>
      obtain ⟨m, t⟩ := f
      cases vfs with
      | nil => simp at hn
      | cons vf vfs =>
        obtain ⟨k, v⟩ := vf
        simp at hn
        obtain ⟨hk, hn⟩ := hn
        subst hk
        cases ha with
        | cons hv ha =>
        cases oc with
        | none =>
          simp only [convFieldsTy] at hc
          split at hc
          · rename_i hnm
            subst hnm
            split at hc
            · rename_i fs'' hfs
              cases hc
              obtain ⟨wfs, hw, hwn, hwa⟩ := convertFields_typed cs fs fs'' vfs hfs hn ha
              exact ⟨(n, v) :: wfs, by simp [convertFields, hw], by simp [hwn], .cons hv hwa⟩
            · cases hc
          · cases hc
        | some c =>
          simp only [convFieldsTy] at hc
          split at hc
          · rename_i hnm
            subst hnm
            split at hc
            · rename_i u fs'' hu hfs
              cases hc
              obtain ⟨w, hw, hwt⟩ := convert_typed c t u v hu hv
              obtain ⟨wfs, hws, hwn, hwa⟩ := convertFields_typed cs fs fs'' vfs hfs hn ha
              exact ⟨(n, w) :: wfs, by simp [convertFields, hw, hws], by simp [hwn], .cons hwt hwa⟩
            · cases hc
          · cases hc
theorem widenFields_typed {E : Enums} (vfs : List (String × Val)) (fs : List (String × Ty))
    (hn : vfs.map Prod.fst = fs.map Prod.fst) (ha : All2 (VT E) (vfs.map Prod.snd) (fs.map Prod.snd)) :
    ∀ (ws : List (String × Bool × Option Conv × Ty)) (fs' : List (String × Ty)), widenTy E fs ws = some fs' →
      ∃ wfs, widenFields vfs ws = .ok wfs ∧ wfs.map Prod.fst = fs'.map Prod.fst ∧
        All2 (VT E) (wfs.map Prod.snd) (fs'.map Prod.snd)
  | [], fs', h => by
    simp [widenTy] at h; subst h
    exact ⟨[], by simp [widenFields], rfl, .nil⟩
  | (n, false, oc, t) :: ws, fs', h => by
    simp only [widenTy] at h
    split at h
    · rename_i a rest hrest
      cases h
      obtain ⟨wfs, hw, hwn, hwa⟩ := widenFields_typed vfs fs hn ha ws rest hrest
      exact ⟨(n, .none) :: wfs, by simp [widenFields, hw], by simp [hwn], .cons .none hwa⟩
    · cases h
  | (n, true, none, t) :: ws, fs', h => by
    simp only [widenTy] at h
    split at h
    · rename_i sty rest hs hrest
      split at h
      · rename_i heq
        have heq := Ty.eq_of_beq _ _ heq
        subst heq
        cases h
        obtain ⟨w, hwl, hwt⟩ := record_lookup vfs fs n _ hn ha hs
        obtain ⟨wfs, hw, hwn, hwa⟩ := widenFields_typed vfs fs hn ha ws rest hrest
        exact ⟨(n, w) :: wfs, by simp [widenFields, hwl, hw], by simp [hwn], .cons hwt hwa⟩
      · cases h
    · cases h
  | (n, true, some c, t) :: ws, fs', h => by
    simp only [widenTy] at h
    split at h
    · rename_i sty rest hs hrest
      split at h
      · rename_i heq
        have heq : convTy E c sty = some t := eq_of_beq heq
        cases h
        obtain ⟨w0, hwl, hwt0⟩ := record_lookup vfs fs n _ hn ha hs
        obtain ⟨w, hcw, hwt⟩ := convert_typed c sty t w0 heq hwt0
        obtain ⟨wfs, hw, hwn, hwa⟩ := widenFields_typed vfs fs hn ha ws rest hrest
        exact ⟨(n, w) :: wfs, by simp [widenFields, hwl, hcw, hw], by simp [hwn], .cons hwt hwa⟩
      · cases h
    · cases h
theorem convertItems_typed {E : Enums} :
    ∀ (cs : List (Option Conv)) (ts ts' : List Ty) (vs : List Val),
      convItemsTy E cs ts = some ts' → All2 (VT E) vs ts → ∃ ws, convertItems vs cs = .ok ws ∧ All2 (VT E) ws ts'
  | [], ts, ts', vs, hc, hv => by
    cases ts with
    | nil =>
      simp [convItemsTy] at hc; subst hc
      cases hv with
      | nil => exact ⟨[], by simp [convertItems], .nil⟩
    | cons _ _ => simp [convItemsTy] at hc
  | oc :: cs, ts, ts', vs, hc, hv => by
    cases ts with
    | nil => cases oc <;> simp [convItemsTy] at hc
    | cons t ts =>
      cases hv with
      | @cons v t vs ts hv hvs =>
      cases oc with
      | none =>
        simp only [convItemsTy] at hc
        split at hc
        · rename_i ts'' hts
          cases hc
          obtain ⟨ws, hws, hwt⟩ := convertItems_typed cs ts ts'' vs hts hvs
          exact ⟨v :: ws, by simp [convertItems, hws], .cons hv hwt⟩
        · cases hc
      | some c =>
        simp only [convItemsTy] at hc
        split at hc
        · rename_i u ts'' hu hts
          cases hc
          obtain ⟨w, hw, hwt⟩ := convert_typed c t u v hu hv
          obtain ⟨ws, hws, hwst⟩ := convertItems_typed cs ts ts'' vs hts hvs
          exact ⟨w :: ws, by simp [convertItems, hw, hws], .cons hwt hwst⟩
        · cases hc
end

end Sx
