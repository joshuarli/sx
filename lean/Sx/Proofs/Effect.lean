import Sx.Proofs.MachineInv
import Sx.Check

namespace Sx

def EventOk (C : List String) : Event → Prop
  | .request _ cap _ => cap ∈ C
  | _ => True

structure MInv (ctx : Ctx) (m : Machine) : Prop where
  same : m.ctx = ctx
  fibers : ∀ f ∈ m.fibers, FiberOk (callsOf ctx.prog.body) f
  trace : ∀ ev ∈ m.trace, EventOk (callsOf ctx.prog.body) ev
  counters : CountersOk ctx.budgets m.glob.counters

theorem fiber?_mem {m : Machine} {id : Nat} {f : Fiber} (h : m.fiber? id = some f) : f ∈ m.fibers :=
  List.mem_of_find?_eq_some h

theorem setFiber_ok {ctx : Ctx} {m : Machine} {f : Fiber} (hm : MInv ctx m)
    (hf : FiberOk (callsOf ctx.prog.body) f) : MInv ctx (m.setFiber f) := by
  refine ⟨hm.same, ?_, hm.trace, hm.counters⟩
  intro x hx
  simp only [Machine.setFiber, List.mem_map] at hx
  obtain ⟨y, hy, rfl⟩ := hx
  split
  · exact hf
  · exact hm.fibers y hy

theorem recordChild_ok {C : List String} {i : Nat} {o : Sum Val Val} {s : List Frame} (h : StackOk C s) :
    StackOk C (recordChild i o s) := by
  unfold recordChild
  split
  · split
    · exact StackOk.cons (h.head : FrameOk C (.join _)) h.tail
    · exact StackOk.cons (h.head : FrameOk C (.join _)) h.tail
  · exact h

theorem notifyParent_ok {ctx : Ctx} {m : Machine} {f : Fiber} (hm : MInv ctx m) : MInv ctx (m.notifyParent f) := by
  unfold Machine.notifyParent
  split
  · split
    · rename_i p hp
      have := hm.fibers p (fiber?_mem hp)
      refine setFiber_ok hm ⟨this.1, ?_⟩
      dsimp only
      exact recordChild_ok this.2
    · exact hm
  · split
    · rename_i p hp
      have := hm.fibers p (fiber?_mem hp)
      refine setFiber_ok hm ⟨this.1, ?_⟩
      dsimp only
      exact recordChild_ok this.2
    · exact hm
  · exact hm

theorem stepOne_ok {ctx : Ctx} {m : Machine} {f : Fiber} (hm : MInv ctx m) (hf : f ∈ m.fibers) :
    OkP (MInv ctx) (m.stepOne f) := by
  have hm' := hm
  obtain ⟨hsame, hfib, _, hcnt⟩ := hm'
  subst hsame
  unfold Machine.stepOne
  refine OkP.bind (stepFiber_ok (hfib f hf) hcnt) fun r hr => ?_
  obtain ⟨f', cs, g⟩ := r
  obtain ⟨hf', hcs, hg'⟩ := hr
  apply notifyParent_ok
  have h1 := setFiber_ok hm hf'
  refine ⟨h1.same, ?_, h1.trace, hg'⟩
  intro x hx
  simp at hx
  rcases hx with hx | hx
  · exact h1.fibers x hx
  · exact hcs x hx

theorem runFiber_ok {ctx : Ctx} (id : Nat) : ∀ (fuel : Nat) (m : Machine), MInv ctx m → OkP (MInv ctx) (m.runFiber id fuel)
  | 0, _, _ => trivial
  | fuel + 1, m, hm => by
    unfold Machine.runFiber
    split
    · rename_i f hf
      split
      · exact OkP.bind (stepOne_ok hm (fiber?_mem hf)) fun m' hm' => runFiber_ok id fuel m' hm'
      · exact hm
    · exact hm

theorem foldlM_ok {α : Type} {P : Machine → Prop} {f : Machine → α → Except Failure Machine}
    (hf : ∀ m a, P m → OkP P (f m a)) : ∀ (l : List α) (m : Machine), P m → OkP P (l.foldlM f m)
  | [], m, h => h
  | a :: l, m, h => by
    simp only [List.foldlM_cons]
    exact OkP.bind (hf m a h) fun m' h' => foldlM_ok hf l m' h'

theorem pass_ok {ctx : Ctx} {m : Machine} {fuel : Nat} (hm : MInv ctx m) : OkP (MInv ctx) (m.pass fuel) :=
  foldlM_ok (fun m id h => runFiber_ok id fuel m h) _ m hm

theorem cancel_ok {ctx : Ctx} : ∀ (fuel : Nat) (m : Machine) (id : Nat), MInv ctx m → MInv ctx (m.cancel id fuel)
  | 0, _, _, h => h
  | fuel + 1, m, id, h => by
    unfold Machine.cancel
    split
    · exact h
    · rename_i f hf
      have h1 : MInv ctx (m.setFiber { f with cancelled := true }) := setFiber_ok h (h.fibers f (fiber?_mem hf))
      have key : ∀ (l : List Nat) (m' : Machine), MInv ctx m' →
          MInv ctx (l.foldl (fun m c => m.cancel c fuel) m') := by
        intro l
        induction l with
        | nil => intro m' h; exact h
        | cons c cs ih => intro m' h; exact ih _ (cancel_ok fuel m' c h)
      exact key _ _ h1

theorem cancels_ok {ctx : Ctx} : ∀ (l : List Nat) (m : Machine), MInv ctx m →
    MInv ctx (l.foldl (fun m c => m.cancel c (m.fibers.length + 1)) m)
  | [], m, h => h
  | c :: cs, m, h => cancels_ok cs _ (cancel_ok _ m c h)

theorem settleOne_ok {ctx : Ctx} {m : Machine} {id : Nat} (hm : MInv ctx m) : OkP (MInv ctx) (m.settleOne id) := by
  unfold Machine.settleOne
  split
  · rename_i f hf
    have hfo := hm.fibers f (fiber?_mem hf)
    split
    · rename_i j rest hctrl hstack hcanc
      have hs : StackOk (callsOf ctx.prog.body) (Frame.join j :: rest) := by rw [← hstack]; exact hfo.2
      split
      · have h1 := cancels_ok j.children m hm
        dsimp only
        split
        · rename_i f' _
          exact setFiber_ok h1 ⟨trivial, hs.tail⟩
        · exact h1
      · have hjc : ∀ c, j.clo = some c → CloOk (callsOf ctx.prog.body) c := hs.head
        dsimp only
        split
        · rename_i c hc
          have hcnt : CountersOk m.ctx.budgets m.glob.counters := hm.same ▸ hm.counters
          refine OkP.bind (startChildren_ok (ctx := m.ctx) (f := f) (hjc c hc) (j.count + 1) m.glob j hcnt)
            fun r hr => ?_
          obtain ⟨j', cs, g⟩ := r
          obtain ⟨hcs, hg, hclo⟩ := hr
          simp only at hcs hg hclo ⊢
          rw [hm.same] at hg
          have h2 : MInv ctx { m with fibers := m.fibers ++ cs, glob := g } := by
            refine ⟨hm.same, ?_, hm.trace, hg⟩
            intro x hx; simp at hx; rcases hx with hx | hx; exact hm.fibers x hx; exact hcs x hx
          split
          · exact OkP.bind (OkP.any _) fun _ _ => setFiber_ok h2 ⟨trivial, hs.tail⟩
          · refine setFiber_ok h2 ⟨hfo.1, StackOk.cons ?_ hs.tail⟩
            intro c' hc'
            rw [hclo] at hc'
            exact hjc c' hc'
        · simp only [except_pure, except_ok_bind]
          have h2 : MInv ctx { m with fibers := m.fibers ++ [], glob := m.glob } := by
            refine ⟨hm.same, ?_, hm.trace, hm.counters⟩
            intro x hx; simp at hx; exact hm.fibers x hx
          split
          · exact OkP.bind (OkP.any _) fun _ _ => setFiber_ok h2 ⟨trivial, hs.tail⟩
          · exact setFiber_ok h2 ⟨hfo.1, StackOk.cons hs.head hs.tail⟩
    · exact hm
  · exact hm

theorem settle_ok {ctx : Ctx} {m : Machine} (hm : MInv ctx m) : OkP (MInv ctx) m.settle :=
  foldlM_ok (fun _ _ h => settleOne_ok h) _ m hm

theorem insertBy_mem {α : Type} {lt : α → α → Bool} {x : α} :
    ∀ (ys : List α) (z : α), z ∈ insertBy lt x ys → z = x ∨ z ∈ ys
  | [], z, h => by simp [insertBy] at h; exact .inl h
  | y :: ys, z, h => by
    simp only [insertBy] at h
    split at h
    · simp at h; rcases h with h | h | h; exact .inl h; exact .inr (by simp [h]); exact .inr (by simp [h])
    · simp at h
      rcases h with h | h
      · exact .inr (by simp [h])
      · rcases insertBy_mem ys z h with h | h; exact .inl h; exact .inr (by simp [h])

theorem sortBy_mem {α : Type} {lt : α → α → Bool} {xs : List α} {z : α} (h : z ∈ sortBy lt xs) : z ∈ xs := by
  unfold sortBy at h
  have key : ∀ (l acc : List α), z ∈ l.foldl (fun acc x => insertBy lt x acc) acc → z ∈ acc ∨ z ∈ l := by
    intro l
    induction l with
    | nil => intro acc h; exact .inl h
    | cons x l ih =>
      intro acc h
      simp only [List.foldl] at h
      rcases ih _ h with h | h
      · rcases insertBy_mem acc z h with h | h
        · exact .inr (by simp [h])
        · exact .inl h
      · exact .inr (by simp [h])
  rcases key xs [] h with h | h
  · simp at h
  · exact h

theorem waitOk {C : List String} {f : Fiber} {r : Req} {b : Bool} (hf : FiberOk C f) (hc : f.ctrl = .wait r false) :
    FiberOk C { f with ctrl := .wait r b } := by
  obtain ⟨hctrl, hs⟩ := hf
  rw [hc] at hctrl
  refine ⟨?_, hs⟩
  cases r <;> exact hctrl

theorem issue_ok {ctx : Ctx} {m : Machine} (hm : MInv ctx m) : MInv ctx m.issue.1 := by
  unfold Machine.issue
  dsimp only
  generalize hw : m.fibers.filterMap _ = waiting
  have hwait : ∀ p ∈ waiting, p.1 ∈ m.fibers ∧ p.1.ctrl = .wait p.2 false := by
    intro p hp
    rw [← hw] at hp
    simp only [List.mem_filterMap] at hp
    obtain ⟨f, hf, hfp⟩ := hp
    split at hfp
    · rename_i r hr
      split at hfp
      · cases hfp
      · cases hfp; exact ⟨hf, hr⟩
    · cases hfp
  generalize hc : List.take _ (sortBy _ waiting) = chosen
  have hchosen : ∀ p ∈ chosen, FiberOk (callsOf ctx.prog.body) p.1 ∧ p.1.ctrl = .wait p.2 false := by
    intro p hp
    rw [← hc] at hp
    have := hwait p (sortBy_mem (List.mem_of_mem_take hp))
    exact ⟨hm.fibers _ this.1, this.2⟩
  have key : ∀ (l : List (Fiber × Req)) (acc : Machine × List Req), (∀ p ∈ l, FiberOk (callsOf ctx.prog.body) p.1 ∧
      p.1.ctrl = .wait p.2 false) → MInv ctx acc.1 →
      MInv ctx (l.foldl (fun (acc : Machine × List Req) (p : Fiber × Req) =>
        ({ (acc.1.setFiber { p.1 with ctrl := .wait p.2 true }) with
            outstanding := (acc.1.setFiber { p.1 with ctrl := .wait p.2 true }).outstanding ++
              [⟨p.2.id, p.1.id, p.2.path, p.2.seq⟩],
            trace := (acc.1.setFiber { p.1 with ctrl := .wait p.2 true }).trace ++
              [match p.2 with
                | .call _ _ cap input _ => Event.request p.2.id cap input
                | .delay _ _ ms => Event.delay p.2.id ms] }, acc.2 ++ [p.2])) acc).1 := by
    intro l
    induction l with
    | nil => intro acc _ h; exact h
    | cons p l ih =>
      intro acc hl h
      obtain ⟨f, r⟩ := p
      have hp := hl (f, r) (by simp)
      apply ih _ (fun q hq => hl q (by simp [hq]))
      have h1 := setFiber_ok h (waitOk (b := true) hp.1 hp.2)
      refine ⟨h1.same, h1.fibers, ?_, h1.counters⟩
      intro ev hev
      simp at hev
      rcases hev with hev | rfl
      · exact h1.trace ev hev
      · have hctrl := hp.1.1
        rw [hp.2] at hctrl
        cases r with
        | call _ _ cap _ _ => exact hctrl
        | delay _ _ _ => trivial
  exact key chosen (m, []) hchosen hm

theorem loop_ok {ctx : Ctx} (fuel : Nat) : ∀ (n : Nat) (m : Machine), MInv ctx m → MInv ctx (m.loop fuel n).1
  | 0, m, h => h
  | n + 1, m, h => by
    unfold Machine.loop
    have hps : OkP (MInv ctx) (m.pass fuel >>= Machine.settle) :=
      OkP.bind (pass_ok h) fun m' h' => settle_ok h'
    split
    · exact h
    · rename_i m' hm'
      rw [hm'] at hps
      split
      · exact hps
      · exact hps
      · split
        · exact loop_ok fuel n m' hps
        · have := issue_ok hps
          generalize m'.issue = r at this ⊢
          obtain ⟨m'', reqs⟩ := r
          dsimp only
          split <;> exact this

theorem run_ok {ctx : Ctx} {m : Machine} (h : MInv ctx m) : MInv ctx m.run.1 := loop_ok _ _ m h

theorem deliver_ok {ctx : Ctx} {m : Machine} {o : Outstanding} {resp : Response} (h : MInv ctx m) :
    OkP (MInv ctx) (m.deliver o resp) := by
  unfold Machine.deliver
  split
  · trivial
  · rename_i f hf
    have hfo := h.fibers f (fiber?_mem hf)
    split
    · exact h
    · split
      · exact setFiber_ok h ⟨trivial, hfo.2⟩
      · trivial
      · split
        · exact setFiber_ok h ⟨trivial, hfo.2⟩
        · trivial
      · split
        · trivial
        · exact OkP.bind (OkP.any _) fun _ _ => setFiber_ok h ⟨trivial, hfo.2⟩
      · trivial

theorem resume_ok {ctx : Ctx} {m : Machine} {responses : List (String × Response)} (h : MInv ctx m) :
    MInv ctx (m.resume responses).1 := by
  unfold Machine.resume
  split
  · exact h
  · have key : ∀ (os : List Outstanding) (m' : Machine), MInv ctx m' →
        MInv ctx (Machine.resume.deliverAll responses m' os).1 ∧
        ((Machine.resume.deliverAll responses m' os).2 = none →
          MInv ctx (Machine.resume.deliverAll responses m' os).1) := by
      intro os
      induction os with
      | nil => intro m' h'; simp [Machine.resume.deliverAll]; exact h'
      | cons o os ih =>
        intro m' h'
        simp only [Machine.resume.deliverAll]
        split
        · exact ⟨h', fun _ => h'⟩
        · rename_i resp _
          have h1 : MInv ctx { m' with trace := m'.trace ++ [responseEvent o.id resp] } := by
            refine ⟨h'.same, h'.fibers, ?_, h'.counters⟩
            intro ev hev
            simp at hev
            rcases hev with hev | rfl
            · exact h'.trace ev hev
            · cases resp <;> trivial
          have hd := deliver_ok (o := o) (resp := resp) h1
          split
          · exact ⟨h1, fun _ => h1⟩
          · rename_i m'' hm''
            rw [hm''] at hd
            exact ih m'' hd
    have h0 : MInv ctx { m with outstanding := [] } := ⟨h.same, h.fibers, h.trace, h.counters⟩
    have hk := (key (sortBy outstandingLt m.outstanding) _ h0).1
    dsimp only
    generalize Machine.resume.deliverAll responses { m with outstanding := [] } (sortBy outstandingLt m.outstanding) = r
      at hk ⊢
    obtain ⟨m', res⟩ := r
    cases res with
    | some f => exact hk
    | none => exact run_ok hk

theorem init_ok (ctx : Ctx) (inputs : List (String × Val)) : MInv ctx (Machine.init ctx inputs) := by
  refine ⟨rfl, ?_, by simp [Machine.init], ⟨by simp [Machine.init], by simp [Machine.init],
    by simp [Machine.init], by simp [Machine.init]⟩⟩
  intro f hf
  simp [Machine.init] at hf
  subst hf
  exact ⟨List.Subset.refl _, by simp [StackOk]⟩

inductive Reachable (ctx : Ctx) : Machine → Prop
  | init (inputs : List (String × Val)) : Reachable ctx (Machine.init ctx inputs)
  | run {m : Machine} : Reachable ctx m → Reachable ctx m.run.1
  | resume {m : Machine} (responses : List (String × Response)) : Reachable ctx m →
      Reachable ctx (m.resume responses).1

theorem reachable_inv {ctx : Ctx} {m : Machine} (h : Reachable ctx m) : MInv ctx m := by
  induction h with
  | init inputs => exact init_ok ctx inputs
  | run _ ih => exact run_ok ih
  | resume responses _ ih => exact resume_ok ih

theorem effect_soundness_calls {ctx : Ctx} {m : Machine} (h : Reachable ctx m) :
    ∀ id cap input, Event.request id cap input ∈ m.trace → cap ∈ callsOf ctx.prog.body :=
  fun _ _ _ hev => (reachable_inv h).trace _ hev

theorem checkProgram_effects {p : Program} (h : checkProgram p = .ok ()) : ∀ c ∈ callsOf p.body, c ∈ p.caps := by
  unfold checkProgram at h
  split at h
  · cases h
  · split at h
    · cases h
    · split at h
      · cases h
      · rename_i hall
        intro c hc
        simp only [Bool.not_eq_true', Bool.not_eq_false] at hall
        have := List.all_eq_true.mp hall c hc
        simpa using this

theorem effect_soundness {ctx : Ctx} {m : Machine} (hchecked : checkProgram ctx.prog = .ok ()) (h : Reachable ctx m) :
    ∀ id cap input, Event.request id cap input ∈ m.trace → cap ∈ ctx.prog.caps :=
  fun id cap input hev => checkProgram_effects hchecked cap (effect_soundness_calls h id cap input hev)

theorem budgets_respected {ctx : Ctx} {m : Machine} (h : Reachable ctx m) : CountersOk ctx.budgets m.glob.counters :=
  (reachable_inv h).counters

end Sx
