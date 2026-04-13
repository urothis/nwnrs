#![forbid(unsafe_code)]
//! WebAssembly bindings for NWN1EE types and utilities.

#[macro_use]
mod bindings;
mod erf;
mod gff;
mod lossless;
mod mdl;
mod ssf;
mod tlk;
mod twoda;

pub use erf::{
    CompressedBufAlgorithmDto, ErfDto, ErfEntryDto, ErfLocStringDto, ErfVersionDto,
    read_erf_from_bytes, write_erf_to_bytes,
};
pub use gff::{
    GffFieldDto, GffLocStringDto, GffLocStringEntryDto, GffRootDto, GffStructDto, GffValueDto,
    read_gff_from_bytes, write_gff_to_bytes,
};
pub use lossless::LosslessDtoMetadata;
pub use mdl::{MdlDto, MdlEncodingDto, read_mdl_from_bytes, write_mdl_to_bytes};
pub use ssf::{SsfEntryDto, SsfRootDto, read_ssf_from_bytes, write_ssf_to_bytes};
pub use tlk::{SingleTlkDto, TlkEntryDto, read_tlk_from_bytes, write_tlk_to_bytes};
pub use twoda::{TwoDaDto, read_twoda_from_bytes, write_twoda_to_bytes};

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use nwnrs::{
        prelude::{
            compressedbuf, erf, exo, gff, localization::Language, mdl, resref, ssf, tlk, twoda,
        },
        resman::CachePolicy,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use nwnrs_test_support::{demand_resource, require_game_resource};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    use crate::{
        ErfDto, GffFieldDto, GffRootDto, GffStructDto, GffValueDto, MdlDto, MdlEncodingDto,
        SingleTlkDto, SsfRootDto, TwoDaDto,
        erf::{read_erf_dto, write_erf_dto},
        gff::{read_gff_dto, write_gff_dto},
        lossless::with_lossless_metadata,
        mdl::{read_mdl_dto, write_mdl_dto},
        ssf::{read_ssf_dto, write_ssf_dto},
        tlk::{read_tlk_dto, write_tlk_dto},
        twoda::{read_twoda_dto, unchanged_twoda_bytes, write_twoda_dto},
    };

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn lossless_metadata_returns_original_bytes_when_unchanged() {
        let value = TwoDaDto {
            default_value: Some("****".to_string()),
            columns:       vec!["col".to_string()],
            rows:          vec![vec![Some("value".to_string())]],
            row_labels:    vec!["0".to_string()],
            lossless:      None,
        };
        let value_result = with_lossless_metadata(
            value,
            b"original".to_vec(),
            |dto| &mut dto.lossless,
            "failed to fingerprint 2DA DTO",
        );
        assert!(
            value_result.is_ok(),
            "unexpected error: {:?}",
            value_result.as_ref().err()
        );
        let value = match value_result.ok() {
            Some(value) => value,
            None => return,
        };

        let bytes_result = unchanged_twoda_bytes(&value);
        assert!(
            bytes_result.is_ok(),
            "unexpected error: {:?}",
            bytes_result.as_ref().err()
        );
        let bytes = match bytes_result.ok() {
            Some(bytes) => bytes,
            None => return,
        };
        assert_eq!(bytes, Some(b"original".to_vec()));
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn lossless_metadata_detects_semantic_change() {
        let value = TwoDaDto {
            default_value: None,
            columns:       vec!["col".to_string()],
            rows:          vec![vec![Some("value".to_string())]],
            row_labels:    vec!["0".to_string()],
            lossless:      None,
        };
        let value_result = with_lossless_metadata(
            value,
            b"original".to_vec(),
            |dto| &mut dto.lossless,
            "failed to fingerprint 2DA DTO",
        );
        assert!(
            value_result.is_ok(),
            "unexpected error: {:?}",
            value_result.as_ref().err()
        );
        let mut value = match value_result.ok() {
            Some(value) => value,
            None => return,
        };
        let row = value.rows.get_mut(0);
        assert!(row.is_some());
        if let Some(row) = row {
            let cell = row.get_mut(0);
            assert!(cell.is_some());
            if let Some(cell) = cell {
                *cell = Some("changed".to_string());
            }
        }

        let bytes_result = unchanged_twoda_bytes(&value);
        assert!(
            bytes_result.is_ok(),
            "unexpected error: {:?}",
            bytes_result.as_ref().err()
        );
        let bytes = match bytes_result.ok() {
            Some(bytes) => bytes,
            None => return,
        };
        assert_eq!(bytes, None);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn twoda_edited_write_roundtrips_through_native_codec() {
        let mut table = twoda::TwoDa::new();
        table
            .set_columns(vec!["col".to_string()])
            .expect("set columns");
        table
            .replace_rows(vec![vec![Some("value".to_string())]], vec!["0".to_string()])
            .expect("set rows");
        let mut encoded = Vec::new();
        twoda::write_twoda(&mut encoded, &table, false).expect("encode 2DA");

        let value: TwoDaDto = read_twoda_dto(&encoded).expect("read wasm 2DA");
        let mut edited = value.clone();
        let row = edited.rows.get_mut(0);
        assert!(row.is_some());
        if let Some(row) = row {
            let cell = row.get_mut(0);
            assert!(cell.is_some());
            if let Some(cell) = cell {
                *cell = Some("changed".to_string());
            }
        }
        let label = edited.row_labels.get_mut(0);
        assert!(label.is_some());
        if let Some(label) = label {
            *label = "custom0".to_string();
        }

        let rewritten = write_twoda_dto(&edited).expect("write 2DA");
        let reparsed = twoda::read_twoda(Cursor::new(rewritten)).expect("reparse 2DA");
        assert_eq!(
            reparsed
                .rows
                .first()
                .and_then(|row| row.first())
                .and_then(|cell| cell.as_deref()),
            Some("changed")
        );
        assert_eq!(reparsed.row_label(0), Some("custom0"));
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn tlk_edited_write_preserves_descriptor_metadata() {
        let mut source = tlk::SingleTlk::new();
        source.language = Language::French;
        let mut entry = tlk::TlkEntry::new("before", "snd01", 1.25);
        entry.raw_text = Some(vec![b'b', b'e', b'f', b'o', b'r', b'e']);
        entry.flags = 7;
        entry.volume_variance = 11;
        entry.pitch_variance = 13;
        entry.sound_length_bits = 1.25_f32.to_bits();
        source.set_entry(0, entry);
        let mut encoded = Cursor::new(Vec::new());
        tlk::write_single_tlk(&mut encoded, &mut source).expect("encode TLK");

        let value: SingleTlkDto = read_tlk_dto(&encoded.into_inner()).expect("read wasm TLK");
        let mut edited = value.clone();
        let entry = edited.entries.get_mut(0).and_then(Option::as_mut);
        assert!(entry.is_some());
        let Some(entry) = entry else {
            return;
        };
        entry.text = "after".to_string();

        let rewritten = write_tlk_dto(&edited).expect("write TLK");
        let mut reparsed =
            tlk::read_single_tlk(Cursor::new(rewritten), CachePolicy::Bypass).expect("reparse TLK");
        let entry = reparsed.get(0).expect("get entry").expect("present");
        assert_eq!(entry.text, "after");
        assert_eq!(entry.flags, 7);
        assert_eq!(entry.volume_variance, 11);
        assert_eq!(entry.pitch_variance, 13);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn ssf_edited_write_preserves_raw_slot_bytes() {
        let mut source = ssf::SsfRoot::new();
        let mut entry = ssf::SsfEntry::new("snd", 10);
        entry.raw_resref = [
            b's', b'n', b'd', 0, b'X', b'Y', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        source.entries.push(entry);
        let mut encoded = Vec::new();
        ssf::write_ssf(&mut encoded, &source).expect("encode SSF");

        let value: SsfRootDto = read_ssf_dto(&encoded).expect("read wasm SSF");
        let mut edited = value.clone();
        let entry = edited.entries.get_mut(0);
        assert!(entry.is_some());
        if let Some(entry) = entry {
            entry.strref = 42;
        }

        let rewritten = write_ssf_dto(&edited).expect("write SSF");
        let mut cursor = Cursor::new(rewritten);
        let reparsed = ssf::read_ssf(&mut cursor).expect("reparse SSF");
        let entry = reparsed.entries.first();
        assert_eq!(entry.map(|entry| entry.strref), Some(42));
        assert_eq!(
            entry.and_then(|entry| entry.raw_resref.get(4)).copied(),
            Some(b'X')
        );
        assert_eq!(
            entry.and_then(|entry| entry.raw_resref.get(5)).copied(),
            Some(b'Y')
        );
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn erf_edited_write_preserves_resource_list_padding() {
        let entries = vec![
            resref::ResolvedResRef::from_filename("test.utc")
                .expect("resref")
                .into(),
        ];
        let mut encoded = Cursor::new(Vec::new());
        erf::write_erf_with_options(
            &mut encoded,
            "ERF ",
            erf::ErfVersion::E1,
            2026,
            97,
            exo::ExoResFileCompressionType::CompressedBuf,
            compressedbuf::Algorithm::None,
            &std::collections::BTreeMap::new(),
            -1,
            &entries,
            None,
            erf::ErfWriteOptions {
                resource_list_padding: 8,
            },
            |_rr, io| {
                io.write_all(b"before")?;
                Ok((6, nwnrs::prelude::checksums::secure_hash(b"before")))
            },
            |_rr| compressedbuf::Algorithm::Zlib,
        )
        .expect("encode ERF");

        let value: ErfDto = read_erf_dto(&encoded.into_inner(), "test.erf").expect("read wasm ERF");
        let mut edited = value.clone();
        let entry = edited.entries.get_mut(0);
        assert!(entry.is_some());
        if let Some(entry) = entry {
            entry.bytes = b"after".to_vec();
        }

        let rewritten = write_erf_dto(&edited).expect("write ERF");
        let reparsed =
            erf::read_erf(Cursor::new(rewritten), "test.erf".to_string()).expect("reparse ERF");
        assert_eq!(reparsed.resource_list_padding(), 8);
        assert_eq!(
            reparsed
                .entries()
                .values()
                .next()
                .expect("entry")
                .read_all(CachePolicy::Bypass)
                .expect("bytes"),
            b"after".to_vec()
        );
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn gff_edited_write_uses_native_merge_workflow() {
        let mut root = gff::GffRoot::new("UTC ");
        root.put_value("Comment", gff::GffValue::CExoString("before".to_string()))
            .expect("comment");
        root.put_value("Items", gff::GffValue::List(vec![gff::GffStruct::new(1)]))
            .expect("items");
        let mut encoded = Cursor::new(Vec::new());
        gff::write_gff_root(&mut encoded, &root).expect("encode GFF");

        let value: GffRootDto = read_gff_dto(&encoded.into_inner()).expect("read wasm GFF");
        let mut edited = value.clone();
        let comment = edited.root.fields.get_mut(0);
        assert!(comment.is_some());
        if let Some(comment) = comment {
            comment.value = GffValueDto::CExoString("after".to_string());
        }
        edited.root.fields.push(GffFieldDto {
            label: "Count".to_string(),
            value: GffValueDto::Int(2),
        });
        let items = edited.root.fields.get_mut(1);
        assert!(items.is_some());
        if let Some(items) = items {
            items.value = GffValueDto::List(vec![
                GffStructDto {
                    id:     1,
                    fields: Vec::new(),
                },
                GffStructDto {
                    id:     2,
                    fields: Vec::new(),
                },
            ]);
        }

        let rewritten = write_gff_dto(&edited).expect("write GFF");
        let reparsed = gff::read_gff_root(&mut Cursor::new(rewritten)).expect("reparse GFF");
        assert_eq!(
            reparsed.root.get_field("Comment").expect("comment").value(),
            &gff::GffValue::CExoString("after".to_string())
        );
        assert_eq!(
            reparsed.root.get_field("Count").expect("count").value(),
            &gff::GffValue::Int(2)
        );
        let items = match reparsed.root.get_field("Items").expect("items").value() {
            gff::GffValue::List(items) => items,
            other => {
                assert!(matches!(other, gff::GffValue::List(_)));
                return;
            }
        };
        assert_eq!(items.len(), 2);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn ascii_mdl_edited_write_roundtrips_through_native_codec() {
        let source = b"newmodel demo\nsetsupermodel demo null\nclassification character\nsetanimationscale 1\nbeginmodelgeom demo\nnode dummy demo\n  parent null\nendnode\nendmodelgeom demo\ndonemodel demo\n";
        let value: MdlDto = read_mdl_dto(source).expect("read wasm MDL");
        let mut edited = value.clone();
        edited.text = edited.text.replace("demo", "renamed");
        edited.encoding = MdlEncodingDto::Ascii;

        let rewritten = write_mdl_dto(&edited).expect("write ascii mdl");
        let mut cursor = Cursor::new(rewritten);
        let reparsed = mdl::read_ascii_model(&mut cursor).expect("reparse mdl");
        assert_eq!(reparsed.geometry_name, "renamed");
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg(not(target_arch = "wasm32"))]
    fn compiled_mdl_unchanged_write_reuses_original_bytes() {
        let result = require_game_resource(demand_resource("a_ba2", mdl::MODEL_RES_TYPE));
        let Ok(res) = result else {
            return;
        };
        let bytes = res
            .read_all(CachePolicy::Bypass)
            .expect("read shipped mdl bytes");

        let value: MdlDto = read_mdl_dto(&bytes).expect("read wasm compiled mdl");
        assert_eq!(value.encoding, MdlEncodingDto::Compiled);

        let rewritten = write_mdl_dto(&value).expect("write unchanged compiled mdl");
        assert_eq!(rewritten, bytes);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg(not(target_arch = "wasm32"))]
    fn compiled_mdl_edited_write_is_rejected() {
        let result = require_game_resource(demand_resource("a_ba2", mdl::MODEL_RES_TYPE));
        let Ok(res) = result else {
            return;
        };
        let bytes = res
            .read_all(CachePolicy::Bypass)
            .expect("read shipped mdl bytes");

        let mut value: MdlDto = read_mdl_dto(&bytes).expect("read wasm compiled mdl");
        value.text.push_str("\n# edited\n");

        let err = write_mdl_dto(&value).expect_err("edited compiled mdl should fail");
        let message = err.as_string().expect("error string");
        assert!(message.contains("edited compiled MDL writes are not supported yet"));
    }
}
