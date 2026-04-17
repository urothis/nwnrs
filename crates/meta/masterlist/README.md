# nwnrs-masterlist

Async client types for the Beamdog NWN masterlist API.

## Why This Crate Exists

Server-browser tooling needs typed access to the Beamdog masterlist without
pulling in application-level networking or caching concerns. This crate
provides minimal, schema-close types for the masterlist wire format so other
crates and tools can consume server listings without embedding JSON parsing
logic themselves.

## Scope

- model the JSON payloads returned by the public masterlist service
- provide thin fetch helpers over that wire format
- keep the public types close to the service schema

## Non-goals

- define a higher-level server-browser abstraction independent of the Beamdog
  API
- replace application-level networking or caching policy
