//! Prints the top-level fields from one installed GFF resource.

use std::io::Cursor;

use nwnrs_game::prelude::{find_nwnrs_root, find_user_root, new_default_resman};
use nwnrs_gff::prelude::{GffRoot, GffStruct, GffValue, read_gff_root};
use nwnrs_resref::prelude::ResolvedResRef;

fn main() {
    let filename = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "nw_commale.utc".to_string());

    let root = find_nwnrs_root("").unwrap_or_else(|error| {
        panic!("resolve NWN root: {error}");
    });
    let user_root = find_user_root("").unwrap_or_else(|error| {
        panic!("resolve NWN user root: {error}");
    });
    let mut resman = new_default_resman(
        &root,
        &user_root,
        "english",
        0,
        true,
        false,
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap_or_else(|error| {
        panic!("build default resman: {error}");
    });

    let resolved = ResolvedResRef::from_filename(filename.as_str()).unwrap_or_else(|error| {
        panic!("invalid resref {filename}: {error}");
    });
    let res = resman.get_resolved(&resolved).unwrap_or_else(|| {
        panic!("resource not found in resman: {filename}");
    });
    let bytes = res.read_all(true).unwrap_or_else(|error| {
        panic!("read {filename}: {error}");
    });
    let root = read_gff_root(&mut Cursor::new(bytes)).unwrap_or_else(|error| {
        panic!("parse {filename}: {error}");
    });

    print_root(&root);
}

fn print_root(root: &GffRoot) {
    println!("file_type={}", root.file_type);
    println!("field_count={}", root.root.fields().len());
    print_struct(&root.root, 0);
}

fn print_struct(value: &GffStruct, depth: usize) {
    let indent = "  ".repeat(depth);
    for (label, field) in value.fields() {
        print!("{indent}{label}: ");
        match field.value() {
            GffValue::Byte(value) => println!("Byte({value})"),
            GffValue::Char(value) => println!("Char({value})"),
            GffValue::Word(value) => println!("Word({value})"),
            GffValue::Short(value) => println!("Short({value})"),
            GffValue::Dword(value) => println!("Dword({value})"),
            GffValue::Int(value) => println!("Int({value})"),
            GffValue::Float(value) => println!("Float({value})"),
            GffValue::Dword64(value) => println!("Dword64({value})"),
            GffValue::Int64(value) => println!("Int64({value})"),
            GffValue::Double(value) => println!("Double({value})"),
            GffValue::CExoString(value) => println!("CExoString({value})"),
            GffValue::ResRef(value) => println!("ResRef({value})"),
            GffValue::CExoLocString(value) => {
                println!(
                    "CExoLocString(str_ref={}, entries={})",
                    value.str_ref,
                    value.entries.len()
                );
            }
            GffValue::Void(value) => println!("Void(len={})", value.len()),
            GffValue::Struct(value) => {
                println!("Struct(id={}, fields={})", value.id, value.fields().len());
                print_struct(value, depth + 1);
            }
            GffValue::List(value) => {
                println!("List(len={})", value.len());
                for (index, entry) in value.iter().enumerate() {
                    println!(
                        "{}  [{index}] Struct(id={}, fields={})",
                        indent,
                        entry.id,
                        entry.fields().len()
                    );
                    print_struct(entry, depth + 2);
                }
            }
        }
    }
}
