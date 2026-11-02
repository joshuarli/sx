import Sx.Proofs.Match
import Sx.Proofs.Conv
import Sx.Proofs.Each

namespace Sx

def CloT (S : Sig) (c : Clo) (tys : List Ty) (res : Ty) : Prop :=
  ∃ Γb Γ', ET S.enums c.env Γb ∧ bindAllTy Γb c.params tys = some Γ' ∧ HasType S Γ' none c.body res ∧
    isPure c.body = true

def CoresOk (S : Sig) (cores : Cores) : Prop :=
  ∀ f sig, fieldLookup S.funs f = some sig → ∃ impl, cores f = some impl ∧
    ∀ vs, All2 (VT S.enums) vs sig.params → SafeE (fun v => VT S.enums v sig.output) (impl vs)

def SafeAt (S : Sig) (P : PureCtx) (n : Nat) : Prop :=
  (∀ ρ Γ e T, isPure e = true → HasType S Γ none e T → ET S.enums ρ Γ →
    SafeE (fun v => VT S.enums v T) (evalP P n ρ e)) ∧
  (∀ ρ Γ es ts, isPureList es = true → HasTypes S Γ es ts → ET S.enums ρ Γ →
    SafeE (fun vs => All2 (VT S.enums) vs ts) (evalList P n ρ es)) ∧
  (∀ ρ Γ es t, isPureList es = true → AllT S Γ es t → ET S.enums ρ Γ →
    SafeE (fun vs => ∀ v ∈ vs, VT S.enums v t) (evalList P n ρ es)) ∧
  (∀ ρ Γ fs tfs, isPureFields fs = true → Fields S Γ fs tfs → ET S.enums ρ Γ →
    SafeE (fun vs => All2 (VT S.enums) vs (tfs.map Prod.snd)) (evalFields P n ρ fs)) ∧
  (∀ c tys res args, CloT S c tys res → All2 (VT S.enums) args tys →
    SafeE (fun v => VT S.enums v res) (applyClo P n c args)) ∧
  (∀ c a b items, CloT S c [a] b → (∀ x ∈ items, VT S.enums x a) →
    SafeE (fun rs => ∀ r ∈ rs, VT S.enums r b) (mapClo P n c items)) ∧
  (∀ c a stop items, CloT S c [a] .bool → (∀ x ∈ items, VT S.enums x a) →
    SafeE (fun v => VT S.enums v .bool) (anyClo P n c stop items)) ∧
  (∀ c acc a init items, CloT S c [acc, a] acc → VT S.enums init acc → (∀ x ∈ items, VT S.enums x a) →
    SafeE (fun v => VT S.enums v acc) (foldClo P n c init items)) ∧
  (∀ ρ Γ op args T, op.higherOrder = true → isPureList args = true → HasType S Γ none (.prelude op args) T →
    ET S.enums ρ Γ → SafeE (fun v => VT S.enums v T) (evalHO P n ρ op args))

theorem fuel_safe {α : Type} {Q : α → Prop} : SafeE Q (outOfFuel : Except Failure α) := SafeE.fail (by decide)

theorem safeAt_zero (S : Sig) (P : PureCtx) : SafeAt S P 0 := by
  refine ⟨?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_⟩ <;> intros <;> exact fuel_safe

theorem Fields.names {S : Sig} {Γ : TyEnv} :
    ∀ {fs : List (String × Expr)} {tfs : List (String × Ty)}, Fields S Γ fs tfs → fs.map Prod.fst = tfs.map Prod.fst
  | [], [], _ => rfl
  | _ :: _, _ :: _, .cons _ h => by simp [Fields.names h]

theorem closureOf_typed {S : Sig} {Γ : TyEnv} {ρ : Env} {f : Expr} {tys : List Ty} {res : Ty}
    (hl : LamT S Γ f tys res) (hp : isPure f = true) (he : ET S.enums ρ Γ) :
    ∃ c, closureOf ρ f = .ok c ∧ CloT S c tys res := by
  cases hl with
  | mk hb hbind hbody =>
    rename_i cap _ _ _ _
    simp only [isPure] at hp
    cases cap with
    | none =>
      simp [lamBase] at hb; subst hb
      exact ⟨_, rfl, _, _, he, hbind, hbody, hp⟩
    | some n =>
      simp only [lamBase] at hb
      split at hb
      · rename_i Γd hΓd
        cases hb
        obtain ⟨ρd, hρd, hed⟩ := ET.lookup_fn he hΓd
        refine ⟨{ params := _, body := _, env := ρd }, ?_, _, _, hed, hbind, hbody, hp⟩
        simp [closureOf, Env.lookup, hρd]
      · cases hb

theorem step_lists {S : Sig} {P : PureCtx} {n : Nat} (ih : SafeAt S P n) :
    (∀ ρ Γ es ts, isPureList es = true → HasTypes S Γ es ts → ET S.enums ρ Γ →
      SafeE (fun vs => All2 (VT S.enums) vs ts) (evalList P (n + 1) ρ es)) ∧
    (∀ ρ Γ es t, isPureList es = true → AllT S Γ es t → ET S.enums ρ Γ →
      SafeE (fun vs => ∀ v ∈ vs, VT S.enums v t) (evalList P (n + 1) ρ es)) ∧
    (∀ ρ Γ fs tfs, isPureFields fs = true → Fields S Γ fs tfs → ET S.enums ρ Γ →
      SafeE (fun vs => All2 (VT S.enums) vs (tfs.map Prod.snd)) (evalFields P (n + 1) ρ fs)) := by
  refine ⟨?_, ?_, ?_⟩
  · intro ρ Γ es ts hp h he
    cases h with
    | nil => exact All2.nil
    | cons he1 hes =>
      simp only [isPureList, Bool.and_eq_true] at hp
      exact SafeE.bind (ih.1 _ _ _ _ hp.1 he1 he) fun v hv =>
        SafeE.bind (ih.2.1 _ _ _ _ hp.2 hes he) fun vs hvs => All2.cons hv hvs
  · intro ρ Γ es t hp h he
    cases h with
    | nil => simp [evalList, SafeE]
    | cons he1 hes =>
      simp only [isPureList, Bool.and_eq_true] at hp
      exact SafeE.bind (ih.1 _ _ _ _ hp.1 he1 he) fun v hv =>
        SafeE.bind (ih.2.2.1 _ _ _ _ hp.2 hes he) fun vs hvs => by
          intro x hx; simp at hx; rcases hx with rfl | hx; exact hv; exact hvs x hx
  · intro ρ Γ fs tfs hp h he
    cases h with
    | nil => exact All2.nil
    | cons he1 hfs =>
      simp only [isPureFields, Bool.and_eq_true] at hp
      exact SafeE.bind (ih.1 _ _ _ _ hp.1 he1 he) fun v hv =>
        SafeE.bind (ih.2.2.2.1 _ _ _ _ hp.2 hfs he) fun vs hvs => All2.cons hv hvs

theorem step_closures {S : Sig} {P : PureCtx} {n : Nat} (ih : SafeAt S P n) :
    (∀ c tys res args, CloT S c tys res → All2 (VT S.enums) args tys →
      SafeE (fun v => VT S.enums v res) (applyClo P (n + 1) c args)) ∧
    (∀ c a b items, CloT S c [a] b → (∀ x ∈ items, VT S.enums x a) →
      SafeE (fun rs => ∀ r ∈ rs, VT S.enums r b) (mapClo P (n + 1) c items)) ∧
    (∀ c a stop items, CloT S c [a] .bool → (∀ x ∈ items, VT S.enums x a) →
      SafeE (fun v => VT S.enums v .bool) (anyClo P (n + 1) c stop items)) ∧
    (∀ c acc a init items, CloT S c [acc, a] acc → VT S.enums init acc → (∀ x ∈ items, VT S.enums x a) →
      SafeE (fun v => VT S.enums v acc) (foldClo P (n + 1) c init items)) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro c tys res args hc ha
    obtain ⟨Γb, Γ', heb, hbind, hbody, hpure⟩ := hc
    obtain ⟨ρ', hρ', he'⟩ := bindAll_typed _ _ _ hbind ha heb
    simp only [applyClo, hρ', except_ok_bind]
    exact ih.1 _ _ _ _ hpure hbody he'
  · intro c a b items hc hi
    cases items with
    | nil => simp [mapClo, SafeE]
    | cons x xs =>
      exact SafeE.bind (ih.2.2.2.2.1 c [a] b [x] hc (.cons (hi x (by simp)) .nil)) fun r hr =>
        SafeE.bind (ih.2.2.2.2.2.1 c a b xs hc (fun y hy => hi y (by simp [hy]))) fun rs hrs => by
          intro z hz; simp at hz; rcases hz with rfl | hz; exact hr; exact hrs z hz
  · intro c a stop items hc hi
    cases items with
    | nil => exact VT.bool
    | cons x xs =>
      refine SafeE.bind (ih.2.2.2.2.1 c [a] .bool [x] hc (.cons (hi x (by simp)) .nil)) fun r _ => ?_
      split
      · exact VT.bool
      · exact ih.2.2.2.2.2.2.1 c a stop xs hc (fun y hy => hi y (by simp [hy]))
  · intro c acc a init items hc hinit hi
    cases items with
    | nil => exact hinit
    | cons x xs =>
      exact SafeE.bind (ih.2.2.2.2.1 c [acc, a] acc [init, x] hc (.cons hinit (.cons (hi x (by simp)) .nil)))
        fun r hr => ih.2.2.2.2.2.2.2.1 c acc a r xs hc hr (fun y hy => hi y (by simp [hy]))

end Sx

namespace Sx

theorem step_ho {S : Sig} {P : PureCtx} {n : Nat} (ih : SafeAt S P n) :
    ∀ ρ Γ op args T, op.higherOrder = true → isPureList args = true → HasType S Γ none (.prelude op args) T →
      ET S.enums ρ Γ → SafeE (fun v => VT S.enums v T) (evalHO P (n + 1) ρ op args) := by
  intro ρ Γ op args T hho hp h he
  cases h with
  | prelude _ _ hfo => rw [hfo] at hho; cases hho
  | map hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his =>
      refine SafeE.bind (ih.2.2.2.2.2.1 _ _ _ _ hct his) fun rs hrs => ?_
      exact VT.list hrs
  | filter hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his =>
      refine SafeE.bind (ih.2.2.2.2.2.1 _ _ _ _ hct his) fun rs _ => ?_
      exact VT.list (fun x hx => his x (filterKept_mem is rs x hx))
  | keep hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his =>
      refine SafeE.bind (ih.2.2.2.2.2.1 _ _ _ _ hct his) fun rs hrs => ?_
      exact VT.list (keepSome_typed rs hrs)
  | any hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his => exact ih.2.2.2.2.2.2.1 _ _ _ _ hct his
  | all hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his => exact ih.2.2.2.2.2.2.1 _ _ _ _ hct his
  | reduce hf hinit hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hinit he) fun a ha => ?_
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his => exact ih.2.2.2.2.2.2.2.1 _ _ _ _ _ hct ha his
  | sortBy hf hk hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his =>
      refine SafeE.bind (ih.2.2.2.2.2.1 _ _ _ _ hct his) fun rs hrs => ?_
      obtain ⟨out, ho, hom⟩ := sortStable_ok (E := S.enums) (key := fun (p : Val × Val) => p.2) hk (is.zip rs)
        (fun p hp => hrs _ (List.of_mem_zip hp).2)
      simp only [finishEach, ho, except_ok_bind]
      refine VT.list ?_
      intro x hx
      simp at hx
      obtain ⟨y, hy⟩ := hx
      exact his _ (List.of_mem_zip (hom _ hy)).1
  | distinctBy hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his =>
      refine SafeE.bind (ih.2.2.2.2.2.1 _ _ _ _ hct his) fun rs _ => ?_
      refine VT.list ?_
      intro x hx
      obtain ⟨p, hp, rfl⟩ := distinctGo_mem [] _ x hx
      exact his _ (List.of_mem_zip hp).1
  | groupBy hf hxs =>
    simp only [isPureList, Bool.and_eq_true] at hp
    obtain ⟨c, hc, hct⟩ := closureOf_typed hf hp.1 he
    simp only [evalHO, hc, except_ok_bind]
    refine SafeE.bind (ih.1 _ _ _ _ hp.2.1 hxs he) fun items hitems => ?_
    cases hitems with
    | @list is _ his =>
      refine SafeE.bind (ih.2.2.2.2.2.1 _ _ _ _ hct his) fun rs hrs => ?_
      exact groupBy_typed his hrs

theorem step_eval {S : Sig} {P : PureCtx} {n : Nat} (hcores : CoresOk S P.cores) (ih : SafeAt S P n) :
    ∀ ρ Γ e T, isPure e = true → HasType S Γ none e T → ET S.enums ρ Γ →
      SafeE (fun v => VT S.enums v T) (evalP P (n + 1) ρ e) := by
  intro ρ Γ e T hp h he
  cases h with
  | lit hv => exact hv
  | var hl =>
    obtain ⟨v, hv, hvt⟩ := ET.lookup_val he hl
    simp only [evalP, Env.lookup, hv]
    exact hvt
  | let_ hv hb hbody =>
    simp only [isPure, Bool.and_eq_true] at hp
    simp only [evalP]
    refine SafeE.bind (ih.1 _ _ _ _ hp.1 hv he) fun x hx => ?_
    obtain ⟨ρ', hρ', he'⟩ := bind_typed _ _ _ hb hx he
    simp only [hρ', except_ok_bind]
    exact ih.1 _ _ _ _ hp.2 hbody he'
  | localFn hbody =>
    simp only [isPure] at hp
    exact ih.1 _ _ _ _ hp hbody (ET.fn he he)
  | if_ hc ht hel =>
    simp only [isPure, Bool.and_eq_true] at hp
    simp only [evalP]
    refine SafeE.bind (ih.1 _ _ _ _ hp.1.1 hc he) fun b _ => ?_
    split
    · exact ih.1 _ _ _ _ hp.1.2 ht he
    · exact ih.1 _ _ _ _ hp.2 hel he
  | match_ hs hcs hx =>
    simp only [isPure, Bool.and_eq_true] at hp
    simp only [evalP]
    refine SafeE.bind (ih.1 _ _ _ _ hp.1 hs he) fun v hv => ?_
    obtain ⟨p, hpm, hm⟩ := exhaustive_witness hx hv he (Clauses.typed hcs)
    obtain ⟨body, ρ', Γ', hsel, he', hbody⟩ := selectClause_typed hv he hcs ⟨p, hpm, hm⟩
    simp only [hsel, except_ok_bind]
    have hpb : isPure body = true := by
      have : ∀ (cs : List (Pat × Expr)) (ρ : Env) (body : Expr) (ρ' : Env), isPureClauses cs = true →
          selectClause ρ v cs = .ok (body, ρ') → isPure body = true := by
        intro cs
        induction cs with
        | nil => intro ρ body ρ' _ h; simp [selectClause, internal] at h
        | cons c cs ihc =>
          intro ρ body ρ' hpc h
          obtain ⟨q, b⟩ := c
          simp only [isPureClauses, Bool.and_eq_true] at hpc
          simp only [selectClause] at h
          cases hq : matchPat ρ q v with
          | error e => rw [hq] at h; cases h
          | ok r =>
            rw [hq] at h
            cases r with
            | none => exact ihc ρ body ρ' hpc.2 h
            | some ρm => simp at h; rw [← h.1]; exact hpc.1
      exact this _ ρ body ρ' hp.2 hsel
    exact ih.1 _ _ _ _ hpb hbody he'
  | applyLocal hl hbind hbody hargs =>
    simp only [isPure, Bool.and_eq_true] at hp
    simp only [evalP]
    refine SafeE.bind (ih.2.1 _ _ _ _ hp.2 hargs he) fun vs hvs => ?_
    obtain ⟨ρd, hρd, hed⟩ := ET.lookup_fn he hl
    simp only [Env.lookup, hρd]
    obtain ⟨ρ', hρ', he'⟩ := bindAll_typed _ _ _ hbind hvs hed
    simp only [hρ', except_ok_bind]
    exact ih.1 _ _ _ _ hp.1 hbody he'
  | prelude hargs hop hfo =>
    simp only [isPure] at hp
    simp only [evalP, hfo]
    exact SafeE.bind (ih.2.1 _ _ _ _ hp hargs he) fun vs hvs => applyOp_typed _ _ _ _ hop hvs
  | tupleItem hxs hti =>
    simp only [isPure] at hp
    simp only [evalP]
    refine SafeE.bind (ih.1 _ _ _ _ hp hxs he) fun v hv => ?_
    cases hv with
    | @tuple items _ hitems =>
      have hlen := All2.length_eq hitems
      obtain ⟨hlt, heq⟩ := List.getElem?_eq_some_iff.mp hti
      have hlt' : _ < items.length := hlen ▸ hlt
      simp only [List.getElem?_eq_getElem hlt']
      subst heq
      exact All2.get hitems _ hlt' hlt
  | map hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.map hf hxs) he
  | filter hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.filter hf hxs) he
  | keep hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.keep hf hxs) he
  | any hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.any hf hxs) he
  | all hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.all hf hxs) he
  | reduce hf hinit hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.reduce hf hinit hxs) he
  | sortBy hf hk hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.sortBy hf hk hxs) he
  | distinctBy hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.distinctBy hf hxs) he
  | groupBy hf hxs =>
    simp only [isPure] at hp
    exact ih.2.2.2.2.2.2.2.2 _ _ _ _ _ rfl hp (.groupBy hf hxs) he
  | core hsig hargs =>
    rename_i fname sig args
    simp only [isPure] at hp
    obtain ⟨impl, himpl, hsafe⟩ := hcores _ _ hsig
    simp only [evalP, himpl]
    exact SafeE.bind (ih.2.1 _ _ _ _ hp hargs he) fun vs hvs =>
      SafeE.bind (hsafe vs hvs) fun v hv => sized hv
  | call _ _ => simp [isPure] at hp
  | get hr hf =>
    simp only [isPure] at hp
    simp only [evalP]
    refine SafeE.bind (ih.1 _ _ _ _ hp hr he) fun v hv => ?_
    cases hv with
    | record hn ha =>
      obtain ⟨w, hw, hwt⟩ := record_lookup _ _ _ _ hn ha hf
      simp only [hw]
      exact hwt
  | @mkRecord _ fs tfs _ hfs =>
    simp only [isPure] at hp
    simp only [evalP]
    refine SafeE.bind (ih.2.2.2.1 _ _ _ _ hp hfs he) fun vs hvs => ?_
    have hn := Fields.names hfs
    have hl1 : vs.length = _ := All2.length_eq hvs
    have hl2 : fs.length = tfs.length := by
      have := congrArg List.length hn; simpa using this
    have hal : Aligned (VT S.enums) ((fs.map Prod.fst).zip vs) tfs := by
      refine ⟨?_, ?_⟩
      · rw [List.map_fst_zip (by simp [hl1, hl2])]; exact hn
      · rw [List.map_snd_zip (by simp [hl1, hl2])]; exact hvs
    have hs := sortFields_aligned _ _ hal
    exact sized (VT.record hs.1 hs.2)
  | mkList hes =>
    simp only [isPure] at hp
    simp only [evalP]
    exact SafeE.bind (ih.2.2.1 _ _ _ _ hp hes he) fun vs hvs => sized (VT.list hvs)
  | mkTuple hes =>
    simp only [isPure] at hp
    simp only [evalP]
    exact SafeE.bind (ih.2.1 _ _ _ _ hp hes he) fun vs hvs => VT.tuple hvs
  | wrapSome hv =>
    simp only [isPure] at hp
    simp only [evalP]
    exact SafeE.bind (ih.1 _ _ _ _ hp hv he) fun v hv => VT.some hv
  | wrapOk hv =>
    simp only [isPure] at hp
    simp only [evalP]
    exact SafeE.bind (ih.1 _ _ _ _ hp hv he) fun v hv => VT.ok hv
  | wrapErr hv =>
    simp only [isPure] at hp
    simp only [evalP]
    exact SafeE.bind (ih.1 _ _ _ _ hp hv he) fun v hv => VT.err hv
  | mkVariant hn => exact VT.variant hn
  | mkVariantPayload hn hpv =>
    simp only [isPure] at hp
    simp only [evalP]
    exact SafeE.bind (ih.1 _ _ _ _ hp hpv he) fun v hv => VT.variantPayload hn hv
  | coerce hv hc =>
    simp only [isPure] at hp
    simp only [evalP]
    refine SafeE.bind (ih.1 _ _ _ _ hp hv he) fun v hv => ?_
    obtain ⟨w, hw, hwt⟩ := convert_typed _ _ _ _ hc hv
    rw [hw]; exact hwt
  | attempt _ => simp [isPure] at hp
  | failStr _ => simp [isPure] at hp
  | failErr _ => simp [isPure] at hp
  | log _ => simp [isPure] at hp
  | retry _ => simp [isPure] at hp
  | waitUntil _ _ => simp [isPure] at hp
  | sleep => simp [isPure] at hp
  | loop _ _ _ => simp [isPure] at hp
  | par _ => simp [isPure] at hp
  | parMap _ _ => simp [isPure] at hp

theorem safeAt_succ {S : Sig} {P : PureCtx} {n : Nat} (hcores : CoresOk S P.cores) (ih : SafeAt S P n) :
    SafeAt S P (n + 1) :=
  ⟨step_eval hcores ih, (step_lists ih).1, (step_lists ih).2.1, (step_lists ih).2.2, (step_closures ih).1,
    (step_closures ih).2.1, (step_closures ih).2.2.1, (step_closures ih).2.2.2, step_ho ih⟩

theorem safeAt (S : Sig) (P : PureCtx) (hcores : CoresOk S P.cores) : ∀ n, SafeAt S P n
  | 0 => safeAt_zero S P
  | n + 1 => safeAt_succ hcores (safeAt S P hcores n)

theorem pure_type_safety (S : Sig) (P : PureCtx) (hcores : CoresOk S P.cores) (fuel : Nat) (ρ : Env) (Γ : TyEnv)
    (e : Expr) (T : Ty) (hpure : isPure e = true) (htyped : HasType S Γ none e T) (henv : ET S.enums ρ Γ) :
    match evalP P fuel ρ e with
    | .ok v => VT S.enums v T
    | .error f => f.kind ≠ .internal := by
  have h := (safeAt S P hcores fuel).1 ρ Γ e T hpure htyped henv
  generalize evalP P fuel ρ e = r at h ⊢
  cases r <;> exact h

end Sx
