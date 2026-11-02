import Sx.Proofs.Ops

namespace Sx

theorem cmpKeyLists_of {E : Enums} {t : Ty}
    (h : ∀ a b, VT E a t → VT E b t → ∃ o, cmpKeys a b = .ok o) :
    ∀ (as bs : List Val), (∀ x ∈ as, VT E x t) → (∀ x ∈ bs, VT E x t) → ∃ o, cmpKeyLists as bs = .ok o
  | [], [], _, _ => ⟨.eq, by simp [cmpKeyLists]⟩
  | [], _ :: _, _, _ => ⟨.lt, by simp [cmpKeyLists]⟩
  | _ :: _, [], _, _ => ⟨.gt, by simp [cmpKeyLists]⟩
  | a :: as, b :: bs, ha, hb => by
    obtain ⟨o, ho⟩ := h a b (ha a (by simp)) (hb b (by simp))
    obtain ⟨o', ho'⟩ := cmpKeyLists_of h as bs (fun x hx => ha x (by simp [hx])) (fun x hx => hb x (by simp [hx]))
    simp only [cmpKeyLists, ho, except_ok_bind]
    split
    · exact ⟨o', ho'⟩
    · exact ⟨o, rfl⟩

mutual
theorem cmpKeys_sortKey {E : Enums} :
    ∀ (k : Ty) (a b : Val), sortKey k = true → VT E a k → VT E b k → ∃ o, cmpKeys a b = .ok o
  | .int, a, b, _, ha, hb => by cases ha; cases hb; exact ⟨_, rfl⟩
  | .str, a, b, _, ha, hb => by cases ha; cases hb; exact ⟨_, rfl⟩
  | .float, a, b, _, ha, hb => by cases ha; cases hb; exact ⟨_, rfl⟩
  | .bool, a, b, _, ha, hb => by cases ha; cases hb; exact ⟨_, rfl⟩
  | .var _, a, b, _, ha, _ => by cases ha
  | .never, a, b, _, ha, _ => by cases ha
  | .list t, a, b, hk, ha, hb => by
    simp only [sortKey] at hk
    cases ha with
    | @list as _ has =>
      cases hb with
      | @list bs _ hbs =>
        obtain ⟨o, ho⟩ := cmpKeyLists_of (fun x y hx hy => cmpKeys_sortKey t x y hk hx hy) as bs has hbs
        exact ⟨o, by simp only [cmpKeys]; exact ho⟩
  | .tuple ts, a, b, hk, ha, hb => by
    simp only [sortKey] at hk
    cases ha with
    | @tuple as _ has =>
      cases hb with
      | @tuple bs _ hbs =>
        obtain ⟨o, ho⟩ := cmpKeyLists_tuple ts as bs hk has hbs
        exact ⟨o, by simp only [cmpKeys]; exact ho⟩
  | .nominal _ base, a, b, hk, ha, hb => by
    simp only [sortKey, Bool.and_eq_true] at hk
    cases ha with
    | nominal ha =>
      cases hb with
      | nominal hb => exact cmpKeys_sortKey base a b hk.2 ha hb
  | .unit, _, _, hk, _, _ => by simp [sortKey] at hk
  | .bytes, _, _, hk, _, _ => by simp [sortKey] at hk
  | .map _, _, _, hk, _, _ => by simp [sortKey] at hk
  | .option _, _, _, hk, _, _ => by simp [sortKey] at hk
  | .result _ _, _, _, hk, _, _ => by simp [sortKey] at hk
  | .record _, _, _, hk, _, _ => by simp [sortKey] at hk
  | .enum _, _, _, hk, _, _ => by simp [sortKey] at hk
theorem cmpKeyLists_tuple {E : Enums} :
    ∀ (ts : List Ty) (as bs : List Val), sortKeys ts = true → All2 (VT E) as ts → All2 (VT E) bs ts →
      ∃ o, cmpKeyLists as bs = .ok o
  | [], [], [], _, _, _ => ⟨.eq, by simp [cmpKeyLists]⟩
  | t :: ts, a :: as, b :: bs, hk, ha, hb => by
    simp only [sortKeys, Bool.and_eq_true] at hk
    cases ha with
    | cons ha has =>
      cases hb with
      | cons hb hbs =>
        obtain ⟨o, ho⟩ := cmpKeys_sortKey t a b hk.1 ha hb
        obtain ⟨o', ho'⟩ := cmpKeyLists_tuple ts as bs hk.2 has hbs
        simp only [cmpKeyLists, ho, except_ok_bind]
        split
        · exact ⟨o', ho'⟩
        · exact ⟨o, rfl⟩
  | [], _ :: _, _, _, ha, _ => by cases ha
  | [], [], _ :: _, _, _, hb => by cases hb
  | _ :: _, [], _, _, ha, _ => by cases ha
  | _ :: _, _ :: _, [], _, _, hb => by cases hb
end

theorem insertSortedFirst_ok {α : Type} {E : Enums} {k : Ty} {key : α → Val} (hk : sortKey k = true) (x : α)
    (hx : VT E (key x) k) :
    ∀ (ys : List α), (∀ y ∈ ys, VT E (key y) k) →
      ∃ out, sortStable.insertSortedFirst key x ys = .ok out ∧ ∀ z ∈ out, z = x ∨ z ∈ ys
  | [], _ => ⟨[x], by simp [sortStable.insertSortedFirst], by simp⟩
  | y :: ys, hys => by
    obtain ⟨o, ho⟩ := cmpKeys_sortKey k (key x) (key y) hk hx (hys y (by simp))
    simp only [sortStable.insertSortedFirst, ho, except_ok_bind]
    split
    · obtain ⟨out, hout, hmem⟩ := insertSortedFirst_ok hk x hx ys (fun z hz => hys z (by simp [hz]))
      refine ⟨y :: out, by simp [hout], ?_⟩
      intro z hz; simp at hz
      rcases hz with rfl | hz
      · exact .inr (by simp)
      · rcases hmem z hz with h | h; exact .inl h; exact .inr (by simp [h])
    · refine ⟨x :: y :: ys, rfl, ?_⟩
      intro z hz; simp at hz
      rcases hz with rfl | rfl | hz
      · exact .inl rfl
      · exact .inr (by simp)
      · exact .inr (by simp [hz])

theorem sortStable_ok {α : Type} {E : Enums} {k : Ty} {key : α → Val} (hk : sortKey k = true) :
    ∀ (xs : List α), (∀ x ∈ xs, VT E (key x) k) → ∃ out, sortStable key xs = .ok out ∧ ∀ z ∈ out, z ∈ xs
  | [], _ => ⟨[], rfl, by simp⟩
  | x :: xs, hxs => by
    obtain ⟨sorted, hs, hsm⟩ := sortStable_ok hk xs (fun z hz => hxs z (by simp [hz]))
    obtain ⟨out, ho, hom⟩ := insertSortedFirst_ok hk x (hxs x (by simp)) sorted
      (fun z hz => hxs z (by simp [hsm z hz]))
    refine ⟨out, by simp [sortStable, hs, ho], ?_⟩
    intro z hz
    rcases hom z hz with rfl | h
    · simp
    · simp [hsm z h]

theorem filterKept_mem : ∀ (items rs : List Val) (x : Val), x ∈ filterKept items rs → x ∈ items
  | [], _, x, h => by simp [filterKept] at h
  | _ :: _, [], x, h => by simp [filterKept] at h
  | i :: items, r :: rs, x, h => by
    simp only [filterKept] at h
    split at h
    · simp at h; rcases h with rfl | h; simp; simp [filterKept_mem items rs x h]
    · simp [filterKept_mem items rs x h]

theorem keepSome_typed {E : Enums} {b : Ty} :
    ∀ (rs : List Val), (∀ r ∈ rs, VT E r (.option b)) → ∀ x ∈ keepSome rs, VT E x b
  | [], _, x, h => by simp [keepSome] at h
  | r :: rs, hrs, x, h => by
    have hr := hrs r (by simp)
    cases hr with
    | none => exact keepSome_typed rs (fun y hy => hrs y (by simp [hy])) x (by simpa [keepSome] using h)
    | some hv =>
      simp [keepSome] at h
      rcases h with rfl | h
      · exact hv
      · exact keepSome_typed rs (fun y hy => hrs y (by simp [hy])) x h

theorem distinctGo_mem : ∀ (seen : List Val) (ps : List (Val × Val)) (x : Val),
    x ∈ distinctBy.go seen ps → ∃ p ∈ ps, x = p.1
  | _, [], x, h => by simp [distinctBy.go] at h
  | seen, (item, key) :: ps, x, h => by
    simp only [distinctBy.go] at h
    split at h
    · obtain ⟨p, hp, rfl⟩ := distinctGo_mem seen ps x h; exact ⟨p, by simp [hp], rfl⟩
    · simp at h
      rcases h with rfl | h
      · exact ⟨(x, key), by simp, rfl⟩
      · obtain ⟨p, hp, rfl⟩ := distinctGo_mem _ ps x h; exact ⟨p, by simp [hp], rfl⟩

def GroupsOk (E : Enums) (k a : Ty) (gs : List (Val × List Val)) : Prop :=
  ∀ g ∈ gs, VT E g.1 k ∧ ∀ x ∈ g.2, VT E x a

theorem addToGroup_ok {E : Enums} {k a : Ty} {item key : Val} (hi : VT E item a) (hk : VT E key k) :
    ∀ (gs : List (Val × List Val)), GroupsOk E k a gs → GroupsOk E k a (addToGroup item key gs)
  | [], _ => by
    intro g hg; simp [addToGroup] at hg; subst hg; exact ⟨hk, by simp [hi]⟩
  | (k', items) :: rest, h => by
    simp only [addToGroup]
    split
    · intro g hg; simp at hg
      rcases hg with rfl | hg
      · have := h (k', items) (by simp)
        refine ⟨this.1, ?_⟩
        intro x hx; simp at hx; rcases hx with hx | rfl; exact this.2 x hx; exact hi
      · exact h g (by simp [hg])
    · intro g hg; simp at hg
      rcases hg with rfl | hg
      · exact h _ (by simp)
      · exact addToGroup_ok hi hk rest (fun g' hg' => h g' (by simp [hg'])) g hg

theorem groupFold_ok {E : Enums} {k a : Ty} :
    ∀ (ps : List (Val × Val)) (acc : List (Val × List Val)), (∀ p ∈ ps, VT E p.1 a ∧ VT E p.2 k) →
      GroupsOk E k a acc →
      GroupsOk E k a (ps.foldl (fun acc (p : Val × Val) => addToGroup p.1 p.2 acc) acc)
  | [], acc, _, h => h
  | p :: ps, acc, hps, h => by
    simp only [List.foldl]
    have hp := hps p (by simp)
    exact groupFold_ok ps _ (fun q hq => hps q (by simp [hq])) (addToGroup_ok hp.1 hp.2 acc h)

theorem groupBy_typed {E : Enums} {k a : Ty} {items results : List Val} (hi : ∀ x ∈ items, VT E x a)
    (hr : ∀ r ∈ results, VT E r k) : VT E (.list (groupBy (items.zip results))) (.list (.tuple [k, .list a])) := by
  refine VT.list ?_
  intro v hv
  simp only [groupBy, List.mem_map] at hv
  obtain ⟨g, hg, rfl⟩ := hv
  have hok := groupFold_ok (E := E) (k := k) (a := a) (items.zip results) []
    (fun p hp => ⟨hi _ (List.of_mem_zip hp).1, hr _ (List.of_mem_zip hp).2⟩) (by simp [GroupsOk]) g
  have hok := hok (by
    have : (fun acc (x : Val × Val) => match x with | (item, key) => addToGroup item key acc) =
        (fun acc (p : Val × Val) => addToGroup p.1 p.2 acc) := by funext acc x; cases x; rfl
    rw [← this]; exact hg)
  exact VT.tuple (.cons hok.1 (.cons (VT.list hok.2) .nil))

end Sx
