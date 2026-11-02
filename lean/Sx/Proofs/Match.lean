import Sx.Proofs.Basic

namespace Sx

theorem matchPat_typed {E : Enums} {ρ : Env} :
    ∀ (p : Pat) (Γ Γ' : TyEnv) (t : Ty) (v : Val), patTy E Γ p t = some Γ' → VT E v t → ET E ρ Γ →
      matchPat ρ p v = .ok none ∨ ∃ ρ', matchPat ρ p v = .ok (some ρ') ∧ ET E ρ' Γ'
  | .any b, Γ, Γ', t, v, hp, hv, he => by
    simp only [patTy] at hp
    obtain ⟨ρ', h, h'⟩ := bind_typed b t v hp hv he
    exact .inr ⟨ρ', by simp [matchPat, h], h'⟩
  | .none, Γ, Γ', t, v, hp, hv, he => by
    cases t <;> simp [patTy] at hp
    subst hp
    cases hv with
    | none => exact .inr ⟨ρ, by simp [matchPat], he⟩
    | some _ => exact .inl (by simp [matchPat])
  | .some q, Γ, Γ', t, v, hp, hv, he => by
    cases t <;> simp [patTy] at hp
    cases hv with
    | none => exact .inl (by simp [matchPat])
    | some hw => simpa [matchPat] using matchPat_typed q Γ Γ' _ _ hp hw he
  | .ok q, Γ, Γ', t, v, hp, hv, he => by
    cases t <;> simp [patTy] at hp
    cases hv with
    | ok hw => simpa [matchPat] using matchPat_typed q Γ Γ' _ _ hp hw he
    | err _ => exact .inl (by simp [matchPat])
  | .err q, Γ, Γ', t, v, hp, hv, he => by
    cases t <;> simp [patTy] at hp
    cases hv with
    | ok _ => exact .inl (by simp [matchPat])
    | err hw => simpa [matchPat] using matchPat_typed q Γ Γ' _ _ hp hw he
  | .variant n none, Γ, Γ', t, v, hp, hv, he => by
    cases t with
    | enum e =>
      simp only [patTy] at hp
      split at hp
      · cases hp
        cases hv with
        | @variant _ m _ =>
          by_cases hm : n = m
          · exact .inr ⟨ρ, by simp [matchPat, hm], he⟩
          · exact .inl (by simp [matchPat, hm])
        | @variantPayload _ m _ _ =>
          by_cases hm : n = m
          · exact .inr ⟨ρ, by simp [matchPat, hm], he⟩
          · exact .inl (by simp [matchPat, hm])
      · cases hp
    | _ => simp [patTy] at hp
  | .variant n (some q), Γ, Γ', t, v, hp, hv, he => by
    cases t with
    | enum e =>
      simp only [patTy] at hp
      split at hp
      · rename_i pt hpt
        cases hv with
        | @variant _ m hvm =>
          by_cases hm : n = m
          · subst hm; rw [hvm] at hpt; cases hpt
          · exact .inl (by simp [matchPat, hm])
        | @variantPayload _ m pt' w hvm hw =>
          by_cases hm : n = m
          · subst hm
            rw [hvm] at hpt; cases hpt
            simpa [matchPat] using matchPat_typed q Γ Γ' _ _ hp hw he
          · exact .inl (by simp [matchPat, hm])
      · cases hp
    | _ => simp [patTy] at hp

end Sx

namespace Sx

def Matches (ρ : Env) (p : Pat) (v : Val) : Prop := ∃ ρ', matchPat ρ p v = .ok (some ρ')

theorem any_matches {E : Enums} {ρ : Env} {Γ Γ' : TyEnv} {b : Binder} {t : Ty} {v : Val}
    (hp : patTy E Γ (.any b) t = some Γ') (hv : VT E v t) (he : ET E ρ Γ) : Matches ρ (.any b) v := by
  simp only [patTy] at hp
  obtain ⟨ρ', h, _⟩ := bind_typed b t v hp hv he
  exact ⟨ρ', by simp [matchPat, h]⟩

theorem fieldLookup_some_mem {α : Type} {l : List (String × α)} {n : String} {x : α}
    (h : fieldLookup l n = some x) : ∃ y, (n, y) ∈ l := ⟨x, fieldLookup_mem h⟩

theorem exhaustive_witness {E : Enums} {ρ : Env} {Γ : TyEnv} {pats : List Pat} {t : Ty} {v : Val}
    (hx : exhaustive E pats t = true) (hv : VT E v t) (he : ET E ρ Γ)
    (hp : ∀ p ∈ pats, ∃ Γ', patTy E Γ p t = some Γ') : ∃ p ∈ pats, Matches ρ p v := by
  simp only [exhaustive, Bool.or_eq_true, List.any_eq_true] at hx
  rcases hx with ⟨p, hmem, hirr⟩ | hx
  · cases p with
    | any b =>
      obtain ⟨Γ', hΓ'⟩ := hp _ hmem
      exact ⟨_, hmem, any_matches hΓ' hv he⟩
    | _ => simp [Pat.irrefutable] at hirr
  · cases t with
    | option a =>
      simp only [Bool.and_eq_true, List.any_eq_true] at hx
      obtain ⟨⟨pn, hpn, hn⟩, ⟨ps, hps, hs⟩⟩ := hx
      cases hv with
      | none =>
        cases pn <;> simp at hn
        exact ⟨_, hpn, ρ, by simp [matchPat]⟩
      | some hw =>
        cases ps with
        | some q =>
          cases q with
          | any b =>
            obtain ⟨Γ', hΓ'⟩ := hp _ hps
            simp only [patTy] at hΓ'
            obtain ⟨ρ', h, _⟩ := bind_typed b a _ hΓ' hw he
            exact ⟨_, hps, ρ', by simp [matchPat, h]⟩
          | _ => simp [Pat.irrefutable] at hs
        | _ => simp at hs
    | result a b =>
      simp only [Bool.and_eq_true, List.any_eq_true] at hx
      obtain ⟨⟨po, hpo, ho⟩, ⟨pe, hpe, hpe'⟩⟩ := hx
      cases hv with
      | ok hw =>
        cases po with
        | ok q =>
          cases q with
          | any bd =>
            obtain ⟨Γ', hΓ'⟩ := hp _ hpo
            simp only [patTy] at hΓ'
            obtain ⟨ρ', h, _⟩ := bind_typed bd a _ hΓ' hw he
            exact ⟨_, hpo, ρ', by simp [matchPat, h]⟩
          | _ => simp [Pat.irrefutable] at ho
        | _ => simp at ho
      | err hw =>
        cases pe with
        | err q =>
          cases q with
          | any bd =>
            obtain ⟨Γ', hΓ'⟩ := hp _ hpe
            simp only [patTy] at hΓ'
            obtain ⟨ρ', h, _⟩ := bind_typed bd b _ hΓ' hw he
            exact ⟨_, hpe, ρ', by simp [matchPat, h]⟩
          | _ => simp [Pat.irrefutable] at hpe'
        | _ => simp at hpe'
    | enum e =>
      simp only at hx
      split at hx
      · rename_i vs hvs
        rw [List.all_eq_true] at hx
        have cover : ∀ n x, variantPayload E e n = some x → coversVariant pats n = true := by
          intro n x hn
          simp only [variantPayload, hvs] at hn
          exact hx (n, x) (fieldLookup_mem hn)
        cases hv with
        | @variant _ n hvn =>
          have hc := cover n _ hvn
          simp only [coversVariant, List.any_eq_true] at hc
          obtain ⟨q, hq, hqm⟩ := hc
          cases q with
          | variant m inner =>
            cases inner with
            | none =>
              simp at hqm; subst hqm
              exact ⟨_, hq, ρ, by simp [matchPat]⟩
            | some r =>
              obtain ⟨Γ', hΓ'⟩ := hp _ hq
              simp at hqm; obtain ⟨hmn, _⟩ := hqm; subst hmn
              simp only [patTy, hvn] at hΓ'
              cases hΓ'
          | _ => simp at hqm
        | @variantPayload _ n pt w hvn hw =>
          have hc := cover n _ hvn
          simp only [coversVariant, List.any_eq_true] at hc
          obtain ⟨q, hq, hqm⟩ := hc
          cases q with
          | variant m inner =>
            cases inner with
            | none =>
              simp at hqm; subst hqm
              exact ⟨_, hq, ρ, by simp [matchPat]⟩
            | some r =>
              simp at hqm; obtain ⟨hmn, hirr⟩ := hqm; subst hmn
              cases r with
              | any bd =>
                obtain ⟨Γ', hΓ'⟩ := hp _ hq
                simp only [patTy, hvn] at hΓ'
                obtain ⟨ρ', h, _⟩ := bind_typed bd pt w hΓ' hw he
                exact ⟨_, hq, ρ', by simp [matchPat, h]⟩
              | _ => simp [Pat.irrefutable] at hirr
          | _ => simp at hqm
      · cases hx
    | never => cases hv
    | _ => simp at hx

theorem Clauses.typed {S : Sig} {Γ : TyEnv} {L : Option (List Ty)} {ts T : Ty} :
    ∀ {cs : List (Pat × Expr)}, Clauses S Γ L cs ts T → ∀ p ∈ cs.map Prod.fst, ∃ Γ', patTy S.enums Γ p ts = some Γ'
  | [], _ => by simp
  | (p, body) :: cs, h => by
    cases h with
    | cons hp _ hcs =>
      intro q hq
      simp at hq
      rcases hq with rfl | ⟨b, hb⟩
      · exact ⟨_, hp⟩
      · exact Clauses.typed hcs q (List.mem_map.mpr ⟨(q, b), hb, rfl⟩)

theorem selectClause_typed {S : Sig} {Γ : TyEnv} {L : Option (List Ty)} {ts T : Ty} {ρ : Env} {v : Val}
    (hv : VT S.enums v ts) (he : ET S.enums ρ Γ) :
    ∀ {cs : List (Pat × Expr)}, Clauses S Γ L cs ts T → (∃ p ∈ cs.map Prod.fst, Matches ρ p v) →
      ∃ body ρ' Γ', selectClause ρ v cs = .ok (body, ρ') ∧ ET S.enums ρ' Γ' ∧ HasType S Γ' L body T
  | [], _, hw => by simp at hw
  | (p, body) :: cs, h, hw => by
    cases h with
    | cons hp hb hcs =>
      rcases matchPat_typed p Γ _ ts v hp hv he with hnone | ⟨ρ', hm, he'⟩
      · have hw' : ∃ q ∈ cs.map Prod.fst, Matches ρ q v := by
          obtain ⟨q, hq, ρq, hρq⟩ := hw
          simp at hq
          rcases hq with rfl | ⟨b, hb⟩
          · rw [hnone] at hρq; cases hρq
          · exact ⟨q, List.mem_map.mpr ⟨(q, b), hb, rfl⟩, ρq, hρq⟩
        obtain ⟨body', ρ'', Γ'', h1, h2, h3⟩ := selectClause_typed hv he hcs hw'
        exact ⟨body', ρ'', Γ'', by simp [selectClause, hnone, h1], h2, h3⟩
      · exact ⟨body, ρ', _, by simp [selectClause, hm], he', hb⟩

end Sx
