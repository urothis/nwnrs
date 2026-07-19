#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>

struct CExoString {
    char* string = nullptr;
    std::uint32_t length = 0;
    std::uint32_t capacity = 0;

    CExoString() = default;

    CExoString(const char* value, std::size_t value_length) {
        if (value_length == 0) {
            return;
        }
        string = new char[value_length + 1];
        std::memcpy(string, value, value_length);
        string[value_length] = '\0';
        length = static_cast<std::uint32_t>(value_length);
        capacity = static_cast<std::uint32_t>(value_length + 1);
    }

    CExoString(const CExoString& other) : CExoString(other.string, other.length) {}

    CExoString& operator=(CExoString other) noexcept {
        std::swap(string, other.string);
        std::swap(length, other.length);
        std::swap(capacity, other.capacity);
        return *this;
    }

    ~CExoString() { delete[] string; }
};

struct FixtureNetLayer {
    std::uint32_t max_players;
    std::uint32_t udp_port;
    CExoString session_name;
    CExoString player_password;
    CExoString dm_password;
};

struct FixtureLocString {
    CExoString value;
};

struct FixtureTurd {
    CExoString community_name;
    FixtureLocString first_name;
    FixtureLocString last_name;
};

struct FixtureLinkedListNode {
    void* previous;
    FixtureLinkedListNode* next;
    void* object;
};

struct FixtureLinkedListInternal {
    FixtureLinkedListNode* head;
    FixtureLinkedListNode* tail;
    std::uint32_t count;
    std::uint32_t padding;
};

struct FixtureModule {
    FixtureLinkedListInternal* turds;
};

struct FixturePlayer {
    std::uint32_t player_id;
    char file_name[17];
};

struct FixturePlayerInfo {
    CExoString public_cd_key;
};

struct FixtureCreatureStats {
    FixtureLocString first_name;
    FixtureLocString last_name;
};

struct FixtureCreature {
    FixtureCreatureStats* stats;
};

struct FixtureAliasList {
    CExoString server_vault;
};

struct FixtureExoBase {
    FixtureAliasList* alias_list;
};

static_assert(offsetof(FixtureTurd, community_name) == 0);
static_assert(offsetof(FixtureTurd, first_name) == 16);
static_assert(offsetof(FixtureTurd, last_name) == 32);
static_assert(offsetof(FixtureLinkedListInternal, head) == 0);
static_assert(offsetof(FixtureLinkedListInternal, count) == 16);
static_assert(offsetof(FixtureLinkedListNode, next) == 8);
static_assert(offsetof(FixtureLinkedListNode, object) == 16);
static_assert(offsetof(FixturePlayer, player_id) == 0);
static_assert(offsetof(FixturePlayer, file_name) == 4);
static_assert(sizeof(FixturePlayer::file_name) == 17);
static_assert(offsetof(FixtureCreature, stats) == 0);
static_assert(offsetof(FixtureCreatureStats, first_name) == 0);
static_assert(offsetof(FixtureCreatureStats, last_name) == 16);
static_assert(offsetof(FixtureExoBase, alias_list) == 0);

FixtureTurd fixture_turd;
FixtureLinkedListNode fixture_turd_node{};
FixtureLinkedListInternal fixture_turd_list{};
FixtureModule fixture_module{&fixture_turd_list};
FixturePlayer fixture_player{};
FixturePlayerInfo fixture_player_info;
FixtureCreatureStats fixture_creature_stats;
FixtureCreature fixture_creature{&fixture_creature_stats};
FixtureAliasList fixture_alias_list;
FixtureExoBase fixture_exo_base{&fixture_alias_list};

void reset_fixture_turd() {
    fixture_turd.community_name = CExoString("fixture-player", 14);
    fixture_turd.first_name.value = CExoString("Fixture", 7);
    fixture_turd.last_name.value = CExoString("Character", 9);
    fixture_turd_node = {nullptr, nullptr, &fixture_turd};
    fixture_turd_list = {&fixture_turd_node, &fixture_turd_node, 1, 0};
}

extern "C" {

std::int32_t nwnrs_fixture_enable_combat_debugging = 0;
std::int32_t nwnrs_fixture_enable_saving_throw_debugging = 1;
std::int32_t nwnrs_fixture_enable_movement_speed_debugging = 0;
std::int32_t nwnrs_fixture_enable_hit_die_debugging = 1;
std::int32_t nwnrs_fixture_exit_program = 0;
std::int32_t nwnrs_fixture_rules_object = 0;
void* nwnrs_fixture_rules = &nwnrs_fixture_rules_object;
void* nwnrs_fixture_exo_base = &fixture_exo_base;
std::int32_t nwnrs_fixture_disconnect_count = 0;
std::uint32_t nwnrs_fixture_disconnect_reason_length = 0;

void nwnrs_fixture_admin_init(
    void* object,
    const char* server_vault,
    std::size_t server_vault_length) {
    auto* network = static_cast<FixtureNetLayer*>(object);
    network->session_name = CExoString("fixture server", 14);
    network->player_password = CExoString("player secret", 13);
    network->dm_password = CExoString();

    fixture_player.player_id = 77;
    std::memset(fixture_player.file_name, 0, sizeof(fixture_player.file_name));
    constexpr char file_name[] = "fixturechar";
    std::memcpy(fixture_player.file_name, file_name, sizeof(file_name) - 1);
    fixture_player_info.public_cd_key = CExoString("fixture-key", 11);
    fixture_creature_stats.first_name.value = CExoString("Fixture", 7);
    fixture_creature_stats.last_name.value = CExoString("Character", 9);
    fixture_alias_list.server_vault = CExoString(server_vault, server_vault_length);
    nwnrs_fixture_disconnect_count = 0;
    nwnrs_fixture_disconnect_reason_length = 0;
    reset_fixture_turd();
}

void nwnrs_fixture_reset_turd() { reset_fixture_turd(); }

} // extern "C"

CExoString fixture_get_session_name(void* object)
    asm("nwnrs_fixture_get_session_name");
CExoString fixture_get_session_name(void* object) {
    return static_cast<FixtureNetLayer*>(object)->session_name;
}

void fixture_set_session_name(void* object, CExoString value)
    asm("nwnrs_fixture_set_session_name");
void fixture_set_session_name(void* object, CExoString value) {
    static_cast<FixtureNetLayer*>(object)->session_name = std::move(value);
}

CExoString fixture_get_player_password(void* object)
    asm("nwnrs_fixture_get_player_password");
CExoString fixture_get_player_password(void* object) {
    return static_cast<FixtureNetLayer*>(object)->player_password;
}

std::int32_t fixture_set_player_password(void* object, CExoString value)
    asm("nwnrs_fixture_set_player_password");
std::int32_t fixture_set_player_password(void* object, CExoString value) {
    static_cast<FixtureNetLayer*>(object)->player_password = std::move(value);
    return 1;
}

CExoString fixture_get_game_master_password(void* object)
    asm("nwnrs_fixture_get_game_master_password");
CExoString fixture_get_game_master_password(void* object) {
    return static_cast<FixtureNetLayer*>(object)->dm_password;
}

std::int32_t fixture_set_game_master_password(void* object, CExoString value)
    asm("nwnrs_fixture_set_game_master_password");
std::int32_t fixture_set_game_master_password(void* object, CExoString value) {
    static_cast<FixtureNetLayer*>(object)->dm_password = std::move(value);
    return 1;
}

void fixture_add_banned_ip(void*, CExoString)
    asm("nwnrs_fixture_add_banned_ip");
void fixture_add_banned_ip(void*, CExoString) {}

void fixture_remove_banned_ip(void*, CExoString)
    asm("nwnrs_fixture_remove_banned_ip");
void fixture_remove_banned_ip(void*, CExoString) {}

void fixture_add_banned_cd_key(void*, CExoString)
    asm("nwnrs_fixture_add_banned_cd_key");
void fixture_add_banned_cd_key(void*, CExoString) {}

void fixture_remove_banned_cd_key(void*, CExoString)
    asm("nwnrs_fixture_remove_banned_cd_key");
void fixture_remove_banned_cd_key(void*, CExoString) {}

void fixture_add_banned_player_name(void*, CExoString)
    asm("nwnrs_fixture_add_banned_player_name");
void fixture_add_banned_player_name(void*, CExoString) {}

void fixture_remove_banned_player_name(void*, CExoString)
    asm("nwnrs_fixture_remove_banned_player_name");
void fixture_remove_banned_player_name(void*, CExoString) {}

void fixture_reload_rules(void*) asm("nwnrs_fixture_reload_rules");
void fixture_reload_rules(void*) {}

void* fixture_get_module(void*) asm("nwnrs_fixture_get_module");
void* fixture_get_module(void*) { return &fixture_module; }

std::int32_t fixture_get_loc_string(
    const void* object,
    std::int32_t,
    CExoString* output,
    std::uint8_t) asm("nwnrs_fixture_get_loc_string");
std::int32_t fixture_get_loc_string(
    const void* object,
    std::int32_t,
    CExoString* output,
    std::uint8_t) {
    if (object == nullptr || output == nullptr) {
        return 0;
    }
    *output = static_cast<const FixtureLocString*>(object)->value;
    return 1;
}

void* fixture_remove_linked_list_node(void* list, void* node)
    asm("nwnrs_fixture_remove_linked_list_node");
void* fixture_remove_linked_list_node(void* list, void* node) {
    auto* linked_list = static_cast<FixtureLinkedListInternal*>(list);
    auto* linked_node = static_cast<FixtureLinkedListNode*>(node);
    if (linked_list == nullptr || linked_node == nullptr ||
        linked_list->head != linked_node || linked_list->count != 1) {
        return nullptr;
    }
    linked_list->head = nullptr;
    linked_list->tail = nullptr;
    linked_list->count = 0;
    return linked_node->object;
}

void* fixture_get_client_object_by_object_id(void*, std::uint32_t object_id)
    asm("nwnrs_fixture_get_client_object_by_object_id");
void* fixture_get_client_object_by_object_id(void*, std::uint32_t object_id) {
    return object_id == 0x01020304 ? &fixture_player : nullptr;
}

void* fixture_get_creature_by_game_object_id(void*, std::uint32_t object_id)
    asm("nwnrs_fixture_get_creature_by_game_object_id");
void* fixture_get_creature_by_game_object_id(void*, std::uint32_t object_id) {
    return object_id == 0x01020304 ? &fixture_creature : nullptr;
}

CExoString fixture_get_player_name(void*) asm("nwnrs_fixture_get_player_name");
CExoString fixture_get_player_name(void*) {
    return CExoString("fixture-player", 14);
}

void* fixture_get_player_info(void*, std::uint32_t player_id)
    asm("nwnrs_fixture_get_player_info");
void* fixture_get_player_info(void*, std::uint32_t player_id) {
    return player_id == fixture_player.player_id ? &fixture_player_info : nullptr;
}

std::int32_t fixture_disconnect_player(
    void*,
    std::uint32_t player_id,
    std::uint32_t string_reference,
    std::int32_t cd_auth_failure,
    const CExoString& reason) asm("nwnrs_fixture_disconnect_player");
std::int32_t fixture_disconnect_player(
    void*,
    std::uint32_t player_id,
    std::uint32_t string_reference,
    std::int32_t cd_auth_failure,
    const CExoString& reason) {
    if (player_id != fixture_player.player_id || string_reference != 10392 ||
        cd_auth_failure != 1) {
        return 0;
    }
    ++nwnrs_fixture_disconnect_count;
    nwnrs_fixture_disconnect_reason_length = reason.length;
    return 1;
}

const CExoString& fixture_get_alias_path(
    const void*,
    const CExoString& alias,
    std::int32_t) asm("nwnrs_fixture_get_alias_path");
const CExoString& fixture_get_alias_path(
    const void*,
    const CExoString& alias,
    std::int32_t) {
    static const CExoString empty;
    if (alias.length != 11 || alias.string == nullptr ||
        std::memcmp(alias.string, "SERVERVAULT", 11) != 0) {
        return empty;
    }
    return fixture_alias_list.server_vault;
}

extern "C" void* nwnrs_fixture_admin_keep_symbols() {
    static void* volatile sink;
    sink = reinterpret_cast<void*>(&fixture_get_session_name);
    sink = reinterpret_cast<void*>(&fixture_set_session_name);
    sink = reinterpret_cast<void*>(&fixture_get_player_password);
    sink = reinterpret_cast<void*>(&fixture_set_player_password);
    sink = reinterpret_cast<void*>(&fixture_get_game_master_password);
    sink = reinterpret_cast<void*>(&fixture_set_game_master_password);
    sink = reinterpret_cast<void*>(&fixture_add_banned_ip);
    sink = reinterpret_cast<void*>(&fixture_remove_banned_ip);
    sink = reinterpret_cast<void*>(&fixture_add_banned_cd_key);
    sink = reinterpret_cast<void*>(&fixture_remove_banned_cd_key);
    sink = reinterpret_cast<void*>(&fixture_add_banned_player_name);
    sink = reinterpret_cast<void*>(&fixture_remove_banned_player_name);
    sink = reinterpret_cast<void*>(&fixture_reload_rules);
    sink = reinterpret_cast<void*>(&fixture_get_module);
    sink = reinterpret_cast<void*>(&fixture_get_loc_string);
    sink = reinterpret_cast<void*>(&fixture_remove_linked_list_node);
    sink = reinterpret_cast<void*>(&fixture_get_client_object_by_object_id);
    sink = reinterpret_cast<void*>(&fixture_get_creature_by_game_object_id);
    sink = reinterpret_cast<void*>(&fixture_get_player_name);
    sink = reinterpret_cast<void*>(&fixture_get_player_info);
    sink = reinterpret_cast<void*>(&fixture_disconnect_player);
    sink = reinterpret_cast<void*>(&fixture_get_alias_path);
    sink = nwnrs_fixture_exo_base;
    return sink;
}
