#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <new>

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

} // namespace

extern "C" std::size_t nwnrs_engine_get_string(
    void* address,
    void* object,
    char* output,
    std::size_t capacity) {
    const auto function = reinterpret_cast<GetString>(address);
    const CExoString value = function(object);
    if (value.length != 0 && output != nullptr && capacity >= value.length) {
        std::copy_n(value.string, value.length, output);
    }
    return value.length;
}

extern "C" std::size_t nwnrs_engine_get_loc_string(
    void* address,
    const void* object,
    char* output,
    std::size_t capacity) {
    const auto function = reinterpret_cast<GetLocString>(address);
    CExoString value;
    function(object, 0, &value, 0);
    if (value.length != 0 && output != nullptr && capacity >= value.length) {
        std::copy_n(value.string, value.length, output);
    }
    return value.length;
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
