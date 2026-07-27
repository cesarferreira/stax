# pi

Install [pi](https://pi.dev) and add the stax skill so pi can drive stax workflows correctly.

## 1. Install

See [pi.dev](https://pi.dev) for installation.

## 2. Add the stax skill

The easiest path is to let stax manage it:

```bash
stax skills update
```

This writes the skill to `~/.pi/agent/skills/stax/SKILL.md` (alongside the other agents). To do it manually:

```bash
mkdir -p ~/.pi/agent/skills/stax
curl -o ~/.pi/agent/skills/stax/SKILL.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

pi loads skills from `~/.pi/agent/skills/<name>/SKILL.md`.

## 3. Use pi with AI create/PR generation

```bash
st generate --pr-body --agent pi
st generate --pr-body --agent pi --model anthropic/claude-opus-4-8
st gen --pr-title --agent pi
st gen --commit-msg --agent pi
```

## 4. AI worktree lanes

```bash
st lane deep-dive --agent pi
st lane deep-dive --agent pi --model anthropic/claude-opus-4-8 "trace the flaky test"
```

`--yolo` is not supported for pi (permission bypass is provided by opt-in
permission-mode extensions, not a stable core CLI flag). Pass a bypass flag
manually with `--agent-arg` if your pi setup supports one.

## Related

- [Claude Code](claude-code.md) · [Codex](codex.md) · [Gemini CLI](gemini-cli.md) · [OpenCode](opencode.md)
- [PR templates + AI](pr-templates-and-ai.md)
