# Hermes

Install [Hermes](https://hermes-agent.nousresearch.com) and add the stax skill so Hermes can drive stax workflows correctly.

## 1. Install

```bash
curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash
```

See the [Hermes docs](https://hermes-agent.nousresearch.com/docs) for other install methods.

## 2. Add the stax skill

The easiest path is to let stax manage it:

```bash
st setup --install-skills --skills hermes   # recommended
```

This writes the skill to `~/.hermes/skills/stax/SKILL.md` (or `$HERMES_HOME/skills/stax/SKILL.md` when `HERMES_HOME` is set). To do it manually:

```bash
mkdir -p "${HERMES_HOME:-$HOME/.hermes}/skills/stax"
st --skill > "${HERMES_HOME:-$HOME/.hermes}/skills/stax/SKILL.md"
# or fetch the markdown body only from GitHub:
curl -o "${HERMES_HOME:-$HOME/.hermes}/skills/stax/SKILL.md" https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

Hermes loads skills from `$HERMES_HOME/skills/<name>/SKILL.md`, which defaults to `~/.hermes/skills/<name>/SKILL.md`.

## 3. Use Hermes with AI create/PR generation

```bash
st create --ai -a --yes
st submit --ai
st generate --pr-body --agent hermes
st generate --pr-body --agent hermes --model anthropic/claude-sonnet-4.6
st gen --pr-title --agent hermes
st gen --commit-msg --agent hermes
```

stax invokes Hermes through `hermes -z/--oneshot`, which prints only the final
response text, so it works in the same pipes as the other agents. Hermes resolves
its own provider and model from `~/.hermes/config.yaml`; `--model` and `--provider`
override that for a single run. No `OPENAI_API_KEY` or other provider key is
required by stax — Hermes owns its credentials.

## 4. AI worktree lanes

```bash
st lane deep-dive --agent hermes
st lane deep-dive --agent hermes --model anthropic/claude-sonnet-4.6 "trace the flaky test"
```

`--yolo` maps to Hermes's own `--yolo` (bypass dangerous command approval prompts).

## Related

- [Claude Code](claude-code.md) · [Codex](codex.md) · [Gemini CLI](gemini-cli.md) · [OpenCode](opencode.md) · [pi](pi.md)
- [PR templates + AI](pr-templates-and-ai.md)
