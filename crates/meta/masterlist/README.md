# nwnrs-masterlist

Async client types for the Beamdog NWN masterlist API.

## Scope

- model the JSON payloads returned by the public masterlist service
- provide thin fetch helpers over that wire format
- keep the public types close to the service schema

## Non-goals

- define a higher-level server-browser abstraction independent of the Beamdog
  API
- replace application-level networking or caching policy
