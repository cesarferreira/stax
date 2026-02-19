# Working with Multiple Stacks

You can keep multiple independent stacks in the same repository.

```bash
# Stack A
stax create auth
stax create auth-login
stax create auth-validation

# Stack B (hotfix)
stax co main
stax create hotfix-payment

# View all stacks
stax ls
```

This is useful when feature work is ongoing and an unrelated fix needs to ship immediately.
