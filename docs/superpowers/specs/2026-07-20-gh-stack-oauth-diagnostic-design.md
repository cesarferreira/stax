# gh-stack OAuth Diagnostic Design

## Problem

After PR #648, `stax doctor` correctly detects `github/gh-stack` when GitHub CLI is authenticated only through `GH_TOKEN` or `GITHUB_TOKEN`. Native GitHub Stack operations still cannot use those Personal Access Token overrides: stax removes them before invoking `gh stack`, and the private-preview API requires a keyring-backed OAuth login. In the env-only case, doctor therefore reports the extension as installed without warning that native stack operations have no usable credentials.

The authentication boundary also lacks direct regression coverage for `gh stack unstack`, extension installation, and extension upgrades.

## Desired Behavior

- Normal stax commands must perform no additional authentication probe.
- A normal `stax doctor` run with no `GH_TOKEN` or `GITHUB_TOKEN` must perform no additional authentication probe.
- When either token override is non-empty and `gh-stack` is installed, doctor checks whether a usable keyring-backed GitHub OAuth account exists.
- The OAuth check must remove both token overrides so it observes the credentials that `gh stack` will actually use.
- A missing or invalid OAuth account produces a non-blocking, actionable warning directing the user to `gh auth login` or `gh auth switch`.
- A successful OAuth check adds no extra output; the existing installed-extension diagnostic remains the success signal.
- A probe execution error is treated as unknown rather than incorrectly claiming that OAuth is missing.
- Extension discovery, installation, and upgrade continue to inherit token overrides.
- Remote `gh stack link` and `gh stack unstack` operations continue to remove token overrides.

## Design

Keep the credential boundary in `src/github/gh_stack.rs`:

1. Add a small predicate that detects a non-empty `GH_TOKEN` or `GITHUB_TOKEN` in the current process.
2. Add an OAuth-status helper that runs `gh auth status --active --hostname github.com` through `gh_stack_command`, which already removes both token overrides.
3. Represent the result as available, missing-or-invalid, or unknown. A successful exit means available, a completed non-zero exit means missing-or-invalid, and a process execution error means unknown.
4. In the existing `ExtensionStatus::Installed` branch of `stax doctor`, call the helper only when the environment predicate is true. Print the actionable warning only for missing-or-invalid; do nothing for available or unknown.

This confines the additional subprocess to `stax doctor` in the ambiguous env-token case. Submit, stack commands, and the common doctor path retain their current process count and latency.

## Error Handling

The warning should explain that the environment token can authenticate local extension discovery but cannot authenticate native Stack API operations. It should recommend `gh auth login` or `gh auth switch` without exposing command output, usernames, scopes, or tokens.

Doctor remains successful and does not add an automatic repair action because OAuth login is interactive and native stacks are optional.

## Regression Coverage

Add integration tests using the existing fake-`gh` fixture for:

- env-token doctor runs that have no usable OAuth login and receive the warning;
- env-token doctor runs whose token-stripped OAuth probe succeeds and receive no warning;
- doctor runs without token overrides that never invoke `gh auth status`, guarding the no-slowdown requirement;
- `gh stack unstack` receiving neither `GH_TOKEN` nor `GITHUB_TOKEN`;
- extension installation receiving both token overrides; and
- extension upgrade receiving both token overrides.

Follow the repository test policy with targeted `cargo nextest run gh_stack_tests::` feedback, then `make lint` and `make test` before publishing.

## Documentation

Update the native-stack sections in `README.md`, `docs/integrations/github-native-stacks.md`, and `skills.md` to describe the conditional doctor warning and its remediation.

## Non-Goals

- Probing OAuth on every stax command or every doctor run.
- Validating native Stack API access with a network mutation.
- Automatically running interactive GitHub authentication.
- Changing native-stack submit or link failure behavior.
- Supporting GitHub Enterprise native stacks, which are outside the current github.com private-preview integration.
