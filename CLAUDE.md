# CLAUDE.md

Guidance for Claude Code in this repository.

## GitHub delivery

Applies to any task that touches GitHub, CI, pull requests, releases or
deploys. For anything else, ignore this section.

### Access and authority

- Assume GitHub is connected with read/write access to the account's
  repositories. Report an access problem only after a real permission error, a
  missing capability, or a step that needs account-owner privileges.
- Reversible actions (branches, commits, pull requests, comments) proceed
  without asking again.
- Ask first, every time, before: deploy, delete, transfer a repository, a
  destructive data operation, or anything irreversible outside the repository.

### One task, one pull request

- Make the smallest complete change that can be merged on its own.
- Split when parts have independent acceptance criteria, can be reviewed and
  merged separately, or when you find unrelated cleanup or a follow-up. Open
  that as its own pull request.
- Keep together only what would leave the repository invalid if split, or what
  is one atomic contract or migration.
- Never mix unrelated features, or a feature with unrelated cleanup. Never grow
  a pull request just to have fewer of them.
- Do not repeat work another pull request already did.

### Sequence

One delivery pull request is active per chain. Before starting the next:

1. Inspect the previous one and finish its requested scope if incomplete.
2. Merge it when it is ready, refresh the base branch, and branch from the
   latest base.

If the previous one is blocked, stop the dependent work and report the
blocker. Unrelated pull requests may run in parallel only when they are
independent and do not conflict; never merge one only to clear a queue.

### Flow

inspect state → resolve the previous pull request → branch from the latest
base → open a **draft** pull request → full CI → green → deploy validation (if
applicable) → mark ready for review → merge when ready → deploy by hand.

GitHub cannot merge a draft, and nobody can approve one. Once the head is green
and you have re-read your own diff, mark the pull request ready yourself. A
pull request that waits on a decision from the author, or that you could not
verify (say what was not checked), stays a draft.

Each stage has one job and does not redo another's:

- **Full CI** verifies correctness: format, lint, static analysis, tests,
  contract validation, and the build where correctness needs it. It is the
  authoritative gate and runs once per unchanged head. It never deploys.
- **Deploy validation** verifies deployability before merge, only where the
  repository deploys and that can be validated: the artifact or package, deploy
  configuration, environment contracts, migrations, preflight, readiness and
  pre-activation smoke contracts. Reuse CI results and artifacts. It does not
  rerun CI, tests, analysis or contract checks, rebuild an identical artifact,
  or activate a release.
- **Merge** is automatic and needs no user authorization once the requested
  task is complete, its scope is satisfied, CI is green, deploy validation is
  green or not applicable, and no known blocker invalidates it. Afterwards,
  record the merged state, refresh the base, and carry on with the sequence
  without waiting for confirmation. Merging is not deploying.
- **Deploy** is manual, comes after merge, and needs explicit user
  authorization every time. It consumes the verified artifact, makes the
  environment transition, activates the release, and checks minimal health. It
  does not rerun CI or validation, or rebuild an identical artifact.

Do not rerun full CI or deploy validation after merge. Repeat a check only when
the artifact changed, the relevant environment state changed, the platform
requires it, or the repository documents why. Keep a repeat narrow and
different from the earlier check.

### Shared-account co-authorship

Commit as the GitHub account that actually did the work, identified by its
account ID:

| GitHub account | Account ID | Git author (ID-based noreply)               |
| -------------- | ---------- | ------------------------------------------- |
| `0x0sky`       | 75973992   | `75973992+0x0sky@users.noreply.github.com`  |
| `x0skai`       | 300452669  | `300452669+x0skai@users.noreply.github.com` |

Tool credit is separate from account credit and applies only to real
contribution to the commit:

- `Assisted-by: OpenAI Codex` when Codex contributed.
- `Co-authored-by: Claude <noreply@anthropic.com>` when Claude contributed.

Rules:

- Add the other account as `Co-authored-by: <login> <<ID>+<login>@users.noreply.github.com>`
  only when it contributed code, tests, documentation or an incorporated patch
  to that exact commit. A review, approval, prompt or account ownership alone
  does not qualify.
- Solo authorship is normal. A commit with no real second contributor has no
  account co-author. Do not add one for an achievement or to satisfy a check.
- Never add the primary author as its own co-author, and never duplicate a
  trailer.
- Put trailers in the final paragraph of the commit message, one per line.
- Never label a commit as an unknown author to avoid this rule. If the connected
  GitHub API cannot author as the correct account, commit from a shell with the
  correct repository-local identity. If no such shell exists, state the
  limitation in the PR and do not claim the attribution.
- The commit-attribution check validates identities, trailer format, and
  duplicate or self credit. It cannot prove that a contribution happened; the
  author and reviewers establish that.
- Before merge, inspect the pushed commits: author account ID, trailers and
  co-author IDs. A comment, review or PR body is not a substitute.
- For squash merges, write the final message explicitly so genuine co-authors
  and tool credits survive. Do not assume GitHub preserves them.
- Do not manufacture commits, rewrite merged history, or change merge methods to
  create credit. Signature verification and achievement status are reported
  separately.

### Efficiency

- Read only the state the current decision needs: targeted files and metadata
  over whole-repository scans, pull request metadata before the full diff,
  CI logs only for failed or ambiguous checks. Do not re-read what you already
  verified in this task.
- Batch independent reads. Reuse identifiers, SHAs and results you already
  have. Do not poll a status that cannot have changed. Prefer one precise write
  over several incremental ones.
- Run a fast, targeted check locally when it can fail early, and leave the
  repository's full CI as the final gate. A green run for the exact same head
  is reused, not repeated; rerun only what changed inputs affect.

### Engineering

Inspect before modifying. Prefer verified state and contracts over assumptions,
the root cause over a symptom patch, the smallest sufficient change, the
repository's own conventions over generic preferences, and a complete change
over a partial one.

### Reporting

Say what changed, why, what was verified, which delivery stage it is at, any
remaining risk or blocker, and whether deploy or another protected action
needs authorization. Do not narrate every tool call, repeat logs without
interpreting them, claim success you did not verify, or ask for confirmation
the policy already grants.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
