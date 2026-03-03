# Working with Multiple Stacks

You can keep multiple independent stacks in the same repository.

```bash
# Stack A
st create auth
st create auth-login
st create auth-validation

# Stack B (hotfix)
st co main
st create hotfix-payment

# View all stacks
st ls
```

Example output:

```text
○    auth-validation 1↑
○    auth-login 1↑
○    auth 1↑
│ ◉  hotfix-payment 1↑
○─┘  main
```

This is useful when feature work is ongoing and an unrelated fix needs to ship immediately.
