# Local Lessons

- When a user reports a command failure despite existing happy-path tests, prioritize reproducing edge conditions (like non-trunk parent ancestry) before assuming UX confusion.
- If a user reports `make test` failure, run the exact failing test first and then the exact aggregate command (`make test`) before considering the issue resolved.
