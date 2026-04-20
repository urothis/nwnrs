use crate::{GffField, GffResult, GffRoot, GffStruct, GffValue};

/// Applies `edited` onto `target` while retaining provenance already present on
/// matching parsed fields and structures.
///
/// # Errors
///
/// Returns [`GffError`] if the merge cannot be applied.
pub fn merge_root_preserving_provenance(target: &mut GffRoot, edited: &GffRoot) -> GffResult<()> {
    target.file_type.clone_from(&edited.file_type);
    target.file_version.clone_from(&edited.file_version);
    merge_struct_preserving_provenance(&mut target.root, &edited.root);
    Ok(())
}

fn merge_struct_preserving_provenance(target: &mut GffStruct, edited: &GffStruct) {
    target.id = edited.id;

    let mut existing_fields = std::mem::take(&mut target.fields);
    let mut merged_fields = Vec::with_capacity(edited.fields().len());

    for (label, edited_field) in edited.fields() {
        if let Some(position) = existing_fields
            .iter()
            .position(|(existing_label, _)| existing_label == label)
        {
            let (_, mut existing_field) = existing_fields.remove(position);
            merge_field_preserving_provenance(&mut existing_field, edited_field);
            merged_fields.push((label.clone(), existing_field));
        } else {
            merged_fields.push((label.clone(), edited_field.clone()));
        }
    }

    target.fields = merged_fields;
    if let Some(provenance) = &mut target.provenance {
        provenance.field_labels = target
            .fields
            .iter()
            .map(|(label, _)| label.clone())
            .collect();
    }
}

fn merge_field_preserving_provenance(target: &mut GffField, edited: &GffField) {
    if target.kind() != edited.kind() {
        *target = edited.clone();
        return;
    }

    merge_value_preserving_provenance(&mut target.value, edited.value());
}

fn merge_value_preserving_provenance(target: &mut GffValue, edited: &GffValue) {
    match edited {
        GffValue::Struct(edited_struct) => {
            if let GffValue::Struct(target_struct) = target {
                merge_struct_preserving_provenance(target_struct, edited_struct);
            } else {
                *target = edited.clone();
            }
        }
        GffValue::List(edited_list) => {
            if let GffValue::List(target_list) = target {
                let existing = std::mem::take(target_list);
                let mut merged = Vec::with_capacity(edited_list.len());

                for (edited_struct, maybe_existing_struct) in edited_list.iter().zip(
                    existing
                        .into_iter()
                        .map(Some)
                        .chain(std::iter::repeat(None)),
                ) {
                    if let Some(mut existing_struct) = maybe_existing_struct {
                        merge_struct_preserving_provenance(&mut existing_struct, edited_struct);
                        merged.push(existing_struct);
                    } else {
                        merged.push(edited_struct.clone());
                    }
                }

                *target_list = merged;
            } else {
                *target = edited.clone();
            }
        }
        _ => *target = edited.clone(),
    }
}
