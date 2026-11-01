//! The name of the node a scene source belongs to: the one rule shared by
//! blobsketch's coverage check (which writes the bundle's owner map and its
//! node table) and the inspector's pick (which reads them), so the two
//! panes speak one namespace.

use crate::scene::{Plane, Source};
use lmsheets_document::state::document::Document;

/// The node a scene source belongs to, as `(kind, name)`.
///
/// A marking region owns its rectangle's and diagonals' lines. A witness
/// line is named by the sorted dimensions that use it, joined by commas.
pub fn node_key(plane: &Plane, doc: &Document, s: Source) -> (String, String) {
    let n = |kind: &str, name: &str| (kind.to_string(), name.to_string());
    match s {
        Source::Sheet(e) => n("entity", e.as_str()),
        Source::View(v, e) => match doc.marking_region_of(v.as_str(), e.as_str()) {
            Some((id, _)) => n("marking_region", id.as_str()),
            None => n("entity", &format!("{}/{}", v.as_str(), e.as_str())),
        },
        Source::Text(t) => n("text", t.as_str()),
        Source::Enclosure(t) => n("enclosure", t.as_str()),
        Source::Leader(l) => n("leader", l.as_str()),
        Source::Dimension(d, _) => n("dimension", d.as_str()),
        Source::CentreMark(m) => n("centre_mark", m.as_str()),
        Source::CentreLine(l) => n("centre_line", l.as_str()),
        Source::Unmodelled(u) => n("unmodelled", u.as_str()),
        Source::Observed(o) => n("observed", o.as_str()),
        Source::Form(item) => n("form", &item.name()),
        Source::Field(f) => n("field", f.as_str()),
        Source::Witness(w) => n("witness", &{
            let mut users: Vec<&str> = plane.witnesses[w.0 as usize]
                .users
                .iter()
                .map(|d| d.as_str())
                .collect();
            users.sort_unstable();
            users.join(",")
        }),
    }
}
