use std::io::{Read, Write};

use tracing::instrument;

use crate::{Model, ModelResult};

/// Reads an `MDL` payload from `reader`.
///
/// # Errors
///
/// Returns [`ModelError`] if the data cannot be read.
#[instrument(level = "debug", skip_all, err)]
pub fn read_model<R: Read>(reader: &mut R) -> ModelResult<Model> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(Model::new(bytes))
}

/// Writes an `MDL` payload to `writer`.
///
/// # Errors
///
/// Returns [`ModelError`] if the write fails.
#[instrument(level = "debug", skip_all, err, fields(byte_len = model.byte_len()))]
pub fn write_model<W: Write>(writer: &mut W, model: &Model) -> ModelResult<()> {
    writer.write_all(model.bytes())?;
    Ok(())
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::{Model, read_model, write_model};

    #[test]
    fn model_roundtrips_raw_bytes() {
        let original = Model::new(b"newmodel x\r\nendmodel\r\n".to_vec());

        let mut encoded = Vec::new();
        if let Err(error) = write_model(&mut encoded, &original) {
            panic!("write model: {error}");
        }

        let mut cursor = Cursor::new(encoded);
        let decoded = match read_model(&mut cursor) {
            Ok(model) => model,
            Err(error) => panic!("read model: {error}"),
        };

        assert_eq!(decoded, original);
    }
}
