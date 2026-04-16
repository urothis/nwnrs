# Resource Identity and Resolution

Once you move above the foundation layer, the next thing to understand is how `nwnrs` names and resolves game resources.

This layer matters because most higher-level workflows are not driven by files in isolation. They are driven by resource identities and lookup order across installs, user directories, archives, manifests, and overrides.

The key progression is:

1. [`nwnrs-restype`](./resources-restype.md) for resource type identity
2. [Built-In Resource Catalog](./resources-catalog.md) for the shipped registry surface
3. [`nwnrs-resref`](./resources-resref.md) for resource reference identity
4. [`nwnrs-resman`](./resources-resman.md) for lookup algebra
5. [`nwnrs-install`](./resources-install.md) for conventional install-backed assembly
6. [Concrete Resource Backends](./resources-backends.md) for actual containers
