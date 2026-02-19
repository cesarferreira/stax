# Local Lessons

- When a user reports a command failure despite existing happy-path tests, prioritize reproducing edge conditions (like non-trunk parent ancestry) before assuming UX confusion.
- If a user reports `make test` failure, run the exact failing test first and then the exact aggregate command (`make test`) before considering the issue resolved.
- If docs mention setup steps, surface required auth setup directly in Quick Start with both supported paths (`stax auth` and `gh auth` import), not only in later sections.
- If a docs refactor trims or restructures `README.md`, keep the previous long-form content recoverable and be ready to restore it quickly with minimal additions (like a single docs link) when requested.
