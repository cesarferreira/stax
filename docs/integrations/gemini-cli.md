# Gemini CLI Integration

Install Gemini CLI and add stax guidance as a project `GEMINI.md` file.

## 1) Install Gemini CLI

```bash
npm install -g @google/gemini-cli
```

Authenticate with `gemini` login flow or set `GEMINI_API_KEY` (see the Gemini CLI README).

## 2) Add stax instructions for this repo

```bash
curl -o GEMINI.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

Gemini CLI loads hierarchical instructions from `GEMINI.md`, so this gives it stax-aware workflow guidance.

## 3) Use Gemini with AI PR body generation

```bash
stax generate --pr-body --agent gemini
stax generate --pr-body --agent gemini --model gemini-2.5-flash
```

For Claude setup, see [Claude Code Integration](claude-code.md). For Codex setup, see [Codex Integration](codex.md).
