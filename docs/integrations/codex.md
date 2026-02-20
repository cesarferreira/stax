# Codex Integration

Install the stax skill file so Codex can operate stax workflows correctly.

```bash
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills/stax"
curl -o "${CODEX_HOME:-$HOME/.codex}/skills/stax/SKILL.md" https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

This enables Codex to help with stacked branch creation, submit flows, and related operations.

For Claude Code setup, see [Claude Code Integration](claude-code.md). For Gemini setup, see [Gemini CLI Integration](gemini-cli.md).
