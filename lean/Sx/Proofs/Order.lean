import Sx.Proofs.Effect

namespace Sx

theorem core_application_no_request (ctx : Ctx) (g : Glob) (f : Fiber) (stack : List Frame) (name : String)
    (args : List Expr) (env : Env) (done : List Res) :
    OkP (fun (r : Fiber × List Fiber × Glob) => (∃ v, r.1.ctrl = .ret (.val v)) ∧ r.2.1 = [] ∧ r.2.2 = g)
      (finishSeq ctx g f stack (.core name args) env done) := by
  unfold finishSeq; dsimp only
  okp_auto
  all_goals first | trivial | exact ⟨⟨_, rfl⟩, rfl, rfl⟩

theorem pure_program_no_request {ctx : Ctx} {m : Machine} (hpure : callsOf ctx.prog.body = []) (h : Reachable ctx m) :
    ∀ id cap input, Event.request id cap input ∉ m.trace := by
  intro id cap input hev
  have := effect_soundness_calls h id cap input hev
  rw [hpure] at this
  cases this

theorem pure_program_never_waits_on_a_call {ctx : Ctx} {m : Machine} (hpure : callsOf ctx.prog.body = [])
    (h : Reachable ctx m) : ∀ f ∈ m.fibers, ∀ p s cap input out b, f.ctrl ≠ .wait (.call p s cap input out) b := by
  intro f hf p s cap input out b hc
  have := ((reachable_inv h).fibers f hf).1
  rw [hc, hpure] at this
  cases this

theorem fieldLookup_perm {α : Type} {l l' : List (Nat × α)} (hp : l.Perm l') (hn : (l.map Prod.fst).Nodup) (k : Nat) :
    fieldLookup l k = fieldLookup l' k := by
  induction hp with
  | nil => rfl
  | cons x _ ih =>
    obtain ⟨a, v⟩ := x
    simp only [List.map_cons, List.nodup_cons] at hn
    by_cases h : a = k
    · subst h; simp [fieldLookup_eq]
    · rw [fieldLookup_ne _ _ h, fieldLookup_ne _ _ h]; exact ih hn.2
  | swap x y l =>
    obtain ⟨a, v⟩ := x
    obtain ⟨b, w⟩ := y
    simp only [List.map_cons, List.nodup_cons, List.mem_cons, not_or] at hn
    have hab : b ≠ a := hn.1.1
    by_cases hb : b = k
    · subst hb
      rw [fieldLookup_eq, fieldLookup_ne _ _ (Ne.symm hab), fieldLookup_eq]
    · by_cases ha : a = k
      · subst ha
        rw [fieldLookup_ne _ _ hb, fieldLookup_eq, fieldLookup_eq]
      · rw [fieldLookup_ne _ _ hb, fieldLookup_ne _ _ ha, fieldLookup_ne _ _ ha, fieldLookup_ne _ _ hb]
  | trans h1 _ ih1 ih2 =>
    rw [ih1 hn, ih2 ((h1.map Prod.fst).nodup_iff.mp hn)]

theorem par_order_independent {count : Nat} {rs rs' : List (Nat × Val)} (hp : rs.Perm rs')
    (hn : (rs.map Prod.fst).Nodup) : assemble count rs = assemble count rs' := by
  unfold assemble
  congr 1
  funext i
  rw [fieldLookup_perm hp hn i]

theorem mapM_ok {α β : Type} {g : α → Except Failure β} :
    ∀ (is : List α) (vs : List β), is.mapM g = .ok vs →
      ∃ h : vs.length = is.length, ∀ j (hj : j < vs.length), g (is[j]'(h ▸ hj)) = .ok vs[j]
  | [], vs, hm => by
    simp at hm; subst hm; exact ⟨rfl, fun j hj => by simp at hj⟩
  | i :: is, vs, hm => by
    simp only [List.mapM_cons] at hm
    cases hg : g i with
    | error e => rw [hg] at hm; cases hm
    | ok b =>
      rw [hg] at hm
      simp only [except_ok_bind] at hm
      cases hr : is.mapM g with
      | error e => rw [hr] at hm; cases hm
      | ok bs =>
        rw [hr] at hm
        simp at hm; subst hm
        obtain ⟨hl, hall⟩ := mapM_ok is bs hr
        refine ⟨by simp [hl], ?_⟩
        intro j hj
        cases j with
        | zero => simpa using hg
        | succ j => simpa using hall j (by simpa using hj)

theorem par_results_by_index {count : Nat} {rs : List (Nat × Val)} {vs : List Val}
    (h : assemble count rs = .ok vs) : vs.length = count ∧ ∀ i (hi : i < vs.length), fieldLookup rs i = some vs[i] := by
  unfold assemble at h
  obtain ⟨hl, hall⟩ := mapM_ok _ _ h
  refine ⟨by simpa using hl, ?_⟩
  intro i hi
  have := hall i hi
  simp only [List.getElem_range] at this
  split at this
  · rename_i v hv; cases this; exact hv
  · cases this

end Sx
