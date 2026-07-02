# Gemini CLI

Install Gemini CLI and add stax guidance as a project `GEMINI.md` file.

## 1. Install

```bash
npm install -g @google/gemini-cli
```

Authenticate via the `gemini` login flow or set `GEMINI_API_KEY` (see the Gemini CLI README).

## 2. Add stax instructions to the repo

```bash
curl -o GEMINI.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

Gemini CLI loads hierarchical instructions from `GEMINI.md`, which gives it stax-aware workflow guidance.

## 3. Use Gemini with AI create/PR generation

```bash
st create --ai -a --yes
st submit --ai
st generate --pr-body --agent gemini
st generate --pr-body --agent gemini --model gemini-2.5-flash
st gen --pr-title --agent gemini
st gen --commit-msg --agent gemini
```

## Related

- [Claude Code](claude-code.md) · [Codex](codex.md) · [OpenCode](opencode.md)
- [PR templates + AI](pr-templates-and-ai.md)
