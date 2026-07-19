#pragma once

// Unified's generated API umbrella includes the POSIX networking header for
// sockaddr_in. The Windows server uses the Winsock definition with the same
// layout, so the ABI probe supplies this include-only compatibility shim.
#include <winsock2.h>
