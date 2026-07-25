#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <new>

#include "abi_shim.hpp"

namespace {

struct CExoString {
    char* string;
    std::uint32_t length;
    std::uint32_t capacity;

    CExoString() noexcept : string(nullptr), length(0), capacity(0) {}

    CExoString(const char* value, std::size_t value_length)
        : string(nullptr), length(0), capacity(0) {
        if (value_length == 0) {
            return;
        }
        string = new char[value_length + 1];
        std::memcpy(string, value, value_length);
        string[value_length] = '\0';
        length = static_cast<std::uint32_t>(value_length);
        capacity = static_cast<std::uint32_t>(value_length + 1);
    }

    CExoString(const CExoString& other)
        : CExoString(other.string, other.length) {}

    ~CExoString() { delete[] string; }
};

using GetString = CExoString (*)(void*);
#if defined(_WIN32)
using GetStringWindows = CExoString* (*)(void*, CExoString*);
#endif
using FreeStringBuffer = void (*)(void*);
using GetLocString = std::int32_t (*)(const void*, std::int32_t, CExoString*, std::uint8_t);
using GetAliasPath = const CExoString& (*)(const void*, const CExoString&, std::int32_t);
using SetStringBool = std::int32_t (*)(void*, CExoString);
using SetStringVoid = void (*)(void*, CExoString);
using DisconnectPlayer = std::int32_t (*)(
    void*,
    std::uint32_t,
    std::uint32_t,
    std::int32_t,
    const CExoString&);
using String3I32 = std::int32_t (*)(
    void*,
    CExoString,
    CExoString,
    CExoString,
    std::int32_t);

} // namespace

namespace {

void release_engine_string(CExoString& value, void* free_address) {
    if (value.string != nullptr) {
        reinterpret_cast<FreeStringBuffer>(free_address)(value.string);
        value.string = nullptr;
        value.length = 0;
        value.capacity = 0;
    }
}

} // namespace

extern "C" std::size_t nwnrs_engine_get_string(
    void* address,
    void* free_address,
    void* object,
    char* output,
    std::size_t capacity) {
#if defined(_WIN32)
    CExoString value;
    const auto function = reinterpret_cast<GetStringWindows>(address);
    function(object, &value);
#else
    const auto function = reinterpret_cast<GetString>(address);
    CExoString value = function(object);
#endif
    if (value.length != 0 && output != nullptr && capacity >= value.length) {
        std::copy_n(value.string, value.length, output);
    }
    const auto length = value.length;
    release_engine_string(value, free_address);
    return length;
}

extern "C" std::size_t nwnrs_engine_get_loc_string(
    void* address,
    void* free_address,
    const void* object,
    char* output,
    std::size_t capacity) {
    const auto function = reinterpret_cast<GetLocString>(address);
    CExoString value;
    function(object, 0, &value, 0);
    if (value.length != 0 && output != nullptr && capacity >= value.length) {
        std::copy_n(value.string, value.length, output);
    }
    const auto length = value.length;
    release_engine_string(value, free_address);
    return length;
}

extern "C" std::size_t nwnrs_engine_get_alias_path(
    void* address,
    const void* object,
    const char* alias,
    std::size_t alias_length,
    char* output,
    std::size_t capacity) {
    const auto function = reinterpret_cast<GetAliasPath>(address);
    const CExoString& value = function(object, CExoString(alias, alias_length), 0);
    if (value.length != 0 && output != nullptr && capacity >= value.length) {
        std::copy_n(value.string, value.length, output);
    }
    return value.length;
}

extern "C" std::int32_t nwnrs_engine_disconnect_player(
    void* address,
    void* object,
    std::uint32_t player_id,
    std::uint32_t string_reference,
    std::int32_t cd_auth_failure,
    const char* reason,
    std::size_t reason_length) {
    const auto function = reinterpret_cast<DisconnectPlayer>(address);
    return function(
        object,
        player_id,
        string_reference,
        cd_auth_failure,
        CExoString(reason, reason_length));
}

extern "C" std::int32_t nwnrs_engine_set_string_bool(
    void* address,
    void* object,
    const char* value,
    std::size_t length) {
    const auto function = reinterpret_cast<SetStringBool>(address);
    return function(object, CExoString(value, length));
}

extern "C" void nwnrs_engine_set_string_void(
    void* address,
    void* object,
    const char* value,
    std::size_t length) {
    const auto function = reinterpret_cast<SetStringVoid>(address);
    function(object, CExoString(value, length));
}

extern "C" void nwnrs_engine_replace_string(
    void* destination,
    const char* value,
    std::size_t length) {
    auto* string = static_cast<CExoString*>(destination);
    CExoString replacement(value, length);
    std::swap(string->string, replacement.string);
    std::swap(string->length, replacement.length);
    std::swap(string->capacity, replacement.capacity);
}

extern "C" std::int32_t nwnrs_engine_call_string3_i32(
    void* address,
    void* object,
    NwnrsByteSpan first,
    NwnrsByteSpan second,
    NwnrsByteSpan third,
    std::int32_t tail) {
    const auto function = reinterpret_cast<String3I32>(address);
    return function(
        object,
        CExoString(
            reinterpret_cast<const char*>(first.data),
            first.length),
        CExoString(
            reinterpret_cast<const char*>(second.data),
            second.length),
        CExoString(
            reinterpret_cast<const char*>(third.data),
        third.length),
        tail);
}

namespace {

std::int32_t string3_i32_fixture(
    void* object,
    CExoString first,
    CExoString second,
    CExoString third,
    std::int32_t tail) {
    const auto seed = static_cast<std::int32_t>(
        reinterpret_cast<std::uintptr_t>(object));
    return seed
        + static_cast<std::int32_t>(first.length)
        + static_cast<std::int32_t>(second.length * 10)
        + static_cast<std::int32_t>(third.length * 100)
        + tail;
}

} // namespace

extern "C" std::int32_t nwnrs_engine_shim_self_test_string3_i32() {
    const std::uint8_t first[] = {'a'};
    const std::uint8_t second[] = {'b', 'c'};
    const std::uint8_t third[] = {'d', 'e', 'f'};
    return nwnrs_engine_call_string3_i32(
        reinterpret_cast<void*>(&string3_i32_fixture),
        reinterpret_cast<void*>(4),
        NwnrsByteSpan{first, sizeof(first)},
        NwnrsByteSpan{second, sizeof(second)},
        NwnrsByteSpan{third, sizeof(third)},
        5);
}
