#include "abi_shim.hpp"

extern "C" std::uint32_t nwnrs_engine_shim_abi_version() {
    return NWNRS_ENGINE_SHIM_ABI_VERSION;
}

extern "C" std::int32_t nwnrs_engine_shim_supports_signature(
    const std::uint32_t signature) {
    switch (signature) {
        case NWNRS_ENGINE_SHIM_STRING3_I32_V1:
            return 1;
        default:
            return 0;
    }
}
