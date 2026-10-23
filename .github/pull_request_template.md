## What and why

<!-- What changed, why, and any decision worth knowing. Link the Tandem task. -->

## Checklist

- [ ] Tests cover the change, and the README and docs are updated if behaviour changed
- [ ] Commits for a Tandem task carry its `Task: <id>` trailer

<!-- For a version/vX.Y.Z pull request into main; delete otherwise. -->

## Release

- [ ] The version is set with `cargo xtask version --next` or `--exact`, and `cargo xtask version check pr` passes
- [ ] Every feature branch for this release is merged into the version branch
- [ ] The feature pull requests have clear titles and an `enhancement`, `bug` or `documentation` label; they become the release notes
- [ ] This pull request is labelled `skip-release-notes`, and will be merged with a merge commit, not a squash
