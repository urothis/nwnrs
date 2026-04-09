use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use nwnrs_resman::prelude::*;
use tracing::instrument;

use crate::{MODEL_RES_TYPE, Model, ModelError, ModelResult};

/// Reads an `MDL` payload from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_model<R: Read>(reader: &mut R) -> io::Result<Model> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(Model::new(bytes))
}

/// Reads an `MDL` payload from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_model_from_file(path: impl AsRef<Path>) -> io::Result<Model> {
    let mut file = File::open(path.as_ref())?;
    read_model(&mut file)
}

/// Reads an `MDL` payload from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_model_from_res(res: &Res, use_cache: bool) -> ModelResult<Model> {
    if res.resref().res_type() != MODEL_RES_TYPE {
        return Err(ModelError::msg(format!(
            "expected mdl resource, got {}",
            res.resref()
        )));
    }

    Ok(Model::new(res.read_all(use_cache)?))
}

/// Writes an `MDL` payload to `writer`.
#[instrument(level = "debug", skip_all, err, fields(byte_len = model.byte_len()))]
pub fn write_model<W: Write>(writer: &mut W, model: &Model) -> io::Result<()> {
    writer.write_all(model.bytes())
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
