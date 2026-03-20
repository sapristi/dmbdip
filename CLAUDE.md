# First 

read README.md and docs/DEVELOPMENT.md

## Implementation

- When a task is done:
  - check if any documentation needs updating
  - Commit the changes
- Clean git history if appropriate (squash fixup commits)
- Evaluate quality every few commits and after fixing bugs
- Don't include Co-authored by in the commit message.

## Release

1. Run `git fetch` to pull remote tags
2. Go to GitHub Actions → "Prepare Release" → Run workflow
3. Enter the new version (semver, e.g. `0.5.0`)

This automatically bumps `Cargo.toml`, commits, tags, and triggers the release workflow which builds binaries, creates a GitHub release, and publishes to crates.io.
