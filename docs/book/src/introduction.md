# Introduction

This book is the guided map for `nwnrs`.

The project is large enough now that filesystem order is not a useful way to learn it. This book walks the workspace in dependency order instead:

1. foundation primitives
2. resource identity and resolution
3. file and asset formats
4. language tooling
5. public interfaces

That order mirrors how the system is actually built.

`nwnrs` is also a reverse engineering project. Many of the crates exist to capture recovered semantics from Neverwinter Nights without smearing those semantics across unrelated layers. This guide is written with that in mind: what each crate knows, what it does not know, and which types matter first.

Useful entry points outside the book:

- [Workspace README](https://github.com/urothis/nwnrs/blob/main/README.md)
- [CLI README](https://github.com/urothis/nwnrs/blob/main/cli/README.md)
- [Crate tree on GitHub](https://github.com/urothis/nwnrs/tree/main/crates)

The first pass of this book is crate-oriented and type-guided. It is meant to give you the right mental model quickly. From there, each chapter links outward to the crate docs and source so the book can grow into a denser reference over time.
