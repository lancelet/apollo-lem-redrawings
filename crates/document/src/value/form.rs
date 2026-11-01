//! A drawing form as printed: the template library of
//! docs/redraw-plan.md §2.5.
//!
//! A [`FormTemplate`] is one edition of a printed form (Grumman's
//! ENG-73.1 6-64, the B sheet): its paper, its frame, the centre arrows,
//! and its blocks, each with its rules, its cells (a box, the field
//! entered in it if it is an entry, and its printed label) and its
//! printed lines with their cap heights, baselines and extents, and one
//! width factor for its lettering. It is converted from the drawing
//! survey's record of the form by tools/form-template.py, which
//! measures what the record lacks (the lettering's places and width,
//! the box-X) from the record's own crops; it is identified by its edition and a hash of its
//! content ([`FormTemplate::hash`]).
//!
//! Lengths are paper mm. Points are mm from the frame's top-left corner
//! (the frame line's centre), x right, y down, as the record's inches
//! are; [`FormTemplate::to_paper`] adds the frame's origin on the paper.
//!
//! **A form placed on a sheet** ([`Form`]) names its template by edition
//! and hash, places the template's paper on the sheet's (normally the
//! identity) and holds its fields: each field cell's entry as read, a
//! [`Text`], or empty. The printed parts are drawn from the template and
//! are not stored again; a claim on printed ink names a [`FormItem`].

use crate::value::geometry::{Paper, P2};
use crate::value::provenance::Provenance;
use crate::value::text::{Placement as TextPlacement, Text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

id! {
    /// A field of a form, by the name its cells give it (`size`,
    /// `drawing_number`).
    FieldId
}

/// The templates in this crate, by edition id.
const LIBRARY: [(&str, &str); 1] = [(
    "ENG-73.1_6-64",
    include_str!("../../forms/ENG-73.1_6-64.json"),
)];

/// A form's edition: the form number with its date, as Grumman numbers
/// its forms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edition {
    /// The id, `ENG-73.1_6-64`: the file's name in the library.
    pub id: String,
    /// The form number as printed, `ENG 73.1 6-64`.
    pub printed: String,
}

/// The paper the form is printed on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaperSize {
    /// Its letter, `B`.
    pub size: String,
    /// Width and height, mm.
    pub mm: [f64; 2],
}

/// The frame: a rectangle of one line weight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormFrame {
    /// Its top-left corner on the paper, mm from the paper's top-left.
    pub origin: [f64; 2],
    /// Width and height between the line's centres, mm.
    pub size: [f64; 2],
    /// The line's weight, mm.
    pub weight: f64,
}

/// One value per side of the frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sides {
    /// The top side.
    pub top: f64,
    /// The bottom side.
    pub bottom: f64,
    /// The left side.
    pub left: f64,
    /// The right side.
    pub right: f64,
}

/// The filled arrows at the middle of each side of the frame, their
/// tips on the frame line, pointing inward, each on a stalk running
/// outward.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CentreArrows {
    /// The head's length along the stalk, mm.
    pub head_length: f64,
    /// The head's width across its base, mm.
    pub head_width: f64,
    /// The stalk's width, mm.
    pub stalk_width: f64,
    /// How far each stalk reaches outward from the frame line, mm.
    pub stalk_reach: Sides,
}

/// A printed straight rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// The record's name for it, `SIZE | CODE IDENT NO.`.
    pub name: String,
    /// One end, mm in the frame.
    pub from: [f64; 2],
    /// The other end.
    pub to: [f64; 2],
    /// Its weight, mm.
    pub weight: f64,
}

/// A printed line of lettering, placed as the original prints it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lettered {
    /// The text as printed.
    pub text: String,
    /// Its cap height, mm.
    pub cap: f64,
    /// Its baseline, mm down the frame (measured).
    pub baseline: f64,
    /// Its ink's left and right edges, mm across the frame (measured).
    pub extent: [f64; 2],
    /// Which of them holds it when our lettering's width differs.
    pub align: Align,
    /// Its own width factor where the print is not the form's one (a
    /// clogged line spread wider than the rest), else the form's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_factor: Option<f64>,
    /// Its own stroke, mm, where the print is heavier than the standard
    /// pen's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

/// Where a printed line is held along its measured ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// At its left edge.
    Left,
    /// At its centre.
    Centre,
}

/// A cell of a block: a box, and the field entered in it if it is an
/// entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// Its box, `[x0, y0, x1, y1]` mm in the frame, between its rules'
    /// centres.
    #[serde(rename = "box")]
    pub area: [f64; 4],
    /// The field a drafter enters in it (`size`, `drawing_number`,
    /// `sheet`); none for a cell left blank on the form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Its printed label, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Lettered>,
}

/// A block of the form: the title block, the operations block, the form
/// number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    /// The record's name for it.
    pub name: String,
    /// Its rules; the frame's sides are not repeated here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    /// Its cells other than printed ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Cell>,
    /// Its printed lines other than the cells' labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub printed: Vec<Lettered>,
}

/// One edition of a printed drawing form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormTemplate {
    /// The edition.
    pub edition: Edition,
    /// The paper.
    pub paper: PaperSize,
    /// The frame.
    pub frame: FormFrame,
    /// The centre arrows.
    pub centre_arrows: CentreArrows,
    /// The pen, a share of the cap, that the lettering's measurements
    /// assumed (the render's standard pen when converted).
    pub pen_of_cap: f64,
    /// The width factor of all its printed lettering in our font (one
    /// for the form, measured).
    pub width_factor: f64,
    /// The blocks.
    pub blocks: Vec<Block>,
    /// The record it was converted from.
    pub source: String,
    /// How it was converted: derivations, and what the record lacked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// What can be wrong with a template, or with a form placed from one.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The JSON does not parse as a template: where and why.
    Parse(String),
    /// A length that is not finite and positive, or a point that is not
    /// finite, at this place.
    BadValue(String),
    /// No template of this edition in the library.
    Unknown(String),
    /// A placed form's hash is not its edition's in the library.
    Hash {
        /// The library's.
        library: String,
        /// The form's.
        form: String,
    },
    /// A placed form's fields are not its template's: each field once,
    /// filled or empty.
    Fields,
    /// A field's entry is not placed on the paper.
    FieldPlacement(FieldId),
    /// A placement that is not finite.
    Placement,
}

impl FormTemplate {
    /// The library's template of this edition.
    pub fn builtin(edition: &str) -> Result<Self, Error> {
        let (_, json) = LIBRARY
            .iter()
            .find(|(id, _)| *id == edition)
            .ok_or_else(|| Error::Unknown(edition.into()))?;
        Self::parse(json)
    }

    /// A template from its JSON, checked.
    pub fn parse(json: &str) -> Result<Self, Error> {
        let de = &mut serde_json::Deserializer::from_str(json);
        let t: Self =
            serde_path_to_error::deserialize(de).map_err(|e| Error::Parse(e.to_string()))?;
        t.check()?;
        Ok(t)
    }

    /// Its JSON, compact.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a template serialises")
    }

    /// The content hash, `fnv1a64:` of the compact JSON of what defines
    /// the form (its geometry, lettering and fields), leaving out its
    /// provenance (`source`, `notes`), so rewording a note does not
    /// change the identity: with the edition, what a document names the
    /// template by.
    pub fn hash(&self) -> String {
        let mut v = serde_json::to_value(self).expect("a template serialises");
        let map = v.as_object_mut().expect("a template is an object");
        map.remove("source");
        map.remove("notes");
        let h = canonical(&v)
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            });
        format!("fnv1a64:{h:016x}")
    }

    /// Every length positive and every point finite.
    pub fn check(&self) -> Result<(), Error> {
        let length = |v: f64, at: &str| {
            (v.is_finite() && v > 0.0)
                .then_some(())
                .ok_or_else(|| Error::BadValue(at.into()))
        };
        let point = |p: &[f64], at: &str| {
            p.iter()
                .all(|v| v.is_finite())
                .then_some(())
                .ok_or_else(|| Error::BadValue(at.into()))
        };
        let [pw, ph] = self.paper.mm;
        length(pw, "paper")?;
        length(ph, "paper")?;
        point(&self.frame.origin, "frame origin")?;
        length(self.frame.size[0], "frame")?;
        length(self.frame.size[1], "frame")?;
        length(self.frame.weight, "frame weight")?;
        let a = &self.centre_arrows;
        let s = &a.stalk_reach;
        length(self.width_factor, "width factor")?;
        length(self.pen_of_cap, "pen")?;
        for v in [a.head_length, a.head_width, a.stalk_width] {
            length(v, "centre arrows")?;
        }
        for v in [s.top, s.bottom, s.left, s.right] {
            length(v, "stalk reach")?;
        }
        for b in &self.blocks {
            for r in &b.rules {
                point(&r.from, &r.name)?;
                point(&r.to, &r.name)?;
                length(r.weight, &r.name)?;
            }
            for c in &b.cells {
                point(&c.area, &b.name)?;
            }
            for l in b
                .cells
                .iter()
                .filter_map(|c| c.label.as_ref())
                .chain(&b.printed)
            {
                length(l.cap, &l.text)?;
                for v in [l.width_factor, l.weight].into_iter().flatten() {
                    length(v, &l.text)?;
                }
                point(&[l.baseline, l.extent[0], l.extent[1]], &l.text)?;
                length(l.extent[1] - l.extent[0], &l.text)?;
            }
        }
        Ok(())
    }

    /// A point of the form (mm in the frame) on the paper.
    pub fn to_paper(&self, p: [f64; 2]) -> P2<Paper> {
        P2::new(self.frame.origin[0] + p[0], self.frame.origin[1] + p[1])
    }

    /// Its fields, each once, in the order of its cells.
    pub fn fields(&self) -> Vec<FieldId> {
        let mut out = Vec::new();
        for f in self
            .blocks
            .iter()
            .flat_map(|b| &b.cells)
            .filter_map(|c| c.field.as_deref())
        {
            let f = FieldId::new(f);
            if !out.contains(&f) {
                out.push(f);
            }
        }
        out
    }

    /// Every printed item, in drawing order: the frame's sides, the
    /// arrows, then each block's rules, labels and printed lines.
    pub fn items(&self) -> Vec<FormItem> {
        let mut out: Vec<FormItem> = (0..4).map(FormItem::Frame).collect();
        out.extend((0..4).map(FormItem::CentreArrow));
        for (block, b) in self.blocks.iter().enumerate() {
            out.extend((0..b.rules.len()).map(|index| FormItem::Rule { block, index }));
            out.extend(
                b.cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.label.is_some())
                    .map(|(cell, _)| FormItem::Label { block, cell }),
            );
            out.extend((0..b.printed.len()).map(|line| FormItem::Printed { block, line }));
        }
        out
    }

    /// Whether it prints this item.
    pub fn has(&self, item: FormItem) -> bool {
        let block = |b: usize| self.blocks.get(b);
        match item {
            FormItem::Frame(s) | FormItem::CentreArrow(s) => s < 4,
            FormItem::Rule { block: b, index } => block(b).is_some_and(|b| index < b.rules.len()),
            FormItem::Label { block: b, cell } => {
                block(b).is_some_and(|b| b.cells.get(cell).is_some_and(|c| c.label.is_some()))
            }
            FormItem::Printed { block: b, line } => {
                block(b).is_some_and(|b| line < b.printed.len())
            }
        }
    }

    /// A printed line of lettering: a cell's label or a block's line.
    pub fn lettered(&self, item: FormItem) -> Option<&Lettered> {
        match item {
            FormItem::Label { block, cell } => {
                self.blocks.get(block)?.cells.get(cell)?.label.as_ref()
            }
            FormItem::Printed { block, line } => self.blocks.get(block)?.printed.get(line),
            _ => None,
        }
    }

    /// A straight printed line, its ends mm in the frame: a side of the
    /// frame, a centre arrow's stalk (from its tip outward), a rule.
    pub fn line(&self, item: FormItem) -> Option<([f64; 2], [f64; 2])> {
        let [w, h] = self.frame.size;
        match item {
            FormItem::Frame(s) => {
                let c = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
                (s < 4).then(|| (c[s], c[(s + 1) % 4]))
            }
            FormItem::CentreArrow(s) => {
                let (tip, out, reach) = *self.arrows().get(s)?;
                Some((tip, [tip[0] + out[0] * reach, tip[1] + out[1] * reach]))
            }
            FormItem::Rule { block, index } => {
                let rule = self.blocks.get(block)?.rules.get(index)?;
                Some((rule.from, rule.to))
            }
            _ => None,
        }
    }

    /// Each centre arrow's tip (mm in the frame, the middle of a side),
    /// its outward direction and its stalk's reach: top, bottom, left,
    /// right.
    pub fn arrows(&self) -> [([f64; 2], [f64; 2], f64); 4] {
        let [w, h] = self.frame.size;
        let r = &self.centre_arrows.stalk_reach;
        [
            ([w / 2.0, 0.0], [0.0, -1.0], r.top),
            ([w / 2.0, h], [0.0, 1.0], r.bottom),
            ([0.0, h / 2.0], [-1.0, 0.0], r.left),
            ([w, h / 2.0], [1.0, 0.0], r.right),
        ]
    }
}

/// A JSON value's compact text with every object's keys sorted: the
/// same whatever order a build's `serde_json` keeps keys in (its
/// `preserve_order` feature, which another crate in the build may turn
/// on, keeps a struct's field order instead of sorting).
fn canonical(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), canonical(&m[k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// One printed item of a form, what a claim on its printed ink names
/// ([`FormItem::name`]). Indices are the template's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormItem {
    /// A side of the frame: 0 top, 1 right, 2 bottom, 3 left.
    Frame(usize),
    /// A centre arrow, its head and stalk: 0 top, 1 bottom, 2 left, 3
    /// right.
    CentreArrow(usize),
    /// A block's rule.
    Rule {
        /// The block.
        block: usize,
        /// The rule in it.
        index: usize,
    },
    /// A cell's printed label.
    Label {
        /// The block.
        block: usize,
        /// The cell in it.
        cell: usize,
    },
    /// A block's printed line of lettering.
    Printed {
        /// The block.
        block: usize,
        /// The line in it.
        line: usize,
    },
}

impl FormItem {
    /// Its name: `frame/0`, `arrow/1`, `rule/0/3`, `label/0/1`,
    /// `printed/1/4`.
    pub fn name(self) -> String {
        match self {
            FormItem::Frame(s) => format!("frame/{s}"),
            FormItem::CentreArrow(s) => format!("arrow/{s}"),
            FormItem::Rule { block, index } => format!("rule/{block}/{index}"),
            FormItem::Label { block, cell } => format!("label/{block}/{cell}"),
            FormItem::Printed { block, line } => format!("printed/{block}/{line}"),
        }
    }

    /// The item a name names ([`FormItem::name`]).
    pub fn parse(name: &str) -> Option<Self> {
        let mut it = name.split('/');
        let kind = it.next()?;
        let n: Vec<usize> = it.map(|v| v.parse().ok()).collect::<Option<_>>()?;
        Some(match (kind, n.as_slice()) {
            ("frame", [s]) => FormItem::Frame(*s),
            ("arrow", [s]) => FormItem::CentreArrow(*s),
            ("rule", [block, index]) => FormItem::Rule {
                block: *block,
                index: *index,
            },
            ("label", [block, cell]) => FormItem::Label {
                block: *block,
                cell: *cell,
            },
            ("printed", [block, line]) => FormItem::Printed {
                block: *block,
                line: *line,
            },
            _ => return None,
        })
    }

    /// Whether it is lettering (a label or a printed line).
    pub fn is_lettering(self) -> bool {
        matches!(self, FormItem::Label { .. } | FormItem::Printed { .. })
    }
}

/// Where a template's paper lies on the sheet's: turned about the
/// paper's origin, then moved. Normally the identity: the form is the
/// sheet's paper.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormPlacement {
    /// The move, mm.
    pub offset_mm: [f64; 2],
    /// The turn, radians, from the paper's x towards its y.
    pub rotation: f64,
}

impl FormPlacement {
    /// The form's paper is the sheet's.
    pub const IDENTITY: Self = Self {
        offset_mm: [0.0, 0.0],
        rotation: 0.0,
    };

    /// A point of the sheet's paper on the template's: the inverse.
    pub fn unapply(&self, p: P2<Paper>) -> P2<Paper> {
        let (s, c) = self.rotation.sin_cos();
        let (x, y) = (p.x - self.offset_mm[0], p.y - self.offset_mm[1]);
        P2::new(c * x + s * y, -s * x + c * y)
    }

    /// A point of the template's paper on the sheet's.
    pub fn apply(&self, p: P2<Paper>) -> P2<Paper> {
        let (s, c) = self.rotation.sin_cos();
        P2::new(
            c * p.x - s * p.y + self.offset_mm[0],
            s * p.x + c * p.y + self.offset_mm[1],
        )
    }
}

/// A printed form placed on a sheet, with its fields ([module](self)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Form {
    /// The template's edition, its id in the library (`ENG-73.1_6-64`).
    pub edition: String,
    /// The template's hash ([`FormTemplate::hash`]).
    pub hash: String,
    /// Where the template's paper lies on the sheet's.
    pub placement: FormPlacement,
    /// The printed parts' provenance: the printed form, drawn from the
    /// template and registered to its ink.
    pub printed: Provenance,
    /// Every field of the template, its entry as read or empty.
    pub fields: BTreeMap<FieldId, Option<Text>>,
}

impl Form {
    /// The library's template this form names, its hash checked.
    pub fn template(&self) -> Result<FormTemplate, Error> {
        let t = FormTemplate::builtin(&self.edition)?;
        if t.hash() != self.hash {
            return Err(Error::Hash {
                library: t.hash(),
                form: self.hash.clone(),
            });
        }
        Ok(t)
    }

    /// Valid: its template known by edition and hash, its placement
    /// finite, its fields exactly the template's, each entry on the
    /// paper. Gives the template.
    pub fn check(&self) -> Result<FormTemplate, Error> {
        let t = self.template()?;
        let p = &self.placement;
        if !(p.offset_mm.iter().all(|v| v.is_finite()) && p.rotation.is_finite()) {
            return Err(Error::Placement);
        }
        let mut theirs = t.fields();
        theirs.sort();
        if !self.fields.keys().copied().eq(theirs) {
            return Err(Error::Fields);
        }
        if let Some((f, _)) = self
            .entries()
            .find(|(_, x)| !matches!(x.at(), TextPlacement::Paper { .. }))
        {
            return Err(Error::FieldPlacement(f));
        }
        Ok(t)
    }

    /// A field's entry, as plain text, its lines joined by newlines;
    /// none when it is empty or not a field.
    pub fn value(&self, field: &str) -> Option<String> {
        let text = self.fields.get(&FieldId::new(field))?.as_ref()?;
        Some(
            text.lines()
                .iter()
                .map(|l| l.to_plain())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// The filled fields' entries.
    pub fn entries(&self) -> impl Iterator<Item = (FieldId, &Text)> {
        self.fields
            .iter()
            .filter_map(|(f, t)| t.as_ref().map(|t| (*f, t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::style::Pen;

    const ID: &str = "ENG-73.1_6-64";

    #[test]
    fn the_template_round_trips() {
        let t = FormTemplate::builtin(ID).unwrap();
        assert_eq!(t.edition.id, ID);
        assert_eq!(FormTemplate::parse(&t.to_json()).unwrap(), t);
        let pretty = serde_json::to_string_pretty(&t).unwrap();
        assert_eq!(FormTemplate::parse(&pretty).unwrap(), t);
    }

    #[test]
    fn the_hash_is_stable() {
        let t = FormTemplate::builtin(ID).unwrap();
        assert_eq!(t.hash(), FormTemplate::parse(&t.to_json()).unwrap().hash());
        // Changing the template changes it; the library's file does not
        // change without the hash below changing with it.
        assert_eq!(t.hash(), "fnv1a64:403185af885dd6cd");
        let mut moved = t.clone();
        moved.blocks[0].rules[0].weight += 1e-9;
        assert_ne!(moved.hash(), t.hash());
        // Its provenance is not its identity.
        let mut renoted = t.clone();
        renoted.notes.push("reworded".into());
        renoted.source.push('!');
        assert_eq!(renoted.hash(), t.hash());
    }

    #[test]
    fn the_measurements_assumed_the_standard_pen() {
        // tools/form-template.py measures the width factor for our
        // standard pen; a new pen needs the template converted again.
        let t = FormTemplate::builtin(ID).unwrap();
        assert_eq!(
            Pen::OfCap {
                share: t.pen_of_cap
            },
            Pen::STANDARD
        );
    }

    #[test]
    fn a_bad_template_is_refused() {
        let mut t = FormTemplate::builtin(ID).unwrap();
        t.frame.weight = 0.0;
        assert!(matches!(
            FormTemplate::parse(&t.to_json()),
            Err(Error::BadValue(_))
        ));
        assert!(matches!(
            FormTemplate::builtin("ENG-0"),
            Err(Error::Unknown(_))
        ));
    }
}
