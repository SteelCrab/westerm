<!--
Title: <type>(<scope>): <subject>, about 72 characters or less in all.
Types: feat, fix, docs, refactor, test, chore. Scope: the part the change touches.
Example: "feat(i18n): add a Japanese restore notice", not "Add Japanese restore notice" or "Update".
Rules: CONTRIBUTING.md ▸ Pull requests.
-->

## Summary

<!-- What changes, and why. -->

Closes #

## How I tested

<!-- Commands you ran, and what you checked by hand. For a UI change, add screenshots in the light and dark themes. -->

## Checklist

<!-- If an item does not apply, leave it unchecked and say why. -->

- [ ] `npm test`, `npm run type-check`, and `npm run build` pass.
- [ ] One topic. Unrelated changes are in other pull requests.
- [ ] New behavior has tests. A bug fix has a test that fails without the fix.
- [ ] UI text is in `src/i18n/messages.ts`, in all four languages.
- [ ] Docs match the change: `docs/` for behavior, the READMEs for run or develop steps.
- [ ] `CHANGELOG.md` has a line under `[Unreleased]` for a change that users notice.
- [ ] If PTY host code changes (`HOST_SOURCES` in `native/build.rs`), this pull request says so: users' shells restart once.
