use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

pub(crate) fn js_error(context: &str, error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&format!("{context}: {error}"))
}

pub(crate) fn to_js_value<T: Serialize>(value: &T, context: &str) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value).map_err(|error| js_error(context, error))
}

pub(crate) fn from_js_value<T: for<'de> Deserialize<'de>>(
    value: JsValue,
    context: &str,
) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|error| js_error(context, error))
}

macro_rules! wasm_read_binding {
    (
        fn $name:ident($($arg:ident : $arg_ty:ty),* $(,)?) -> $dto:ty
        $body:block
        , serialize_context: $serialize_context:literal $(,)?
    ) => {
        #[doc = concat!("Reads a ", stringify!($dto), " value from raw bytes and returns it as a JavaScript value.")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        pub fn $name($($arg: $arg_ty),*) -> Result<JsValue, JsValue> {
            let dto_result: Result<$dto, JsValue> = { $body };
            let dto: $dto = dto_result?;
            crate::bindings::to_js_value(&dto, $serialize_context)
        }
    };
}

macro_rules! wasm_write_binding {
    (
        fn
        $name:ident($value:ident : JsValue) ->
        $dto:ty,deserialize_context:
        $deserialize_context:literal,
        $body:block $(,)?
    ) => {
        #[doc = concat!("Serializes a JavaScript ", stringify!($dto), " value back into raw bytes.")]
        #[::wasm_bindgen::prelude::wasm_bindgen]
        pub fn $name($value: JsValue) -> Result<Vec<u8>, JsValue> {
            let $value: $dto = crate::bindings::from_js_value($value, $deserialize_context)?;
            { $body }
        }
    };
}
