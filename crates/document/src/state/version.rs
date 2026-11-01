//! The file's version, and the upgrades that bring an older file to
//! this reader's form (docs/redraw-plan.md §2.1).
//!
//! Every save writes the header `"format": "lmsheets-document"`,
//! `"version": N`. A file without the header is version 1, the form
//! from before it. [`upgrade`] takes a file of any version this reader
//! knows, as an untyped JSON value, through each upgrader in turn to
//! [`VERSION`]; the typed parse and its checks come after, so the Rust
//! types stay the only schema.
//!
//! **To change the format**, write `fn vN(file: &mut Map<String, Value>)`
//! taking a version-N file's fields to version N+1's, append it to
//! `UPGRADES` ([`VERSION`] follows), and store files of the new
//! version under `tests/fixtures/vN+1/`.

use serde_json::{json, Map, Value};

/// The `format` every document file names.
pub const FORMAT: &str = "lmsheets-document";

/// An upgrader: a version-N file's fields to version N+1's.
type Upgrade = fn(&mut Map<String, Value>);

/// `UPGRADES[i]` takes version `i + 1` to `i + 2`.
const UPGRADES: &[Upgrade] = &[v1, v2, v3];

/// The version this crate writes, and the newest it reads.
pub const VERSION: u64 = UPGRADES.len() as u64 + 1;

/// Why a file cannot be brought to this reader's version.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The file is not a JSON object.
    NotAnObject,
    /// The `format` is not [`FORMAT`], or is missing beside a `version`.
    Format(Option<Value>),
    /// The `version` is not a positive integer, or is missing beside the
    /// `format`.
    Version(Option<Value>),
    /// The file is newer than this reader.
    Newer {
        /// The file's version.
        file: u64,
        /// This reader's, [`VERSION`].
        reader: u64,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NotAnObject => write!(f, "a document file is a JSON object"),
            Error::Format(None) => write!(f, "a version without \"format\": \"{FORMAT}\""),
            Error::Format(Some(v)) => write!(f, "not an {FORMAT} file: \"format\" is {v}"),
            Error::Version(None) => write!(f, "an {FORMAT} file without a version"),
            Error::Version(Some(v)) => write!(f, "the version {v} is not a positive integer"),
            Error::Newer { file, reader } => write!(
                f,
                "the file is version {file}, newer than this reader (version {reader})"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// 1 → 2, the frames (docs/redraw-plan.md §2.2): the one `scan` becomes
/// the frame `scan`, its registration the chain's paper similarity as it
/// was, the other maps identities (the whole frame, no warp, no turn),
/// so the chain is the old similarity exactly.
fn v1(file: &mut Map<String, Value>) {
    // A missing or null scan (version 1 read null as none): no frames.
    let Some(scan) = file.remove("scan").filter(|s| !s.is_null()) else {
        return;
    };
    let size = &scan["size_px"];
    let step = |map: Value, note: &str| {
        json!({
            "map": map,
            "provenance": {
                "evidence": [],
                "method": {"kind": "assumed"},
                "status": {"kind": "provisional"},
                "by": {"kind": "script", "name": "lmsheets-document upgrade 1 → 2"},
                "when": "2026-10-26",
                "note": note,
            },
        })
    };
    let registration = json!({
        "crop": step(
            json!({"box_px": [0, 0, size[0], size[1]]}),
            "the whole frame: version 1 had no crop",
        ),
        "warp": step(
            json!({"centre": [0.0, 0.0], "half": 0.0, "shear": 0.0, "dx": [], "dy": [],
                   "extent": [0.0, 0.0, 0.0, 0.0]}),
            "none: version 1 had no warp",
        ),
        "rotation": step(
            json!({"centre": [0.0, 0.0], "angle": 0.0}),
            "none: version 1's turn is in its one similarity, kept whole as the paper's",
        ),
        "paper": step(
            scan["registration"].clone(),
            "version 1's registration (scan px onto paper mm), as it was",
        ),
    });
    let frame = json!({
        "file": scan["file"],
        "size_px": scan["size_px"],
        "registration": registration,
    });
    file.insert("frames".into(), json!({ "scan": frame }));
}

/// 2 → 3, the observed features (docs/redraw-plan.md §8.3, K11): a new
/// top-level `observed` map and the standard `observed` layer. A version-2
/// file has no observed features, which is the field absent; nothing in
/// it changes. (The layer table is the file's own: a file without the
/// `observed` layer simply has no feature on it.)
fn v2(_file: &mut Map<String, Value>) {}

/// 3 → 4, the marking regions (docs/redraw-plan.md K5, chunk 14): a new
/// top-level `marking_regions` map and a new `marking_region` claim
/// kind. A version-3 file has no marking regions, which is the field
/// absent; nothing in it changes.
fn v3(_file: &mut Map<String, Value>) {}

/// `file` brought to [`VERSION`], its header set; refused if it is not
/// a document or is newer than this reader.
pub fn upgrade(file: Value) -> Result<Value, Error> {
    upgrade_with(file, UPGRADES)
}

/// [`upgrade`] with the chain given, `upgrades[i]` taking version `i + 1`
/// to `i + 2`.
fn upgrade_with(file: Value, upgrades: &[Upgrade]) -> Result<Value, Error> {
    let Value::Object(mut file) = file else {
        return Err(Error::NotAnObject);
    };
    let reader = upgrades.len() as u64 + 1;
    let version = version_of(&file)?;
    if version > reader {
        return Err(Error::Newer {
            file: version,
            reader,
        });
    }
    for up in &upgrades[version as usize - 1..] {
        up(&mut file);
    }
    file.insert("format".into(), FORMAT.into());
    file.insert("version".into(), reader.into());
    Ok(Value::Object(file))
}

/// The version the header gives: both fields or neither (version 1).
fn version_of(file: &Map<String, Value>) -> Result<u64, Error> {
    match (file.get("format"), file.get("version")) {
        (None, None) => Ok(1),
        (None, Some(_)) => Err(Error::Format(None)),
        (Some(f), _) if f != FORMAT => Err(Error::Format(Some(f.clone()))),
        (Some(_), None) => Err(Error::Version(None)),
        (Some(_), Some(v)) => v
            .as_u64()
            .filter(|&n| n >= 1)
            .ok_or_else(|| Error::Version(Some(v.clone()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Two dummy upgraders: 1 → 2 renames `a` to `b`, 2 → 3 doubles `b`.
    const DUMMY: &[Upgrade] = &[
        |f| {
            let a = f.remove("a").unwrap();
            f.insert("b".into(), a);
        },
        |f| {
            let b = f["b"].as_u64().unwrap();
            f.insert("b".into(), (2 * b).into());
        },
    ];

    #[test]
    fn the_chain_runs_from_the_files_version() {
        let up = |v| upgrade_with(v, DUMMY).unwrap();
        let three = json!({"format": FORMAT, "version": 3, "b": 4});
        assert_eq!(up(json!({"a": 2})), three);
        assert_eq!(up(json!({"format": FORMAT, "version": 1, "a": 2})), three);
        assert_eq!(up(json!({"format": FORMAT, "version": 2, "b": 2})), three);
        assert_eq!(up(three.clone()), three);
    }

    #[test]
    fn version_1s_scan_becomes_a_frame() {
        let reg = json!({"origin_px": [-0.49, 0.70], "px_per_mm": 7.87, "rotation": -1e-4});
        let scan = json!({"file": "a.png", "size_px": [30, 20], "registration": reg});
        let v = upgrade(json!({"title": {}, "scan": scan})).unwrap();
        let frame = &v["frames"]["scan"];
        assert!(v.get("scan").is_none());
        assert_eq!(frame["file"], "a.png");
        assert_eq!(frame["size_px"], json!([30, 20]));
        let r = &frame["registration"];
        assert_eq!(r["paper"]["map"], reg);
        assert_eq!(r["crop"]["map"]["box_px"], json!([0, 0, 30, 20]));
        assert_eq!(r["rotation"]["map"]["angle"], 0.0);
        // Without a scan, or with a null one, no frames.
        for file in [json!({"title": {}}), json!({"title": {}, "scan": null})] {
            let v = upgrade(file).unwrap();
            assert!(v.get("frames").is_none() && v.get("scan").is_none());
        }
    }

    #[test]
    fn version_2_gains_nothing_but_the_version() {
        let two = json!({"format": FORMAT, "version": 2, "title": {}, "layers": {}});
        let up = upgrade(two.clone()).unwrap();
        assert_eq!(up["version"], VERSION);
        assert_eq!(up["layers"], two["layers"]);
        assert!(up.get("observed").is_none());
    }

    #[test]
    fn version_3_gains_nothing_but_the_version() {
        let three = json!({"format": FORMAT, "version": 3, "title": {}, "layers": {}});
        let up = upgrade(three.clone()).unwrap();
        assert_eq!(up["version"], VERSION);
        assert_eq!(up["layers"], three["layers"]);
        assert!(up.get("marking_regions").is_none());
    }

    #[test]
    fn an_unversioned_file_is_version_1() {
        let v = upgrade(json!({"title": {}})).unwrap();
        assert_eq!(
            v,
            json!({"format": FORMAT, "version": VERSION, "title": {}})
        );
    }

    #[test]
    fn a_bad_header_is_refused() {
        let e = |v| upgrade(v).unwrap_err();
        let newer = json!({"format": FORMAT, "version": VERSION + 1});
        assert_eq!(
            e(newer).to_string(),
            format!(
                "the file is version {}, newer than this reader (version {VERSION})",
                VERSION + 1
            )
        );
        assert_eq!(e(json!({"version": 1})), Error::Format(None));
        let other = json!({"format": "blobsketch-sketch/1", "version": 1});
        assert_eq!(e(other), Error::Format(Some("blobsketch-sketch/1".into())));
        assert_eq!(e(json!({"format": FORMAT})), Error::Version(None));
        for v in [json!(0), json!(-1), json!(1.5), json!("1")] {
            let file = json!({"format": FORMAT, "version": v});
            assert_eq!(e(file), Error::Version(Some(v)));
        }
        assert_eq!(e(json!([])), Error::NotAnObject);
    }
}
