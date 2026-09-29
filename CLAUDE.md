# CLAUDE.md

Guidance for Claude Code in this repository.

## Git workflow

Unless the task says otherwise:

1. Start each change on a new local branch cut from the latest `origin/master`.
2. With the first commit, push the branch (`git push -u origin <branch>`) and open a pull request into `master`.
3. Keep pushing follow-up commits to that branch. Do not push to `master` directly, and do not force-push a branch someone else works on.
4. Once the pull request is merged, start the next change on a fresh branch from the updated `master`.

`.claude/settings.json` pre-approves the git commands this workflow needs and denies force-pushes.
