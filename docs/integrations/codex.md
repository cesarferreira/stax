# Codex

Install the stax skill file so Codex can drive stax workflows correctly.

```bash
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills/stax"
curl -o "${CODEX_HOME:-$HOME/.codex}/skills/stax/SKILL.md" https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

Enables workflow assistance for stacked branch creation, submit flows, and related operations.

## Use Codex with AI create/PR generation

```bash
st create --ai -a --yes
st submit --ai
st generate --pr-body --agent codex
st generate --pr-body --agent codex --model gpt-5.3-codex
```

When `codex` is selected, stax tries OpenAI's live Models API first (using `OPENAI_API_KEY`) before falling back to its local Codex defaults.

## Related

- [Claude Code](claude-code.md) · [Gemini CLI](gemini-cli.md) · [OpenCode](opencode.md)
- [PR templates + AI](pr-templates-and-ai.md)
