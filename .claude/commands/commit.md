## Version Control
**IMPORTANT**: This project uses Jujutsu (jj) instead of Git. DO NOT use git commands.

```bash
jj status          # Show working copy changes
jj diff            # Show diff of changes
jj commit -m "feat(module): Add feature"  # Commit with Conventional Commits format
jj squash          # Squash into previous commit
jj log --limit 5   # Show recent commits
jj undo            # Undo last operation if mistake made
```

### Commit Message Format
Use Conventional Commits specification:
- `feat(scope):` New feature
- `fix(scope):` Bug fix
- `chore(scope):` Maintenance
- `refactor(scope):` Code restructuring
- `docs(scope):` Documentation
