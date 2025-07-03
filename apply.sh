#!/usr/bin/env bash

# Temporary fix until the new evaluator works in streaming mode
colmena apply --legacy-flake-eval --evaluator streaming --impure
