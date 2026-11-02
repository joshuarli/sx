import Sx.Proofs.Basic

namespace Sx

theorem checkedInt_safe {E : Enums} (i : Int) : SafeE (fun v => VT E v .int) (checkedInt i) := by
  unfold checkedInt
  split
  · exact VT.int
  · simp [SafeE, failWith]

theorem checkSize_safe (l : Nat) (v : Val) : SafeE (fun _ => True) (checkSize l v) := by
  unfold checkSize
  split
  · simp [SafeE, failWith]
  · trivial

theorem sized {E : Enums} {l : Nat} {v : Val} {t : Ty} (h : VT E v t) :
    SafeE (fun w => VT E w t) (do checkSize l v; pure v) :=
  SafeE.bind (checkSize_safe l v) (fun _ _ => h)

theorem flattenLists_typed {E : Enums} {t : Ty} :
    ∀ (vs : List Val), (∀ v ∈ vs, VT E v (.list t)) → ∃ ws, flattenLists vs = .ok ws ∧ ∀ w ∈ ws, VT E w t
  | [], _ => ⟨[], rfl, by simp⟩
  | v :: vs, h => by
    have hv := h v (by simp)
    cases hv with
    | @list xs _ hxs =>
      obtain ⟨ws, hws, hw⟩ := flattenLists_typed vs (fun x hx => h x (by simp [hx]))
      refine ⟨xs ++ ws, by simp [flattenLists, hws], ?_⟩
      intro w hw'
      simp at hw'
      rcases hw' with hw' | hw'
      · exact hxs w hw'
      · exact hw w hw'

theorem pairsOf_typed {E : Enums} {t : Ty} :
    ∀ (vs : List Val), (∀ v ∈ vs, VT E v (.tuple [.str, t])) →
      ∃ ps, pairsOf vs = .ok ps ∧ ∀ p ∈ ps, VT E p.2 t
  | [], _ => ⟨[], rfl, by simp⟩
  | v :: vs, h => by
    have hv := h v (by simp)
    cases hv with
    | tuple hxs =>
      rcases hxs with _ | ⟨hk, _ | ⟨hx, _ | _⟩⟩
      cases hk with
      | @str k =>
      obtain ⟨ps, hps, hp⟩ := pairsOf_typed vs (fun x hx => h x (by simp [hx]))
      refine ⟨_ :: ps, by simp only [pairsOf, hps, except_ok_bind, except_pure]; rfl, ?_⟩
      intro p hp'
      simp at hp'
      rcases hp' with rfl | hp'
      · exact hx
      · exact hp p hp'

theorem mapInsert_mem {k : String} {v : Val} :
    ∀ (acc : List (String × Val)) (e : String × Val), e ∈ mapInsert k v acc → e = (k, v) ∨ e ∈ acc
  | [], e, h => by simp [mapInsert] at h; exact .inl h
  | (k', v') :: rest, e, h => by
    unfold mapInsert at h
    split at h
    · simp at h; rcases h with h | h; exact .inl h; exact .inr (by simp [h])
    · split at h
      · simp at h; rcases h with h | h | h
        · exact .inl h
        · exact .inr (by simp [h])
        · exact .inr (by simp [h])
      · simp at h
        rcases h with h | h
        · exact .inr (by simp [h])
        · rcases mapInsert_mem rest e h with h | h
          · exact .inl h
          · exact .inr (by simp [h])

theorem makeMap_mem : ∀ (ps acc : List (String × Val)) (e : String × Val),
    e ∈ ps.foldl (fun acc (p : String × Val) => mapInsert p.1 p.2 acc) acc → e ∈ acc ∨ e ∈ ps
  | [], acc, e, h => .inl h
  | p :: ps, acc, e, h => by
    simp only [List.foldl] at h
    rcases makeMap_mem ps _ e h with h | h
    · rcases mapInsert_mem acc e h with h | h
      · exact .inr (by simp [h])
      · exact .inl h
    · exact .inr (by simp [h])

theorem makeMap_typed {E : Enums} {t : Ty} {ps : List (String × Val)} (h : ∀ p ∈ ps, VT E p.2 t) :
    ∀ e ∈ makeMap ps, VT E e.2 t := by
  intro e he
  unfold makeMap at he
  rcases makeMap_mem ps [] e he with he | he
  · simp at he
  · exact h e he

theorem cmpKeys_orderable {E : Enums} {a b : Ty} {x y : Val} (h : orderablePair a b = true)
    (hx : VT E x a) (hy : VT E y b) : ∃ o, cmpKeys x y = .ok o := by
  cases a <;> cases b <;> simp [orderablePair] at h <;> cases hx <;> cases hy <;> exact ⟨_, rfl⟩

theorem arith_fail {α : Type} {P : α → Prop} {m : String} : SafeE P (failWith .arithmetic m : Except Failure α) :=
  SafeE.fail (by decide)

theorem range_fail {α : Type} {P : α → Prop} {m : String} : SafeE P (failWith .outOfRange m : Except Failure α) :=
  SafeE.fail (by decide)

theorem applyOp_typed {E : Enums} {limit : Nat} (op : Op) (ts : List Ty) (t : Ty) (vs : List Val)
    (h : opTy op ts = some t) (hvs : All2 (VT E) vs ts) : SafeE (fun v => VT E v t) (applyOp limit op vs) := by
  unfold opTy at h
  split at h <;> (try simp at h)
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    exact checkedInt_safe _
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | _⟩
    exact checkedInt_safe _
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    exact checkedInt_safe _
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    exact checkedInt_safe _
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    simp only [applyOp]; split
    · exact arith_fail
    · exact checkedInt_safe _
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    simp only [applyOp]; split
    · exact arith_fail
    · exact checkedInt_safe _
  next =>
    subst h; rcases hvs with _ | ⟨_, _ | ⟨_, _ | _⟩⟩
    exact VT.bool
  next =>
    subst h; rcases hvs with _ | ⟨_, _ | ⟨_, _ | _⟩⟩
    exact VT.bool
  next =>
    obtain ⟨hc, rfl⟩ := h; rcases hvs with _ | ⟨hx, _ | ⟨hy, _ | _⟩⟩
    obtain ⟨o, ho⟩ := cmpKeys_orderable hc hx hy
    simp only [applyOp, ho, except_ok_bind]; exact VT.bool
  next =>
    obtain ⟨hc, rfl⟩ := h; rcases hvs with _ | ⟨hx, _ | ⟨hy, _ | _⟩⟩
    obtain ⟨o, ho⟩ := cmpKeys_orderable hc hx hy
    simp only [applyOp, ho, except_ok_bind]; exact VT.bool
  next =>
    obtain ⟨hc, rfl⟩ := h; rcases hvs with _ | ⟨hx, _ | ⟨hy, _ | _⟩⟩
    obtain ⟨o, ho⟩ := cmpKeys_orderable hc hx hy
    simp only [applyOp, ho, except_ok_bind]; exact VT.bool
  next =>
    obtain ⟨hc, rfl⟩ := h; rcases hvs with _ | ⟨hx, _ | ⟨hy, _ | _⟩⟩
    obtain ⟨o, ho⟩ := cmpKeys_orderable hc hx hy
    simp only [applyOp, ho, except_ok_bind]; exact VT.bool
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | _⟩
    exact VT.bool
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    exact VT.bool
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨⟨⟩, _ | _⟩⟩
    exact VT.bool
  next =>
    obtain ⟨_, rfl⟩ := h
    exact sized VT.str
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | _⟩
    exact VT.int
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | _⟩
    exact VT.int
  next =>
    subst h; rcases hvs with _ | ⟨hxs, _ | _⟩
    cases hxs with | @list xs _ hxs =>
    cases xs with
    | nil => exact range_fail
    | cons x _ => exact hxs x (by simp)
  next =>
    subst h; rcases hvs with _ | ⟨hxs, _ | _⟩
    cases hxs with | @list xs _ hxs =>
    exact VT.list (fun x hx => hxs x (List.mem_of_mem_drop hx))
  next =>
    subst h; rcases hvs with _ | ⟨hxs, _ | ⟨⟨⟩, _ | _⟩⟩
    cases hxs with | @list xs _ hxs =>
    simp only [applyOp]; split
    · exact hxs _ (List.getElem_mem _)
    · exact range_fail
  next =>
    subst h; rcases hvs with _ | ⟨⟨⟩, _ | ⟨hxs, _ | _⟩⟩
    cases hxs with | @list xs _ hxs =>
    simp only [applyOp]; split
    · exact arith_fail
    · exact VT.list (fun x hx => hxs x (List.mem_of_mem_take hx))
  next =>
    obtain ⟨hab, rfl⟩ := h
    have hab := Ty.eq_of_beq' hab; subst hab
    rcases hvs with _ | ⟨hxs, _ | ⟨hys, _ | _⟩⟩
    cases hxs with | @list xs _ hxs =>
    cases hys with | @list ys _ hys =>
    refine sized (VT.list ?_)
    intro x hx; simp at hx; rcases hx with hx | hx; exact hxs x hx; exact hys x hx
  next =>
    subst h; rcases hvs with _ | ⟨hxs, _ | _⟩
    cases hxs with | @list xs _ hxs =>
    obtain ⟨ws, hws, hw⟩ := flattenLists_typed xs hxs
    show SafeE _ (do let ws ← flattenLists xs; pure (Val.list ws))
    rw [hws]; exact VT.list hw
  next =>
    obtain ⟨_, rfl⟩ := h; rcases hvs with _ | ⟨hxs, _ | ⟨_, _ | _⟩⟩
    cases hxs with | @list xs _ hxs =>
    exact VT.bool
  next =>
    subst h; rcases hvs with _ | ⟨hes, _ | ⟨⟨⟩, _ | _⟩⟩
    cases hes with | @map es _ hes =>
    exact VT.bool
  next =>
    subst h; rcases hvs with _ | ⟨hes, _ | ⟨⟨⟩, _ | _⟩⟩
    cases hes with | @map es _ hes =>
    simp only [applyOp, except_pure]
    split
    · rename_i w hw; exact VT.some (hes _ (fieldLookup_mem hw))
    · exact VT.none
  next =>
    obtain ⟨hvw, rfl⟩ := h
    have hvw := Ty.eq_of_beq' hvw; subst hvw
    rcases hvs with _ | ⟨hes, _ | ⟨⟨⟩, _ | ⟨hw, _ | _⟩⟩⟩
    cases hes with | @map es _ hes =>
    refine sized (VT.map (makeMap_typed ?_))
    intro p hp; simp at hp; rcases hp with hp | hp; exact hes p hp; subst hp; exact hw
  next =>
    subst h; rcases hvs with _ | ⟨hes, _ | ⟨⟨⟩, _ | _⟩⟩
    cases hes with | @map es _ hes =>
    exact VT.map (fun e he => hes e (List.mem_filter.mp he).1)
  next =>
    subst h; rcases hvs with _ | ⟨hes, _ | _⟩
    cases hes with | @map es _ hes =>
    refine VT.list ?_
    intro x hx; simp at hx; obtain ⟨_, _, _, rfl⟩ := hx; exact VT.str
  next =>
    subst h; rcases hvs with _ | ⟨hes, _ | _⟩
    cases hes with | @map es _ hes =>
    refine VT.list ?_
    intro x hx; simp at hx; obtain ⟨k, hk⟩ := hx; exact hes _ hk
  next =>
    subst h; rcases hvs with _ | ⟨hes, _ | _⟩
    cases hes with | @map es _ hes =>
    refine VT.list ?_
    intro x hx; simp at hx; obtain ⟨k, v, hk, rfl⟩ := hx
    exact VT.tuple (.cons VT.str (.cons (hes _ hk) .nil))
  next =>
    subst h; rcases hvs with _ | ⟨hxs, _ | _⟩
    cases hxs with | @list xs _ hxs =>
    obtain ⟨ps, hps, hp⟩ := pairsOf_typed xs hxs
    show SafeE _ (do let ps ← pairsOf xs; let m := Val.map (makeMap ps); checkSize limit m; pure m)
    rw [hps]; exact sized (VT.map (makeMap_typed hp))

end Sx
