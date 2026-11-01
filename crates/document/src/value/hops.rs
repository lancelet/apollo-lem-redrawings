//! A frame's hops back to the archive: how its pixels came to us, oldest
//! first (docs/redraw-plan.md §2.3).
//!
//! ```text
//! card (NARA box) ─scan─▶ upload (archive.org item: the tar member) ─▶ derived … ─▶ the frame's file
//! ```
//!
//! Each hop is made from the one before it, in order, and the hashes
//! chain them: each derived image names the hash of what it was made
//! from, which the hop before must have. The walk back from a frame
//! starts at its hash and ends at the upload's original, the tar member
//! as uploaded, whose URL is the archive's own and canonical. The
//! item's page rendering is kept beside it as a reference only. Each
//! hop carries its provenance: how we know it.

use crate::value::provenance::{self, Provenance};
use serde::{Deserialize, Serialize};

/// One hop, with how we know it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hop {
    /// What it is.
    #[serde(flatten)]
    pub what: What,
    /// How we know it.
    pub provenance: Provenance,
}

/// The kinds of hop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum What {
    /// The aperture card, in its box at NARA.
    Card {
        /// The NARA (Southwest, Fort Worth) aperture-card box.
        nara_box: u32,
    },
    /// The card's scan as archive.org holds it.
    Upload(Upload),
    /// An image we made from the hop before.
    Derived(Derived),
}

/// The scan in its archive.org item: a member of the item's original
/// tar, archive.org's rendering of it, and the item's rights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Upload {
    /// The item's identifier.
    pub item: String,
    /// The original tar, a file of the item.
    pub tar: String,
    /// The member, its name in the tar as listed.
    pub member: String,
    /// The member's bytes as uploaded: canonical, the start of the hash
    /// chain.
    pub original: Fetched,
    /// archive.org's page rendering of the member, when known: a
    /// reference, not canonical, made on request (its bytes may differ
    /// from one fetch to the next).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendering: Option<Rendering>,
    /// The rights, as the item's metadata gives them.
    pub rights: Rights,
}

/// A file of an archive.org item, as fetched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fetched {
    /// Its URL.
    pub url: String,
    /// The bytes' hash, `algorithm:hex`.
    pub hash: String,
    /// Their size, bytes.
    pub size: u64,
}

/// A page rendering of an archive.org item: not canonical.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rendering {
    /// The page, `n<N>` in the item's book reader.
    pub page: u32,
    /// The bytes as we once fetched them.
    #[serde(flatten)]
    pub fetched: Fetched,
}

/// An item's rights, each field verbatim from its metadata; none where
/// the metadata has no such field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rights {
    /// The `rights` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    /// The `licenseurl` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licenseurl: Option<String>,
}

/// An image made from the hop before.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Derived {
    /// The file, where we hold it.
    pub file: String,
    /// What was done, in words.
    pub made: String,
    /// The tool that did it, and its version, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The file's hashes, `algorithm:hex`.
    pub hashes: Vec<String>,
    /// The hash of what it was made from (one of the hop before's).
    pub from: String,
}

/// The walk back from a frame of hash `frame_hash` to its upload:
/// derived images, the last with the frame's hash and each made from a
/// hash the one before it has, then the upload, whose original has the
/// first one's `from`, then at most the card.
pub fn walk<'a>(hops: &'a [Hop], frame_hash: Option<&str>) -> Result<&'a Upload, Error> {
    if hops.is_empty() {
        return Err(Error::Empty);
    }
    let mut want = frame_hash.ok_or(Error::Unlinked(hops.len() - 1))?;
    let mut back = hops.iter().enumerate().rev();
    let upload = loop {
        match back.next() {
            Some((k, h)) => match &h.what {
                What::Derived(d) if d.hashes.iter().any(|x| x == want) => want = &d.from,
                What::Upload(u) if u.original.hash == want => break u,
                What::Card { .. } if k + 1 < hops.len() => return Err(Error::NoUpload),
                _ => return Err(Error::Unlinked(k)),
            },
            None => return Err(Error::NoUpload),
        }
    };
    let of_item = |url: &str| url.contains(&format!("/{}/", upload.item));
    let rendering = upload.rendering.as_ref().map(|r| &r.fetched.url[..]);
    if !(of_item(&upload.original.url) && rendering.is_none_or(of_item)) {
        return Err(Error::NotOfItem);
    }
    match (back.next(), back.next()) {
        (None, _)
        | (
            Some((
                _,
                Hop {
                    what: What::Card { .. },
                    ..
                },
            )),
            None,
        ) => Ok(upload),
        _ => Err(Error::OutOfOrder),
    }
}

/// Every hop's provenance passes its checks, and the walk back from a
/// frame of hash `frame_hash` holds (none to walk when there are no
/// hops).
pub fn check(hops: &[Hop], frame_hash: Option<&str>) -> Result<(), Error> {
    for h in hops {
        h.provenance.check()?;
    }
    if !hops.is_empty() {
        walk(hops, frame_hash)?;
    }
    Ok(())
}

/// What can be wrong with a frame's hops.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// No hops to walk.
    Empty,
    /// The hop at this index lacks the hash the walk came for: the
    /// frame's (the last hop), or the `from` of the hop after it.
    Unlinked(usize),
    /// The derived images do not lead back to an upload.
    NoUpload,
    /// An upload's URL is not of its item.
    NotOfItem,
    /// Something other than one card before the upload.
    OutOfOrder,
    /// A hop's provenance fails its checks.
    Provenance(provenance::Error),
}

impl From<provenance::Error> for Error {
    fn from(e: provenance::Error) -> Self {
        Error::Provenance(e)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::value::provenance::{Agent, Citation, Date, Method};

    fn known() -> Provenance {
        Provenance::new(
            Method::Read,
            Agent::Claude,
            Date::new(2026, 10, 26).unwrap(),
        )
        .cite(Citation::new("archive.org metadata", "the item"))
    }

    fn hop(what: What) -> Hop {
        Hop {
            what,
            provenance: known(),
        }
    }

    /// Card, upload with its rendering, two derived images, the last of
    /// hash `h`.
    pub(crate) fn trail(h: &str) -> Vec<Hop> {
        vec![
            hop(What::Card { nara_box: 511 }),
            hop(What::Upload(Upload {
                item: "box511".into(),
                tar: "box511.tar".into(),
                member: "box511/1374.png".into(),
                original: Fetched {
                    url: "https://archive.org/download/box511/box511.tar/box511/1374.png".into(),
                    hash: "sha256:00".into(),
                    size: 2,
                },
                rendering: Some(Rendering {
                    page: 1374,
                    fetched: Fetched {
                        url: "https://archive.org/download/box511/page/n1374.jpg".into(),
                        hash: "sha256:11".into(),
                        size: 3,
                    },
                }),
                rights: Rights {
                    rights: None,
                    licenseurl: Some("http://creativecommons.org/publicdomain/mark/1.0/".into()),
                },
            })),
            hop(What::Derived(Derived {
                file: "1374.png".into(),
                made: "extracted".into(),
                tool: None,
                hashes: vec!["sha256:00".into()],
                from: "sha256:00".into(),
            })),
            hop(What::Derived(Derived {
                file: "sheet.png".into(),
                made: "copied".into(),
                tool: None,
                hashes: vec!["sha256:00".into(), h.into()],
                from: "sha256:00".into(),
            })),
        ]
    }

    #[test]
    fn the_hops_round_trip() {
        let hops = trail("fnv1a64:815e1921c1dd6dd9");
        let text = serde_json::to_string(&hops).unwrap();
        assert!(text.contains(r#""kind":"upload""#), "{text}");
        assert_eq!(serde_json::from_str::<Vec<Hop>>(&text).unwrap(), hops);
    }

    #[test]
    fn the_walk_finds_the_upload_or_says_what_is_broken() {
        let h = "fnv1a64:815e1921c1dd6dd9";
        let hops = trail(h);
        assert_eq!(walk(&hops, Some(h)).unwrap().item, "box511");
        // The card is optional; the upload is not.
        assert!(walk(&hops[1..], Some(h)).is_ok());
        assert_eq!(walk(&hops[2..], Some(h)), Err(Error::NoUpload));
        assert_eq!(walk(&hops, Some("fnv1a64:0")), Err(Error::Unlinked(3)));
        assert_eq!(walk(&hops, None), Err(Error::Unlinked(3)));
        // Each link of the hash chain must hold.
        let mut broken = hops.clone();
        if let What::Derived(d) = &mut broken[3].what {
            d.from = "sha256:22".into();
        }
        assert_eq!(walk(&broken, Some(h)), Err(Error::Unlinked(2)));
        let mut broken = hops.clone();
        if let What::Derived(d) = &mut broken[2].what {
            d.from = "sha256:22".into();
        }
        assert_eq!(walk(&broken, Some(h)), Err(Error::Unlinked(1)));
        let mut broken = hops.clone();
        if let What::Upload(u) = &mut broken[1].what {
            u.original.hash = "sha256:22".into();
        }
        assert_eq!(walk(&broken, Some(h)), Err(Error::Unlinked(1)));
        assert_eq!(walk(&[], Some(h)), Err(Error::Empty));
        let mut twice = hops.clone();
        twice.insert(0, hops[0].clone());
        assert_eq!(walk(&twice, Some(h)), Err(Error::OutOfOrder));
        let mut elsewhere = hops.clone();
        if let What::Upload(u) = &mut elsewhere[1].what {
            u.item = "box512".into();
        }
        assert_eq!(walk(&elsewhere, Some(h)), Err(Error::NotOfItem));
        let mut unknown = hops;
        unknown[0].provenance = Provenance::new(
            Method::Read,
            Agent::Claude,
            Date::new(2026, 10, 26).unwrap(),
        );
        assert_eq!(
            check(&unknown, Some(h)),
            Err(Error::Provenance(provenance::Error::NoEvidence))
        );
    }
}
