import Sx.Run
import Sx.Effects
import Sx.Proofs.Basic

namespace Sx

def OkP {α : Type} (P : α → Prop) : Except Failure α → Prop
  | .ok a => P a
  | .error _ => True

theorem OkP.bind {α β : Type} {P : α → Prop} {Q : β → Prop} {x : Except Failure α} {f : α → Except Failure β}
    (hx : OkP P x) (hf : ∀ a, P a → OkP Q (f a)) : OkP Q (x >>= f) := by
  cases x with
  | error e => trivial
  | ok a => exact hf a hx

theorem OkP.error {α : Type} {P : α → Prop} {e : Failure} : OkP P (Except.error e : Except Failure α) := trivial

theorem OkP.internal {α : Type} {P : α → Prop} {m : String} : OkP P (internal m : Except Failure α) := trivial

theorem OkP.failWith {α : Type} {P : α → Prop} {k : FailKind} {m : String} :
    OkP P (failWith k m : Except Failure α) := trivial

theorem OkP.any {α : Type} (x : Except Failure α) : OkP (fun _ => True) x := by
  cases x <;> trivial

theorem OkP.mono {α : Type} {P Q : α → Prop} {x : Except Failure α} (hx : OkP P x) (h : ∀ a, P a → Q a) :
    OkP Q x := by
  cases x with
  | error e => trivial
  | ok a => exact h a hx

abbrev Sub (C : List String) (e : Expr) : Prop := callsOf e ⊆ C

def CloOk (C : List String) (c : Clo) : Prop := callsOf c.body ⊆ C

def ResOk (C : List String) : Res → Prop
  | .clo c => CloOk C c
  | _ => True

def CtrlOk (C : List String) : Ctrl → Prop
  | .eval e _ => callsOf e ⊆ C
  | .ret r => ResOk C r
  | .wait (.call _ _ cap _ _) _ => cap ∈ C
  | _ => True

def FrameOk (C : List String) : Frame → Prop
  | .seq origin _ done rest => callsOf origin ⊆ C ∧ (∀ r ∈ done, ResOk C r) ∧ ∀ e ∈ rest, callsOf e ⊆ C
  | .let_ _ body _ => callsOf body ⊆ C
  | .if_ t e _ => callsOf t ⊆ C ∧ callsOf e ⊆ C
  | .match_ cs _ => callsOfClauses cs ⊆ C
  | .attempt => True
  | .retry _ _ body _ _ => callsOf body ⊆ C
  | .wait _ pred body _ _ => CloOk C pred ∧ callsOf body ⊆ C
  | .waitCheck _ pred body _ _ _ => CloOk C pred ∧ callsOf body ⊆ C
  | .loop _ _ body _ _ => callsOf body ⊆ C
  | .each _ c _ _ _ _ => CloOk C c
  | .then_ e _ => callsOf e ⊆ C
  | .join j => ∀ c, j.clo = some c → CloOk C c

def StackOk (C : List String) (s : List Frame) : Prop := ∀ fr ∈ s, FrameOk C fr

def FiberOk (C : List String) (f : Fiber) : Prop := CtrlOk C f.ctrl ∧ StackOk C f.stack

structure CountersOk (B : Budgets) (c : Counters) : Prop where
  steps : c.steps ≤ B.steps
  calls : c.calls ≤ B.calls
  applications : c.applications ≤ B.applications
  delay : c.delay ≤ B.delayMs

def StepOk (C : List String) (B : Budgets) (r : Fiber × List Fiber × Glob) : Prop :=
  FiberOk C r.1 ∧ (∀ c ∈ r.2.1, FiberOk C c) ∧ CountersOk B r.2.2.counters

theorem StackOk.cons {C : List String} {fr : Frame} {s : List Frame} (h1 : FrameOk C fr) (h2 : StackOk C s) :
    StackOk C (fr :: s) := by
  intro x hx; simp at hx; rcases hx with rfl | hx; exact h1; exact h2 x hx

theorem StackOk.tail {C : List String} {fr : Frame} {s : List Frame} (h : StackOk C (fr :: s)) : StackOk C s :=
  fun x hx => h x (by simp [hx])

theorem StackOk.head {C : List String} {fr : Frame} {s : List Frame} (h : StackOk C (fr :: s)) : FrameOk C fr :=
  h fr (by simp)

theorem tick_counters {ctx : Ctx} {g : Glob} (h : CountersOk ctx.budgets g.counters) :
    OkP (fun g' => CountersOk ctx.budgets g'.counters) (tick ctx g) := by
  unfold tick; dsimp only; split
  · trivial
  · exact ⟨by simp; omega, h.calls, h.applications, h.delay⟩

theorem countCall_counters {ctx : Ctx} {g : Glob} (h : CountersOk ctx.budgets g.counters) :
    OkP (fun g' => CountersOk ctx.budgets g'.counters) (countCall ctx g) := by
  unfold countCall; dsimp only; split
  · trivial
  · exact ⟨h.steps, by simp; omega, h.applications, h.delay⟩

theorem countApplication_counters {ctx : Ctx} {g : Glob} (h : CountersOk ctx.budgets g.counters) :
    OkP (fun g' => CountersOk ctx.budgets g'.counters) (countApplication ctx g) := by
  unfold countApplication; dsimp only; split
  · trivial
  · exact ⟨h.steps, h.calls, by simp; omega, h.delay⟩

theorem countDelay_counters {ctx : Ctx} {g : Glob} {ms : Nat} (h : CountersOk ctx.budgets g.counters) :
    OkP (fun g' => CountersOk ctx.budgets g'.counters) (countDelay ctx g ms) := by
  unfold countDelay; dsimp only; split
  · trivial
  · exact ⟨h.steps, h.calls, h.applications, by simp; omega⟩

theorem nextSeq_counters {g : Glob} {path : List Nat} : (nextSeq g path).1.counters = g.counters := rfl

theorem apply_ok {C : List String} {ctx : Ctx} {g : Glob} {c : Clo} {args : List Val} (hc : CloOk C c)
    (hg : CountersOk ctx.budgets g.counters) :
    OkP (fun (r : Glob × Ctrl) => CtrlOk C r.2 ∧ CountersOk ctx.budgets r.1.counters) (apply ctx g c args) := by
  unfold apply
  refine OkP.bind (countApplication_counters hg) fun g' hg' => ?_
  refine OkP.bind (OkP.any _) fun env _ => ?_
  exact ⟨hc, hg'⟩

theorem mem_callsOfList {e : Expr} : ∀ {es : List Expr}, e ∈ es → callsOf e ⊆ callsOfList es
  | [], h => by simp at h
  | x :: xs, h => by
    simp at h
    rcases h with rfl | h
    · simp [callsOfList]
    · exact List.Subset.trans (mem_callsOfList h) (by simp [callsOfList])

theorem mem_callsOfFields {e : Expr} : ∀ {fs : List (String × Expr)}, e ∈ fs.map Prod.snd → callsOf e ⊆ callsOfFields fs
  | [], h => by simp at h
  | (n, x) :: xs, h => by
    simp at h
    rcases h with rfl | ⟨m, h⟩
    · simp [callsOfFields]
    · exact List.Subset.trans (mem_callsOfFields (List.mem_map.mpr ⟨(m, e), h, rfl⟩)) (by simp [callsOfFields])

theorem seqChildren_ok (e : Expr) : OkP (fun cs => ∀ c ∈ cs, callsOf c ⊆ callsOf e) (seqChildren e) := by
  match e with
  | .prelude _ args => intro c hc; exact mem_callsOfList hc
  | .core _ args => intro c hc; exact mem_callsOfList hc
  | .recur args => intro c hc; exact mem_callsOfList hc
  | .mkList es _ => intro c hc; exact mem_callsOfList hc
  | .mkTuple es => intro c hc; exact mem_callsOfList hc
  | .applyLocal _ _ args =>
    intro c hc; exact List.Subset.trans (mem_callsOfList hc) (by simp [callsOf])
  | .loop _ _ init _ =>
    intro c hc; exact List.Subset.trans (mem_callsOfList hc) (by simp [callsOf])
  | .call _ arg => intro c hc; simp at hc; subst hc; simp [callsOf]
  | .get r _ => intro c hc; simp at hc; subst hc; simp [callsOf]
  | .tupleItem r _ => intro c hc; simp at hc; rcases hc with rfl | rfl <;> simp [callsOf]
  | .wrap _ x _ => intro c hc; simp at hc; subst hc; simp [callsOf]
  | .coerce x _ => intro c hc; simp at hc; subst hc; simp [callsOf]
  | .mkRecord fs => intro c hc; exact mem_callsOfFields hc
  | .mkVariant _ _ none => intro c hc; simp at hc
  | .mkVariant _ _ (some x) => intro c hc; simp at hc; subst hc; simp [callsOf]
  | .parMap _ f xs => intro c hc; simp at hc; rcases hc with rfl | rfl <;> simp [callsOf]
  | .lit .. | .var _ | .let_ .. | .localFn .. | .if_ .. | .match_ .. | .lam .. | .attempt _ | .retry .. | .waitUntil ..
  | .par _ | .sleep _ => trivial

theorem closureOf_ok {C : List String} {env : Env} {e : Expr} (h : callsOf e ⊆ C) :
    OkP (CloOk C) (closureOf env e) := by
  unfold closureOf
  split
  · show CloOk C _; simpa [CloOk, callsOf] using h
  · split
    · show CloOk C _; simpa [CloOk, callsOf] using h
    · trivial
  · trivial

theorem requestDelay_ok {C : List String} {ctx : Ctx} {g : Glob} {f : Fiber} {ms : Nat} {stack : List Frame}
    (hs : StackOk C stack) (hg : CountersOk ctx.budgets g.counters) :
    OkP (StepOk C ctx.budgets) (requestDelay ctx g f ms stack) := by
  unfold requestDelay
  refine OkP.bind (countDelay_counters hg) fun g' hg' => ?_
  exact ⟨⟨trivial, hs⟩, by simp, hg'⟩

theorem unwind_ok {C : List String} {ctx : Ctx} {g : Glob} {f : Fiber} {raised : Raised}
    (hg : CountersOk ctx.budgets g.counters) :
    ∀ (frames : List Frame), StackOk C frames → OkP (StepOk C ctx.budgets) (unwind ctx g f raised frames)
  | [], _ => ⟨⟨trivial, by simp [StackOk]⟩, by simp, hg⟩
  | .attempt :: rest, hs => ⟨⟨trivial, hs.tail⟩, by simp, hg⟩
  | .retry delays kinds body env n :: rest, hs => by
    simp only [unwind]
    split
    · split
      · exact requestDelay_ok (StackOk.cons (hs.head : callsOf body ⊆ C) (StackOk.cons hs.head hs.tail)) hg
      · exact unwind_ok hg rest hs.tail
    · exact unwind_ok hg rest hs.tail
  | .join _ :: _, _ => trivial
  | .seq .. :: rest, hs => unwind_ok hg rest hs.tail
  | .let_ .. :: rest, hs => unwind_ok hg rest hs.tail
  | .if_ .. :: rest, hs => unwind_ok hg rest hs.tail
  | .match_ .. :: rest, hs => unwind_ok hg rest hs.tail
  | .wait .. :: rest, hs => unwind_ok hg rest hs.tail
  | .waitCheck .. :: rest, hs => unwind_ok hg rest hs.tail
  | .loop .. :: rest, hs => unwind_ok hg rest hs.tail
  | .each .. :: rest, hs => unwind_ok hg rest hs.tail
  | .then_ .. :: rest, hs => unwind_ok hg rest hs.tail

def ChildrenOk (C : List String) (B : Budgets) (j : Join) (r : Join × List Fiber × Glob) : Prop :=
  (∀ ch ∈ r.2.1, FiberOk C ch) ∧ CountersOk B r.2.2.counters ∧ r.1.clo = j.clo

theorem startChildren_ok {C : List String} {ctx : Ctx} {f : Fiber} {c : Clo} (hc : CloOk C c) :
    ∀ (fuel : Nat) (g : Glob) (j : Join), CountersOk ctx.budgets g.counters →
      OkP (ChildrenOk C ctx.budgets j) (startChildren ctx g f j c fuel)
  | 0, g, j, hg => ⟨by simp, hg, rfl⟩
  | fuel + 1, g, j, hg => by
    simp only [startChildren]
    split
    · split
      · trivial
      · refine OkP.bind (apply_ok hc hg) fun r hr => ?_
        refine OkP.bind (startChildren_ok hc fuel _ _ hr.2) fun r' hr' => ?_
        refine ⟨?_, hr'.2.1, hr'.2.2⟩
        intro ch hch
        simp at hch
        rcases hch with rfl | hch
        · exact ⟨hr.1, by simp [StackOk]⟩
        · exact hr'.1 ch hch
    · exact ⟨by simp, hg, rfl⟩

theorem selectClause_sub {env : Env} {v : Val} :
    ∀ (cs : List (Pat × Expr)), OkP (fun (r : Expr × Env) => callsOf r.1 ⊆ callsOfClauses cs) (selectClause env v cs)
  | [] => trivial
  | (p, body) :: cs => by
    simp only [selectClause]
    refine OkP.bind (OkP.any _) fun r _ => ?_
    split
    · simp [OkP, callsOfClauses]
    · exact OkP.mono (selectClause_sub cs) fun r h => List.Subset.trans h (by simp [callsOfClauses])

macro "okp_auto" : tactic => `(tactic|
  repeat (first
    | (apply OkP.bind (OkP.any _); intro _ _)
    | split
    | exact OkP.internal
    | exact OkP.failWith
    | trivial))

theorem finishSeq_ok {C : List String} {ctx : Ctx} {g : Glob} {f : Fiber} {stack : List Frame} {origin : Expr}
    {env : Env} {done : List Res} (hs : StackOk C stack) (ho : callsOf origin ⊆ C) (hd : ∀ r ∈ done, ResOk C r)
    (hg : CountersOk ctx.budgets g.counters) :
    OkP (StepOk C ctx.budgets) (finishSeq ctx g f stack origin env done) := by
  have hret : ∀ v, OkP (StepOk C ctx.budgets)
      (pure ({ f with stack, ctrl := .ret (.val v) }, ([] : List Fiber), g) : Except Failure _) :=
    fun v => ⟨⟨trivial, hs⟩, by simp, hg⟩
  match origin with
  | .prelude op args =>
    unfold finishSeq; dsimp only
    split
    · split
      · rename_i c rest
        have hc : CloOk C c := hd (.clo c) (by simp)
        generalize (if (op == Op.reduce) = true then (rest.head?, List.drop 1 rest) else (none, rest)) = pr
        obtain ⟨a0, ir⟩ := pr
        dsimp only
        split <;> refine OkP.bind (OkP.any _) fun acc _ => ?_ <;> split <;> (try trivial) <;> split <;>
          first
          | exact OkP.bind (OkP.any _) fun v _ => hret v
          | exact OkP.bind (apply_ok hc hg) fun r hr => ⟨⟨hr.1, StackOk.cons hc hs⟩, by simp, hr.2⟩
      · trivial
    · okp_auto
      all_goals first | trivial | exact ⟨⟨trivial, hs⟩, by simp, hg⟩
  | .core _ _ | .get _ _ | .tupleItem _ _ | .mkRecord _ | .mkList _ _ | .mkTuple _ | .wrap _ _ _ | .mkVariant _ _ _
  | .coerce _ _ | .recur _ =>
    unfold finishSeq; dsimp only
    okp_auto
    all_goals first | trivial | exact ⟨⟨trivial, hs⟩, by simp, hg⟩
  | .call cap arg =>
    unfold finishSeq; dsimp only
    refine OkP.bind (countCall_counters hg) fun g' hg' => ?_
    have hcap : cap ∈ C := ho (by simp [callsOf])
    okp_auto
    all_goals first | trivial | exact ⟨⟨hcap, hs⟩, by simp, hg'⟩
  | .applyLocal name fn args =>
    unfold finishSeq; dsimp only
    split
    · rename_i ps _ _ body _ defEnv _
      refine OkP.bind (OkP.any _) fun _ _ => ?_
      have hb : CloOk C ⟨ps, body, defEnv⟩ := by
        show callsOf body ⊆ C
        exact List.Subset.trans (by simp [callsOf]) ho
      refine OkP.bind (apply_ok hb hg) fun r hr => ?_
      exact ⟨⟨hr.1, hs⟩, by simp, hr.2⟩
    · trivial
  | .loop max bs init body =>
    unfold finishSeq; dsimp only
    refine OkP.bind (OkP.any _) fun _ _ => OkP.bind (OkP.any _) fun _ _ => ?_
    have hb : callsOf body ⊆ C := List.Subset.trans (by simp [callsOf]) ho
    exact ⟨⟨hb, StackOk.cons hb hs⟩, by simp, hg⟩
  | .parMap limit fn items =>
    unfold finishSeq; dsimp only
    split
    · rename_i x c items'
      have hc : CloOk C c := hd (.clo c) (by simp)
      refine OkP.bind (startChildren_ok hc _ g _ hg) fun r hr => ?_
      refine ⟨⟨trivial, StackOk.cons ?_ hs⟩, hr.1, hr.2.1⟩
      intro c' hc'
      have hcl := hr.2.2
      simp at hcl
      rw [hcl] at hc'
      cases hc'
      exact hc
    · trivial
  | .lit .. | .var _ | .let_ .. | .localFn .. | .if_ .. | .match_ .. | .lam .. | .attempt _ | .retry ..
  | .waitUntil .. | .par _ | .sleep _ => trivial

theorem evalNode_ok {C : List String} {ctx : Ctx} {g : Glob} {f : Fiber} {e : Expr} {env : Env}
    (hs : StackOk C f.stack) (he : callsOf e ⊆ C) (hg : CountersOk ctx.budgets g.counters) :
    OkP (StepOk C ctx.budgets) (evalNode ctx g f e env) := by
  have sub : ∀ {x : Expr}, callsOf x ⊆ callsOf e → callsOf x ⊆ C := fun h => List.Subset.trans h he
  match e with
  | .lit v t => exact ⟨⟨trivial, hs⟩, by simp, hg⟩
  | .var n =>
    unfold evalNode; dsimp only
    okp_auto
    all_goals first | trivial | exact ⟨⟨trivial, hs⟩, by simp, hg⟩
  | .lam ps tys res body cap =>
    unfold evalNode; dsimp only
    exact OkP.bind (closureOf_ok he) fun c hc => ⟨⟨hc, hs⟩, by simp, hg⟩
  | .let_ b v body =>
    exact ⟨⟨sub (by simp [callsOf]), StackOk.cons (sub (by simp [callsOf]) : callsOf body ⊆ C) hs⟩, by simp, hg⟩
  | .localFn n body => exact ⟨⟨sub (by simp [callsOf]), hs⟩, by simp, hg⟩
  | .if_ c t el =>
    refine ⟨⟨sub (by simp [callsOf]), StackOk.cons ?_ hs⟩, by simp, hg⟩
    exact ⟨sub (by simp [callsOf]), sub (by simp [callsOf])⟩
  | .match_ sc cs =>
    refine ⟨⟨sub (by simp [callsOf]), StackOk.cons ?_ hs⟩, by simp, hg⟩
    exact List.Subset.trans (by simp [callsOf] : callsOfClauses cs ⊆ callsOf (.match_ sc cs)) he
  | .attempt body => exact ⟨⟨sub (by simp [callsOf]), StackOk.cons trivial hs⟩, by simp, hg⟩
  | .retry ds ks body =>
    exact ⟨⟨sub (by simp [callsOf]), StackOk.cons (sub (by simp [callsOf]) : callsOf body ⊆ C) hs⟩, by simp, hg⟩
  | .waitUntil is pred body =>
    unfold evalNode; dsimp only
    refine OkP.bind (closureOf_ok (sub (by simp [callsOf]) : callsOf pred ⊆ C)) fun c hc => ?_
    refine ⟨⟨sub (by simp [callsOf]), StackOk.cons ?_ hs⟩, by simp, hg⟩
    exact ⟨hc, sub (by simp [callsOf])⟩
  | .sleep 0 => exact ⟨⟨trivial, hs⟩, by simp, hg⟩
  | .sleep (k + 1) => exact requestDelay_ok hs hg
  | .par bs =>
    refine ⟨⟨trivial, StackOk.cons (fun c h => by simp at h) hs⟩, ?_, hg⟩
    intro ch hch
    simp at hch
    obtain ⟨b, i, hbi, rfl⟩ := hch
    refine ⟨?_, by simp [StackOk]⟩
    show callsOf b ⊆ C
    have hb : b ∈ bs := by
      obtain ⟨_, _, heq⟩ := List.mem_zipIdx hbi
      rw [heq]; exact List.getElem_mem _
    exact sub (List.Subset.trans (mem_callsOfList hb) (by simp [callsOf]))
  | .prelude .. | .core .. | .applyLocal .. | .recur _ | .call .. | .get .. | .tupleItem .. | .mkRecord _ | .mkList .. | .mkTuple _
  | .wrap .. | .coerce .. | .mkVariant .. | .loop .. | .parMap .. =>
    unfold evalNode; dsimp only
    refine OkP.bind (seqChildren_ok _) fun cs hcs => ?_
    split
    · exact finishSeq_ok hs he (by simp) hg
    · rename_i c rest
      refine ⟨⟨sub (hcs c (by simp)), StackOk.cons ⟨he, by simp, fun x hx => sub (hcs x (by simp [hx]))⟩ hs⟩,
        by simp, hg⟩

theorem resumeFrame_ok {C : List String} {ctx : Ctx} {g : Glob} {f : Fiber} {fr : Frame} {stack : List Frame}
    {r : Res} (hfr : FrameOk C fr) (hs : StackOk C stack) (hr : ResOk C r) (hg : CountersOk ctx.budgets g.counters) :
    OkP (StepOk C ctx.budgets) (resumeFrame ctx g f fr stack r) := by
  unfold resumeFrame; dsimp only
  split
  next =>
    split
    · trivial
    · exact OkP.bind (OkP.any _) fun _ _ => ⟨⟨hfr, StackOk.cons hfr hs⟩, by simp, hg⟩
  next => exact ⟨⟨hr, hs⟩, by simp, hg⟩
  next =>
    obtain ⟨ho, hd, hrest⟩ := hfr
    split
    · rename_i next rest'
      refine ⟨⟨hrest next (by simp), StackOk.cons ⟨ho, ?_, fun x hx => hrest x (by simp [hx])⟩ hs⟩, by simp, hg⟩
      intro x hx; simp at hx; rcases hx with hx | rfl; exact hd x hx; exact hr
    · refine finishSeq_ok hs ho ?_ hg
      intro x hx; simp at hx; rcases hx with hx | rfl; exact hd x hx; exact hr
  next =>
    exact OkP.bind (OkP.any _) fun _ _ => OkP.bind (OkP.any _) fun _ _ => ⟨⟨hfr, hs⟩, by simp, hg⟩
  next =>
    split
    · exact ⟨⟨hfr.1, hs⟩, by simp, hg⟩
    · exact ⟨⟨hfr.2, hs⟩, by simp, hg⟩
  next =>
    rename_i cs _ _
    refine OkP.bind (OkP.any _) fun v _ => ?_
    refine OkP.bind (selectClause_sub cs) fun p hp => ?_
    exact ⟨⟨List.Subset.trans hp hfr, hs⟩, by simp, hg⟩
  next => exact OkP.bind (OkP.any _) fun _ _ => ⟨⟨trivial, hs⟩, by simp, hg⟩
  next => exact ⟨⟨hr, hs⟩, by simp, hg⟩
  next => exact ⟨⟨hr, hs⟩, by simp, hg⟩
  next =>
    refine OkP.bind (OkP.any _) fun v _ => ?_
    refine OkP.bind (apply_ok hfr.1 hg) fun r' hr' => ?_
    exact ⟨⟨hr'.1, StackOk.cons hfr hs⟩, by simp, hr'.2⟩
  next =>
    split
    · exact ⟨⟨trivial, hs⟩, by simp, hg⟩
    · split
      · exact ⟨⟨trivial, hs⟩, by simp, hg⟩
      · exact requestDelay_ok (StackOk.cons (hfr.2 : callsOf _ ⊆ C) (StackOk.cons hfr hs)) hg
  next =>
    refine OkP.bind (OkP.any _) fun v _ => ?_
    split
    · exact ⟨⟨trivial, hs⟩, by simp, hg⟩
    · split
      · refine OkP.bind (apply_ok hfr hg) fun r' hr' => ?_
        exact ⟨⟨hr'.1, StackOk.cons hfr hs⟩, by simp, hr'.2⟩
      · exact OkP.bind (OkP.any _) fun _ _ => ⟨⟨trivial, hs⟩, by simp, hg⟩
  next => exact ⟨⟨hfr, hs⟩, by simp, hg⟩
  next => trivial

theorem stepFiber_ok {C : List String} {ctx : Ctx} {g : Glob} {f : Fiber} (hf : FiberOk C f)
    (hg : CountersOk ctx.budgets g.counters) : OkP (StepOk C ctx.budgets) (stepFiber ctx g f) := by
  unfold stepFiber
  refine OkP.bind (tick_counters hg) fun g' hg' => ?_
  obtain ⟨hc, hs⟩ := hf
  split
  · exact unwind_ok hg' _ hs
  · rename_i e env h
    rw [h] at hc
    exact evalNode_ok hs hc hg'
  · rename_i r h
    rw [h] at hc
    split
    · split
      · exact ⟨⟨trivial, hs⟩, by simp, hg'⟩
      · trivial
    · rename_i fr stack hst
      rw [hst] at hs
      exact resumeFrame_ok hs.head hs.tail hc hg'
  · trivial

end Sx
