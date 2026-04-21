# Multiple stacks

You can keep independent stacks in the same repository. Useful when feature work is in flight and an unrelated fix needs to ship immediately.

```bash
# Stack A: feature work
st create auth
st create auth-login
st create auth-validation

# Stack B: hotfix from trunk
st co main
st create hotfix-payment

# See both
st ls
```

Output:

```text
○    auth-validation 1↑
○    auth-login      1↑
○    auth            1↑
│ ◉  hotfix-payment  1↑
○─┘  main
```

Each stack is restacked, synced, and merged independently. `st run --stack` and `st run --stack=<branch>` scope commands to one stack at a time.

## Related

- [Stacked branches](stacked-branches.md)
- [Navigation](../commands/navigation.md)
