use std::{collections::BTreeMap, ffi::c_void};

use nwnrs_runtime::{EventObjectId, EventValue, EventVector, FEAT_HAS_ID_WHITELIST};

use super::{
    super::{
        Engine, EventFrame, EventSpec,
        abi::{DecrementFeatRemainingUses, EngineVector, GetFeatRemainingUses, HasFeat, UseFeat},
        active_engine,
        hook::{NativeHookSpec, NativeOriginal},
    },
    dispatch, native,
};

const USE_HOOK: &str = "feat_use";
const DECREMENT_HOOK: &str = "feat_decrement_remaining_uses";
const HAS_HOOK: &str = "feat_has";

static USE_ORIGINAL: NativeOriginal<UseFeat> = NativeOriginal::new();
static DECREMENT_ORIGINAL: NativeOriginal<DecrementFeatRemainingUses> = NativeOriginal::new();
static HAS_ORIGINAL: NativeOriginal<HasFeat> = NativeOriginal::new();

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    if let Some(target) = engine.event_hook_target(USE_HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            USE_HOOK,
            "CNWSCreature::UseFeat events",
            target,
            use_replacement as UseFeat,
            &USE_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(DECREMENT_HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            DECREMENT_HOOK,
            "CNWSCreatureStats::DecrementFeatRemainingUses events",
            target,
            decrement_replacement as DecrementFeatRemainingUses,
            &DECREMENT_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(HAS_HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            HAS_HOOK,
            "CNWSCreatureStats::HasFeat events",
            target,
            has_replacement as HasFeat,
            &HAS_ORIGINAL,
        ));
    }
}

extern "C" fn decrement_replacement(stats: *mut c_void, feat: u16) {
    let Some(creature) = native::stats_creature(stats) else {
        call_decrement(stats, feat);
        return;
    };
    let remaining = get_remaining_uses(stats, feat).unwrap_or(0);
    let before_data = BTreeMap::from([
        ("feat_id".to_string(), EventValue::Integer(i32::from(feat))),
        (
            "remaining_uses".to_string(),
            EventValue::Integer(i32::from(remaining)),
        ),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("feat.decrement_remaining_uses", "before"),
        before_data,
    )
    .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_decrement(stats, feat);
    }
    let remaining = get_remaining_uses(stats, feat).unwrap_or(remaining);
    dispatch::game_object(
        creature,
        EventSpec::catalog("feat.decrement_remaining_uses", "after"),
        BTreeMap::from([
            ("feat_id".to_string(), EventValue::Integer(i32::from(feat))),
            (
                "remaining_uses".to_string(),
                EventValue::Integer(i32::from(remaining)),
            ),
        ]),
    );
}

extern "C" fn has_replacement(stats: *mut c_void, feat: u16) -> i32 {
    if !active_engine().is_some_and(|engine| {
        engine.event_id_is_whitelisted(FEAT_HAS_ID_WHITELIST, i32::from(feat))
    }) {
        return call_has(stats, feat);
    }
    let original_result = call_has(stats, feat);
    let Some(creature) = native::stats_creature(stats) else {
        return original_result;
    };
    let data = BTreeMap::from([
        ("feat_id".to_string(), EventValue::Integer(i32::from(feat))),
        (
            "has_feat".to_string(),
            EventValue::Boolean(original_result != 0),
        ),
    ]);
    let before = dispatch::game_object(creature, EventSpec::catalog("feat.has", "before"), data);
    let result = if before.as_ref().is_some_and(EventFrame::skipped) {
        i32::from(
            before
                .as_ref()
                .and_then(EventFrame::boolean_result)
                .unwrap_or(false),
        )
    } else {
        original_result
    };
    dispatch::game_object(
        creature,
        EventSpec::catalog("feat.has", "after"),
        BTreeMap::from([
            ("feat_id".to_string(), EventValue::Integer(i32::from(feat))),
            (
                "has_feat".to_string(),
                EventValue::Boolean(original_result != 0),
            ),
            (
                "action_result".to_string(),
                EventValue::Boolean(result != 0),
            ),
        ]),
    );
    result
}

fn get_remaining_uses(stats: *mut c_void, feat: u16) -> Option<u8> {
    let function =
        active_engine()?.event_function::<GetFeatRemainingUses>("stats_get_feat_remaining_uses")?;
    Some(function.get()(stats, feat))
}

fn call_decrement(stats: *mut c_void, feat: u16) {
    if let Some(original) = DECREMENT_ORIGINAL.load() {
        original(stats, feat);
    }
}

fn call_has(stats: *mut c_void, feat: u16) -> i32 {
    HAS_ORIGINAL
        .load()
        .map_or(0, |original| original(stats, feat))
}

extern "C" fn use_replacement(
    creature: *mut c_void,
    feat: u16,
    subfeat: u16,
    target: u32,
    area: u32,
    position: *const EngineVector,
) -> i32 {
    let event_position = if position.is_null() {
        EventVector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    } else {
        // SAFETY: UseFeat supplies this vector for the synchronous call.
        let position = unsafe { *position };
        EventVector {
            x: position.x,
            y: position.y,
            z: position.z,
        }
    };
    let data = BTreeMap::from([
        (
            "area".to_string(),
            EventValue::Object(EventObjectId::new(area)),
        ),
        ("feat".to_string(), EventValue::Integer(i32::from(feat))),
        ("position".to_string(), EventValue::Vector(event_position)),
        (
            "subfeat".to_string(),
            EventValue::Integer(i32::from(subfeat)),
        ),
        (
            "target".to_string(),
            EventValue::Object(EventObjectId::new(target)),
        ),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("feat.use", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_original(creature, feat, subfeat, target, area, position)
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(creature, EventSpec::catalog("feat.use", "after"), after);
    result
}

fn call_original(
    creature: *mut c_void,
    feat: u16,
    subfeat: u16,
    target: u32,
    area: u32,
    position: *const EngineVector,
) -> i32 {
    let Some(original) = USE_ORIGINAL.load() else {
        return 0;
    };
    original(creature, feat, subfeat, target, area, position)
}
