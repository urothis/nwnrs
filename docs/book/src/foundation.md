# Foundation

The foundation crates are the lowest-level reusable pieces in the workspace.

These crates are intentionally small. They should not know about NWN domain objects beyond the minimum needed to support higher layers. Their role is to provide stable generic behavior that would otherwise get duplicated across codecs and loaders.

The order to learn them is:

1. [`nwnrs-io`](./foundation-io.md)
2. [`nwnrs-encoding`](./foundation-encoding.md)
3. [`nwnrs-localization`](./foundation-localization.md)
4. [`nwnrs-checksums`](./foundation-checksums.md)
5. [`nwnrs-lru`](./foundation-lru.md)
6. [`nwnrs-streamext`](./foundation-streamext.md)

If you are trying to understand why a format crate is written the way it is, start here before diving into the format itself.
