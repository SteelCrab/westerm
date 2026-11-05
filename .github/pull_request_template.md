<!--
Title: <type>(<scope>): <subject>, about 72 characters or less in all.
Types: feat, fix, docs, refactor, test, chore. Scope: the part the change touches.
Example: "feat(i18n): add a Japanese restore notice", not "Add Japanese restore notice" or "Update".
Rules: CONTRIBUTING.md ▸ Pull requests.
-->

## Summary

<!--
At most 5 bullets, one line each: what changes, and why.
When PTY host code changes (HOST_SOURCES in native/build.rs), say so: users' shells restart once.
Link an issue with "Closes #123" below the bullets.
-->

-

## How to test

<!-- Commands a reviewer can copy and run, then what to check by hand. -->

```bash
gh pr checkout <branch>
npm ci
npm test && npm run type-check && npm run build
```
