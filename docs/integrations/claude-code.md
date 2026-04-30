# Claude Code

Install the stax skill file so Claude Code can drive stax workflows correctly.

```bash
mkdir -p ~/.claude/skills
curl -o ~/.claude/skills/stax.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

Enables workflow assistance for stacked branch creation, submit flows, and related operations.

## Use Claude with AI PR generation

```bash
st submit --ai
st generate --pr-body --agent claude
st generate --pr-body --agent claude --model claude-opus-4-5
```

## Related

- [Codex](codex.md) · [Gemini CLI](gemini-cli.md) · [OpenCode](opencode.md)
- [PR templates + AI](pr-templates-and-ai.md)
