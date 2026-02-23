# OpenCode Integration

Install OpenCode and add the stax skill so OpenCode can operate stax workflows correctly.

## 1) Install OpenCode

```bash
curl -fsSL https://opencode.ai/install | bash
```

## 2) Add stax skill instructions

```bash
mkdir -p ~/.config/opencode/skills/stax
curl -o ~/.config/opencode/skills/stax/SKILL.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

OpenCode loads skills from `~/.config/opencode/skills/<name>/SKILL.md`, so this gives it stax-aware workflow guidance.

## 3) Use OpenCode with AI PR body generation

```bash
stax generate --pr-body --agent opencode
stax generate --pr-body --agent opencode --model opencode/gpt-5.1-codex
```

For Claude setup, see [Claude Code Integration](claude-code.md). For Codex setup, see [Codex Integration](codex.md). For Gemini setup, see [Gemini CLI Integration](gemini-cli.md).
