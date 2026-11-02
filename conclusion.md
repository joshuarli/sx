My conclusion is that **Sx is not viable as a general-purpose code mode for AI agents.** Against JavaScript it costs more, its safety advantage didn't appear, and real use was slightly less correct. It has one real strength, explaining a program's calls before anything runs, but no measurement so far shows that strength preventing harm.

**What the evidence says, across all four campaigns:**

- **Correctness reached parity only with help.** On blind workflows Sx and TypeScript were level, but only after two campaigns of guide work. In real sessions Sx was behind: 27 of 32 correct, against 32 of 32.
- **Sx costs more, and tuning didn't fix it.** It was 2.4× JavaScript per solved task in the third campaign and 2.5–3× on the effects benchmark. The cost is the model's own reasoning: it plans longer in a language it has never seen, and a better guide didn't close that gap on unseen tasks.
- **The safety claim didn't hold up.** That claim is the reason Sx exists: checking a program's calls statically before it runs should prevent harm. On 96 blind sessions designed to tempt harmful effects, both arms were correct every time. The single policy violation was Sx's, and it came from a limit spread across several runs that a per-program check can't see. Today's models in an agent loop read first and act in small steps, so the careless program Sx would stop rarely gets written.
- **The real-session advantage isn't really Sx's.** Sx needed fewer turns there because pi's codemode makes JavaScript search for its tools. With the tools listed up front, as in Phase 16, JavaScript was cheaper. That's a configuration choice in pi, not a property of the language.

**What Sx does well that JavaScript can't:**
- A host can see every call a program will make, with each literal input judged against its grants, before anything runs. It can then approve or refuse the whole program at once.
- There is no authority a program doesn't get from the host, and fixed responses replay deterministically.
- A Lean reference semantics checks the implementation.

These are properties a host or an auditor cares about, not a model. They matter most where someone must approve exact effects in advance, such as regulated or high-stakes automation, or a person approving once per program instead of once per call. Nothing so far measures that setting.

**My recommendation:**

1. **Stop treating Sx as a language for agents to write.** That bet has been tested four ways, and the baseline keeps getting better.
2. **If the approval use case matters to you, run one decisive experiment.** Use blind workflows with data too large for the model to read before acting, host policies the prompt doesn't state, and a cost on every approval round-trip. Decide in advance that if Sx doesn't clearly win there, you stop. That's option 2 from Phase 16.
3. **Otherwise, keep the parts that work without the language:**
   - the MCP import and export with full type round-trips;
   - the fs scope rules;
   - the explain-and-approve model, possibly as a checker in front of JavaScript code mode.

   The last of these would be a new design, and it would be harder for JavaScript than for Sx, because JavaScript can't be statically analysed as precisely.

The honest summary is that Sx works as specified: it's correct, checked against a Lean reference semantics, and usable as a code mode for a real agent. What's missing is evidence that writing in it is better for models than writing JavaScript.