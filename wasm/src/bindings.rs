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

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::{from_js_value, js_error, to_js_value};

    #[wasm_bindgen_test]
    fn js_error_formats_context_and_message() {
        let value = js_error("serialize dto", "bad field");
        assert_eq!(value.as_string().as_deref(), Some("serialize dto: bad field"));
    }

    #[wasm_bindgen_test]
    fn serde_helpers_roundtrip_js_values() {
        let js = to_js_value(&vec!["alpha".to_string(), "beta".to_string()], "serialize vec")
            .expect("serialize should succeed");
        let value: Vec<String> = from_js_value(js, "deserialize vec").expect("deserialize");
        assert_eq!(value, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[wasm_bindgen_test]
    fn deserialize_errors_include_context() {
        let err = from_js_value::<Vec<String>>(JsValue::from_str("not-an-array"), "read vec")
            .expect_err("deserialization should fail");
        let message = err.as_string().expect("error string");
        assert!(message.contains("read vec"));
    }
}
