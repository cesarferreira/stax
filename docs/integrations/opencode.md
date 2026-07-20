# OpenCode

Install OpenCode and add the stax skill so OpenCode can drive stax workflows correctly.

## 1. Install

```bash
curl -fsSL https://opencode.ai/install | bash
```

## 2. Add the stax skill

```bash
mkdir -p ~/.config/opencode/skills/stax
curl -o ~/.config/opencode/skills/stax/SKILL.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

OpenCode loads skills from `~/.config/opencode/skills/<name>/SKILL.md`.

## 3. Use OpenCode with AI create/PR generation

```bash
st create --ai -a --yes
st submit --ai
st generate --pr-body --agent opencode
st generate --pr-body --agent opencode --model opencode/gpt-5.5-fast
st gen --pr-title --agent opencode
st gen --commit-msg --agent opencode
```

If OpenCode exposes a model that is not listed in stax's picker, choose
`Edit config file to use another model` from the model menu and set the model
manually, for example:

```toml
[ai.generate]
agent = "opencode"
model = "opencode/<model-id>"
```

## Related

- [Claude Code](claude-code.md) · [Codex](codex.md) · [Gemini CLI](gemini-cli.md)
- [PR templates + AI](pr-templates-and-ai.md)
