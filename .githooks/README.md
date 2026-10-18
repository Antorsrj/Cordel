# Repository pre-commit gate

Run `git config core.hooksPath .githooks` once in this checkout to activate the
tracked hook. It inspects staged hand-edited text files and rejects any over
20,000 bytes, the project's 5,000-token estimate. Lockfiles and generated
code are excluded. The error explains how to split and restage an oversized
file.
