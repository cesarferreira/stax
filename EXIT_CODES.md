# Exit Codes

Stax uses standardized exit codes to enable better scripting and automation.

## Exit Code Reference

| Code | Type | Description | Example |
|------|------|-------------|---------|
| 0 | Success | Command completed successfully | `stax status` |
| 1 | General | General errors, file not found, invalid state | `stax init` in non-git repo |
| 2 | Conflict | Git conflicts during rebase/merge | `stax restack` hits conflicts |
| 3 | API Error | GitHub/GitLab API failures, network errors | `stax submit` when API is down |
| 4 | Validation | Invalid input or configuration | Invalid branch name format |
| 5 | Auth | Authentication/authorization failures | Missing or invalid API token |

## Usage in Scripts

```bash
#!/bin/bash

# Check for conflicts
stax restack
if [ $? -eq 2 ]; then
    echo "Conflicts detected, resolving..."
    stax resolve
fi

# Handle auth failures
stax submit
case $? in
    0)
        echo "Success!"
        ;;
    2)
        echo "Conflicts - please resolve"
        exit 2
        ;;
    3)
        echo "API error - check network"
        exit 3
        ;;
    5)
        echo "Auth error - run: stax auth"
        exit 5
        ;;
esac
```

## Implementation for Developers

To use specific exit codes in commands:

```rust
use crate::errors::{StaxError, StaxResult};

// Conflict error (exit code 2)
pub fn my_command() -> StaxResult<()> {
    if has_conflict {
        return Err(StaxError::conflict("Rebase stopped on conflict"));
    }
    Ok(())
}

// API error (exit code 3)
pub fn api_command() -> StaxResult<()> {
    client.call().await
        .map_err(|e| StaxError::api(e))?;
    Ok(())
}

// Validation error (exit code 4)
pub fn validate_input(name: &str) -> StaxResult<()> {
    if !is_valid(name) {
        return Err(StaxError::validation("Invalid branch name"));
    }
    Ok(())
}

// Auth error (exit code 5)
pub fn auth_command() -> StaxResult<()> {
    if !has_token() {
        return Err(StaxError::auth("No API token found. Run: stax auth"));
    }
    Ok(())
}
```

## Migration Status

The exit code infrastructure is in place and functional:

- ✅ Exit code constants defined
- ✅ StaxError enum with exit code mapping
- ✅ Main.rs handles StaxError and exits with appropriate codes
- ✅ Backward compatible with anyhow::Error (maps to exit code 1)
- ✅ ConflictStopped already returns exit code 2

Commands can be gradually migrated to use StaxError instead of anyhow::Error
for more specific exit codes. The system is backward compatible - existing
commands using anyhow::Error will continue to work with exit code 1.
