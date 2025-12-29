# Jujutsu (jj) Version Control System Guide

## Table of Contents
1. [Overview](#overview)
2. [Key Concepts](#key-concepts)
3. [Installation Instructions](#installation-instructions)
4. [Basic Workflow and Commands](#basic-workflow-and-commands)
5. [Comparison with Git](#comparison-with-git)
6. [Advanced Features](#advanced-features)
7. [Configuration Options](#configuration-options)
8. [Git Repository Integration](#git-repository-integration)
9. [Best Practices and Tips](#best-practices-and-tips)

## Overview

Jujutsu (jj) is a powerful, Git-compatible version control system designed to address common Git pain points while maintaining full compatibility with Git repositories. It provides a cleaner mental model, automatic rebasing, and a more intuitive command-line interface.

### Why Jujutsu?

- **Simpler Mental Model**: Your working copy is always a commit
- **No Staging Area**: Direct manipulation of commits without an intermediate index
- **Automatic Rebasing**: Descendant commits automatically rebase when you rewrite history
- **Anonymous Branches**: No need to name every small change
- **Powerful History Editing**: First-class support for rewriting and reorganizing commits
- **Git Compatible**: Works seamlessly with existing Git repositories and workflows

## Key Concepts

### Working Copy as a Commit
In Jujutsu, your working directory state is always represented as a commit. This eliminates the distinction between "uncommitted changes" and "commits", simplifying many operations.

```bash
# In Git, you have uncommitted changes
# In jj, your working copy IS a commit that gets automatically amended
```

### Change IDs vs Commit IDs
- **Change ID**: A stable identifier that persists even when a commit is rebased or amended
- **Commit ID**: The Git-compatible SHA hash that changes when content changes

```bash
# View both IDs in the log
jj log --limit 3
```

### Revisions
A revision in Jujutsu can refer to:
- A commit (using change ID or commit ID)
- The working copy commit (`@`)
- Relative references (`@-` for parent, `@+` for children)

### Operations and the Operation Log
Every command that modifies the repository creates an "operation". The operation log tracks all changes to the repository state, enabling powerful undo capabilities.

```bash
# View recent operations
jj op log

# Undo the last operation
jj undo
```

### Bookmarks (Branches)
Jujutsu uses "bookmarks" instead of branches. Key differences:
- No "current branch" concept
- Bookmarks don't automatically move
- Anonymous development is encouraged

## Installation Instructions

### Prerequisites
- Rust version 1.84 or newer (for building from source)
- Git (for Git repository integration)

### Platform-Specific Installation

#### Linux
```bash
# Arch Linux
sudo pacman -S jujutsu

# NixOS/Nix
nix run 'github:jj-vcs/jj'

# Homebrew
brew install jj

# From source
cargo install --locked --bin jj jj-cli

# Binary installation with cargo-binstall
cargo binstall --strategies crate-meta-data jj-cli
```

#### macOS
```bash
# Homebrew (recommended)
brew install jj

# MacPorts
sudo port install jujutsu

# From source (requires Xcode command line tools)
xcode-select --install
cargo install --locked --bin jj jj-cli
```

#### Windows
```bash
# Winget
winget install jj-vcs.jj

# From source
cargo install --locked --bin jj jj-cli
```

### Initial Configuration

```bash
# Set your name and email
jj config set --user user.name "Your Name"
jj config set --user user.email "your.email@example.com"

# Optional: Set your preferred editor
jj config set --user ui.editor "vim"  # or "code", "emacs", etc.

# Optional: Set up command completion (example for bash)
jj util completion bash >> ~/.bashrc
source ~/.bashrc
```

## Basic Workflow and Commands

### Creating and Cloning Repositories

```bash
# Create a new Git-backed repository
jj init --git my-project
cd my-project

# Clone an existing Git repository
jj git clone https://github.com/user/repo.git
cd repo

# Create a colocated repository (can use both jj and git commands)
jj git clone --colocate https://github.com/user/repo.git
```

### Essential Commands

#### Viewing Repository State
```bash
# Show working copy status
jj st

# View commit history
jj log

# View detailed log with file changes
jj log --summary

# Show changes in working copy
jj diff

# Show changes in a specific revision
jj diff -r <change-id>
```

#### Making Changes
```bash
# Start a new change (commit)
jj new

# Start a new change with a description
jj new -m "Add feature X"

# Describe the current change
jj describe -m "Implement user authentication"

# Edit the description interactively
jj describe
```

#### Navigating History
```bash
# Move to a different commit
jj edit <change-id>

# Create a new commit on top of a specific commit
jj new <change-id>

# Create a merge commit
jj new <parent1> <parent2>
```

#### Organizing Changes
```bash
# Move changes from working copy to parent commit
jj squash

# Move specific files to parent commit
jj squash --interactive

# Split a commit into multiple commits
jj split

# Rebase current commit onto a different parent
jj rebase -d <destination>

# Rebase a range of commits
jj rebase -r <revset> -d <destination>
```

#### Working with Remotes
```bash
# Fetch updates from remote
jj git fetch

# Push current bookmark to remote
jj git push

# Push all bookmarks
jj git push --all

# Create a bookmark and push it
jj bookmark create my-feature
jj git push --bookmark my-feature
```

### Example Workflow

```bash
# 1. Clone a repository
jj git clone https://github.com/example/project.git
cd project

# 2. Create a new feature
jj new main -m "Start feature implementation"

# 3. Make changes
echo "New feature" > feature.txt

# 4. View your changes
jj diff
jj st

# 5. Split work into logical commits
jj new -m "Add feature documentation"
echo "Feature docs" > docs.md

# 6. Review history
jj log --limit 5

# 7. Push to remote
jj bookmark create my-feature
jj git push --bookmark my-feature
```

## Comparison with Git

### Conceptual Differences

| Concept | Git | Jujutsu |
|---------|-----|----------|
| Working Directory | Separate from commits | Always a commit |
| Staging Area | Required for commits | Not needed |
| Branches | Current branch concept | Bookmarks (no current branch) |
| History Editing | Complex (interactive rebase) | Natural and simple |
| Merge Conflicts | Block operations | Can be committed |
| Change Identity | SHA changes on rebase | Stable change IDs |

### Command Equivalents

| Task | Git | Jujutsu |
|------|-----|----------|
| Initialize repo | `git init` | `jj init --git` |
| Clone repo | `git clone <url>` | `jj git clone <url>` |
| Status | `git status` | `jj st` |
| View diff | `git diff` | `jj diff` |
| Stage changes | `git add <file>` | Not needed |
| Commit | `git commit -m "msg"` | `jj new -m "msg"` |
| Amend commit | `git commit --amend` | Just edit files (auto-amends) |
| Create branch | `git branch <name>` | `jj bookmark create <name>` |
| Switch branch | `git checkout <branch>` | `jj edit <bookmark>` |
| Merge | `git merge <branch>` | `jj new <rev1> <rev2>` |
| Rebase | `git rebase <target>` | `jj rebase -d <target>` |
| Interactive rebase | `git rebase -i` | `jj rebase`, `jj squash -i` |
| Cherry-pick | `git cherry-pick <commit>` | `jj new <commit> -m "msg"` |
| Reset hard | `git reset --hard` | `jj abandon` |
| Stash | `git stash` | `jj new` (create new change) |
| View log | `git log` | `jj log` |
| Undo | `git reflog` + `git reset` | `jj undo` |

### Workflow Differences

#### Git Workflow
```bash
git checkout -b feature
# Make changes
git add file.txt
git commit -m "Add feature"
git push -u origin feature
```

#### Jujutsu Workflow
```bash
jj new main -m "Add feature"
# Make changes (automatically tracked)
jj bookmark create feature
jj git push --bookmark feature
```

## Advanced Features

### Revsets - Powerful Commit Selection

Revsets are Jujutsu's query language for selecting commits:

```bash
# Basic revset examples
jj log -r @              # Current commit
jj log -r @-             # Parent of current commit
jj log -r @--            # Grandparent
jj log -r main           # Bookmark named 'main'
jj log -r xyz            # Change ID starting with 'xyz'

# Operators
jj log -r "@ | @-"       # Current commit OR its parent
jj log -r "@ & mine()"   # Current commit AND authored by me
jj log -r "main::@"      # All commits from main to current
jj log -r "::@"          # All ancestors of current
jj log -r "@::"          # All descendants of current
jj log -r "main..@"      # Commits in @ but not in main

# Functions
jj log -r 'author(alice)'              # Commits by alice
jj log -r 'description("fix bug")'     # Commits with "fix bug" in message
jj log -r 'mine() & empty()'           # My empty commits
jj log -r 'bookmarks()'                # All bookmarked commits

# Complex queries
jj log -r 'ancestors(feature) & author(me) & description("bug")'
jj log -r 'main..@ & modified(src/)'   # Changes to src/ since main
```

### Automatic Rebasing

When you modify a commit, all descendant commits automatically rebase:

```bash
# Edit an earlier commit
jj edit ABC123

# Make changes
echo "fix" > file.txt

# All descendant commits are automatically rebased!
jj log  # See the updated history
```

### Conflict Resolution

Jujutsu can commit conflicts, allowing you to resolve them later:

```bash
# Create a conflicting merge
jj new branch1 branch2

# The merge is created even with conflicts
jj st  # Shows conflicted files

# Resolve conflicts
jj resolve file.txt  # Interactive resolution
# or edit manually and then:
jj resolve --mark file.txt

# Continue working even with unresolved conflicts
jj new -m "Work on something else"
```

### Operation Log and Undo

The operation log tracks all repository modifications:

```bash
# View operation history
jj op log

# Undo last operation
jj undo

# Restore to a specific operation
jj op restore <operation-id>

# See what changed in an operation
jj op show <operation-id>
```

### Anonymous Branches

Work without creating named branches:

```bash
# Just start working
jj new main

# Make changes...

# Later, if you want to share:
jj bookmark create my-feature
jj git push --bookmark my-feature

# Or just push without a bookmark name
jj git push --change @
```

### Working Copy Management

```bash
# Abandon current changes and start fresh
jj abandon

# Create a backup of current state before risky operation
jj new  # Creates a new commit, preserving current state

# Work on multiple things simultaneously
jj new main -m "Feature A"
# work on feature A...
jj new main -m "Feature B"  
# work on feature B...
# Switch between them:
jj edit <change-id-A>
```

## Configuration Options

### Configuration Levels

```bash
# User configuration (global)
jj config set --user <key> <value>

# Repository configuration
jj config set --repo <key> <value>

# View configuration
jj config list
jj config get <key>
```

### Common Configuration Options

```bash
# User identity
jj config set --user user.name "Your Name"
jj config set --user user.email "email@example.com"

# UI preferences
jj config set --user ui.editor "code --wait"
jj config set --user ui.diff-editor "meld"
jj config set --user ui.default-command "log"
jj config set --user ui.color "auto"  # auto, always, never

# Behavior
jj config set --user revsets.short-prefixes.min-length 4
jj config set --user merge.tool "meld"

# Aliases
jj config set --user alias.ci "commit"
jj config set --user alias.d "diff"
jj config set --user alias.l "log --summary"
```

### Template Configuration

Customize output formatting:

```bash
# Custom log format
jj config set --user 'template.commit_summary' '
  commit_id.short() ++ " " ++ 
  change_id.short() ++ " " ++ 
  author.name() ++ " " ++
  format_timestamp(author.timestamp()) ++ "\n" ++
  description.first_line()
'
```

## Git Repository Integration

### Working with Git Repositories

Jujutsu provides excellent Git compatibility through several modes:

#### Standard Git Backend
```bash
# Clone a Git repo
jj git clone https://github.com/user/repo.git

# Work normally with jj commands
jj new main -m "My feature"

# Push to Git remote
jj git push
```

#### Colocated Repositories
A colocated repository allows using both `jj` and `git` commands:

```bash
# Create colocated repo
jj git clone --colocate https://github.com/user/repo.git

# Use jj commands
jj new -m "Feature"

# Also use git commands if needed
git status  # See Git's view
```

### Git Integration Features

#### Fetch and Push
```bash
# Fetch from specific remote
jj git fetch --remote origin

# Fetch all remotes
jj git fetch --all

# Push specific bookmark
jj git push --bookmark my-feature

# Push all bookmarks
jj git push --all

# Push a specific commit without bookmark
jj git push --change @
```

#### Import/Export
```bash
# Import Git refs to jj
jj git import

# Export jj bookmarks to Git refs
jj git export
```

### Limitations with Git

Current limitations when using Git backend:
- No Git hooks support
- No submodules support
- No Git LFS support
- Limited .gitattributes support
- Partial shallow clone support

## Best Practices and Tips

### 1. Embrace the Working Copy as a Commit
- Don't worry about "committing too often" - your working copy is always committed
- Use `jj new` liberally to checkpoint your work

### 2. Use Descriptive Change Descriptions
```bash
# Good
jj describe -m "Add user authentication with JWT tokens"

# Better - use longer descriptions
jj describe  # Opens editor for detailed message
```

### 3. Leverage Anonymous Branches
- Start working immediately without naming branches
- Only create bookmarks when you need to share or reference work

### 4. Master Revsets for Powerful Workflows
```bash
# Find recent work
jj log -r 'mine() & recent(days=7)'

# Review changes before pushing
jj log -r 'main..@ & mine()'

# Find empty commits to clean up
jj log -r 'empty() & mine()'
```

### 5. Use Operations for Safety
```bash
# Before risky operations
jj op log  # Note current operation

# If something goes wrong
jj undo  # or jj op restore <id>
```

### 6. Organize Work with Strategic Commits
```bash
# Create logical commit boundaries
jj new -m "Refactor: Extract authentication module"
# ... make refactoring changes ...

jj new -m "Feature: Add OAuth support"
# ... add new feature ...

jj new -m "Tests: Add OAuth integration tests"
# ... add tests ...
```

### 7. Efficient Conflict Resolution
```bash
# Don't fear conflicts - they won't block you
jj new conflicted-merge -m "Continue other work"

# Resolve when convenient
jj edit conflicted-merge
jj resolve --list
jj resolve path/to/file
```

### 8. Interactive Development
```bash
# Use split and squash for history editing
jj split            # Split current commit
jj squash -i        # Interactive squash
jj move -i          # Move changes between commits
```

### 9. Helpful Aliases
```bash
# Add to your config
jj config set --user alias.w "log -r '@ | @-' --summary"  # What's here
jj config set --user alias.sm "squash -m"                 # Squash with message
jj config set --user alias.n "new"                         # Shorter new
```

### 10. Integration Tips
- Use colocated repos when transitioning teams to jj
- Keep Git workflows for CI/CD while using jj locally
- Regularly fetch to stay synchronized with team

## Troubleshooting

### Common Issues and Solutions

#### "Concurrent operations detected"
```bash
# Another jj process is running or crashed
jj op log  # Check recent operations
# If safe, remove lock file: .jj/op_lock
```

#### Working with Large Repositories
```bash
# Use git shallow clone first
git clone --depth=1 <url>
cd repo
jj git init --colocate
```

#### Bookmark Divergence
```bash
# When local and remote bookmarks diverge
jj bookmark list  # Check status
jj git fetch
jj rebase -r <local> -d <remote>
```

## Resources

- **Official Documentation**: https://jj-vcs.github.io/jj/latest/
- **GitHub Repository**: https://github.com/jj-vcs/jj
- **Discord Community**: Linked from GitHub
- **Tutorial**: https://jj-vcs.github.io/jj/latest/tutorial/

## Summary

Jujutsu represents a significant evolution in version control, maintaining Git compatibility while fixing many of its pain points. Its key innovations—working copy as commit, automatic rebasing, and powerful history editing—create a more intuitive and efficient development workflow. Whether you're a Git power user looking for better tools or someone frustrated with Git's complexity, Jujutsu offers a compelling alternative that doesn't force you to abandon the Git ecosystem.