#pragma once

#include <cstddef>
#include <cstdint>
#include <type_traits>

constexpr std::uint32_t NWNRS_ENGINE_SHIM_ABI_VERSION = 1;
constexpr std::uint32_t NWNRS_ENGINE_SHIM_STRING3_I32_V1 = 1;

struct NwnrsByteSpan {
    const std::uint8_t* data;
    std::size_t length;
};

struct NwnrsMutableByteSpan {
    std::uint8_t* data;
    std::size_t length;
};

struct NwnrsVector {
    float x;
    float y;
    float z;
};

static_assert(std::is_standard_layout_v<NwnrsByteSpan>);
static_assert(std::is_trivially_copyable_v<NwnrsByteSpan>);
static_assert(std::is_standard_layout_v<NwnrsMutableByteSpan>);
static_assert(std::is_trivially_copyable_v<NwnrsMutableByteSpan>);
static_assert(std::is_standard_layout_v<NwnrsVector>);
static_assert(std::is_trivially_copyable_v<NwnrsVector>);

extern "C" std::uint32_t nwnrs_engine_shim_abi_version();
extern "C" std::int32_t nwnrs_engine_shim_supports_signature(std::uint32_t signature);
extern "C" std::int32_t nwnrs_engine_shim_self_test_string3_i32();
