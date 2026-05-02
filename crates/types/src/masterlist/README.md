# nwnrs-types

Async client types for the Beamdog NWN masterlist API.

## Why This Crate Exists

Server-browser tooling needs typed access to the Beamdog masterlist without
pulling in application-level networking or caching concerns. This crate
provides minimal, schema-close types for the masterlist wire format so other
crates and tools can consume server listings without embedding JSON parsing
logic themselves.
