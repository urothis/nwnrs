# Models and World Data

This family contains the richest object models in the workspace.

Dedicated chapters:

- [MDL Models](./formats-mdl.md)
- [GIT Area Instances](./formats-git.md)
- [SET Tilesets](./formats-set.md)

The common theme is that these formats are not just "containers of fields."
They encode graph structure, transforms, placement, composition rules, and
editor-authored catalogs. That means the main difficulty is usually not parsing
but choosing the right semantic layer without claiming more fidelity than the
source actually supports.
