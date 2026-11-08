# AGENTS.md

## Working agreement: red/green TDD

We complete one issue at a time. Use the `skis` CLI for all issue operations. Never write to `.skis/issues.db` directly.

### The loop

1. Pick the next open issue in the current epic. Record its id and title. If no epic is active, use the lowest-numbered open epic.
2. Create and switch to branch `{id}-{slug-from-title}` from `main`. Slug: lowercase, alphanumeric words separated by hyphens.
3. RED phase: write the tests first. Append a description of the work to `docs/prs/{id}.md`: issue id/title, tests written, expected failures, and the intended behaviour. Tell the user it is ready for review. Stop and wait. Exception: straight configuration or setup work has no test stage; implement it directly, append notes to `docs/prs/{id}.md`, and ask for review.
4. The user writes `docs/reviews/{id}.md` and notifies you. Read it, address every concern, and update the RED work as needed.
5. GREEN phase: implement until the approved tests pass. Append implementation notes to `docs/prs/{id}.md`: what changed, commands run, and results. Tell the user it is ready for review.
6. The user either approves or updates the review document. Repeat from step 4 until the verdict is `approve`.
7. On `approve`: re-run the full checks. If `main` has moved, rebase onto it and re-run. Merge into `main` and delete the spent local branch.
8. Close the completed issue in `skis`. Pick the next open issue in the current epic. If the epic has no open issues, close the epic and continue with the lowest-numbered open epic. Stop only when no open epics remain.

Do not merge before an `approve` verdict. Do not close an issue before its branch is merged.

### Review artefacts

- `docs/prs/{id}.md` is append-only during the loop; add RED notes, review responses, and GREEN implementation notes.
- `docs/reviews/{id}.md` is written by the user. Treat it as the review contract.
- Always add every untracked review artifact to the change set; never leave `docs/reviews/{id}.md` untracked.

### Claims must match the committed tree

Before telling the user an issue is ready for review:

- `git status --short` must be empty, and every file named in `docs/prs/{id}.md` must appear in `git show --stat HEAD`.
- Grep the committed tree for every test, function, flag, and file the notes claim. If a name is absent, the claim is false: fix the code or remove the claim.
- After any scripted edit, read the changed hunk and `git diff` before writing notes. Do not infer success from the edit command.
- PR notes are evidence, not intentions. Anything not committed is marked deferred, not done.

### Definition of done

- All approved tests are green.
- Static checks are green, including rustfmt and clippy for the engine and tools.
- The full test suite has been run with no regressions.
- Code organisation and size constraints have been considered. Is the code as concise as possible while still understandable?
- Comments are sparse and concise, describe the actual invariant, and are never narrative.
- Function names contain no more than two underscores, ever.

### Engineering constraints

- You are the only worker. Never claim an issue is pre-existing or someone else's responsibility.
- Do not use hacks, loopholes, or reward hacking to meet the size budget. Write straight, professional Rust.
- The engine plus playable CLI has a hard ceiling of 999 LOC. The tools crate has a separate 650-line budget. See `docs/PLAN.md`.
- If the work cannot fit under the budget, stop and discuss algorithmic or scope reductions. Do not contort the code.
- Before claiming done, step back and self-review the implementation.
- Fitting the budget will be tight. Be disciplined.
