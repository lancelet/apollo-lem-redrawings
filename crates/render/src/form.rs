//! A form template drawn on its own: the frame, the centre arrows, the
//! blocks' rules, and the cells' labels and printed lines in our
//! lettering (docs/redraw-plan.md §2.5).
//!
//! Each line of lettering is set at its cap and the form's one width
//! factor, on its measured baseline, its ink held at the measured ink's
//! left edge or centre. A character our lettering has no glyph for is set as `?`, as
//! the export does.

use crate::scene::{Fill, Item, Path, Plane, PlaneKind, Primitive, Run, Source};
use crate::text::{ink_bounds, layout, strokes, Frame};
use lmsheets_document::state::document::Document;
use lmsheets_document::value::form::{Align, FormItem, FormPlacement, FormTemplate, Lettered};
use lmsheets_document::value::geometry::{Paper, P2};
use lmsheets_document::value::lettering::Lettering;
use lmsheets_document::value::provenance::{Agent, Date, Method, Provenance};
use lmsheets_document::value::sketch::LayerId;
use lmsheets_document::value::style::{Dash, Pen, Purpose};
use lmsheets_document::value::text::{Anchor, BoxPoint, Size, Text};

/// The layer every printed item of the form is drawn on.
pub const LAYER: &str = "form";

/// The template drawn on its own paper: one plane, the frame first, then
/// the arrows, then each block's rules and lettering.
pub fn draw(t: &FormTemplate) -> Plane {
    let mut items = Vec::new();
    let mut runs = Vec::new();
    printed(t, FormPlacement::IDENTITY, &mut items, &mut runs);
    Plane {
        kind: PlaneKind::Drawing,
        items,
        runs,
        terminations: Vec::new(),
        witnesses: Vec::new(),
    }
}

/// The document's placed form, when the format layer is on for the
/// purpose: its printed items from its template, placed, then each
/// filled field's entry, set as a text on its own layer.
pub fn items(doc: &Document, purpose: Purpose, out: &mut Vec<Item>, runs: &mut Vec<Run>) {
    let Some(f) = doc.form() else {
        return;
    };
    let on = |layer: LayerId| doc.layer(layer).is_some_and(|l| l.visible.is_on(purpose));
    if on(LayerId::new("format")) {
        let t = f.template().expect("the document checks its form");
        printed(&t, f.placement, out, runs);
    }
    let font = Document::font();
    for (field, text) in f.entries() {
        let layer = doc
            .layer(text.layer())
            .expect("the document checks its layers");
        if !layer.visible.is_on(purpose) {
            continue;
        }
        let lmsheets_document::value::text::Placement::Paper { point } = text.at() else {
            unreachable!("the document checks a field is placed on the paper")
        };
        let (layout, frame) = crate::text::place(doc, text, point);
        for set in crate::text::runs(text, &layout, &frame) {
            runs.push(Run {
                source: Source::Field(field),
                text: set.line.to_plain(),
                line: set.line,
                origin: set.origin,
                cap: set.cap,
                width: set.width,
                anchor: set.anchor,
                angle: text.angle(),
                weight: layout.weight,
            });
        }
        for path in crate::text::strokes(text, &layout, &frame, font) {
            out.push(Item {
                primitive: Primitive::Stroke {
                    path,
                    weight: layout.weight,
                    dash: layer.style.dash.clone(),
                    anchors: Vec::new(),
                },
                layer: text.layer(),
                tint: layer.tint,
                source: Source::Field(field),
            });
        }
    }
}

/// The template's printed items, each naming its [`FormItem`], placed
/// on the sheet's paper.
fn printed(t: &FormTemplate, placement: FormPlacement, items: &mut Vec<Item>, runs: &mut Vec<Run>) {
    let on = |p: [f64; 2]| placement.apply(t.to_paper(p));
    let mut push = |primitive, item| {
        items.push(Item {
            primitive,
            layer: LayerId::new(LAYER),
            tint: None,
            source: Source::Form(item),
        })
    };
    let line = |a: P2<Paper>, b: P2<Paper>, weight: f64| Primitive::Stroke {
        path: Path::Line { a, b },
        weight,
        dash: Dash::SOLID,
        anchors: Vec::new(),
    };

    for side in 0..4 {
        let (a, b) = t.line(FormItem::Frame(side)).expect("a side");
        push(line(on(a), on(b), t.frame.weight), FormItem::Frame(side));
    }

    // Each arrow: its tip at the middle of a side, pointing inward; its
    // stalk from the tip outward to its reach.
    let a = &t.centre_arrows;
    for (k, (tip, out, reach)) in t.arrows().into_iter().enumerate() {
        let at = |along: f64, across: f64| {
            on([
                tip[0] + out[0] * along - out[1] * across,
                tip[1] + out[1] * along + out[0] * across,
            ])
        };
        let half = a.head_width / 2.0;
        let head = vec![
            at(0.0, 0.0),
            at(a.head_length, half),
            at(a.head_length, -half),
        ];
        push(
            Primitive::Fill {
                fill: Fill::Polygon { points: head },
                outline: None,
            },
            FormItem::CentreArrow(k),
        );
        push(
            line(at(0.0, 0.0), at(reach, 0.0), a.stalk_width),
            FormItem::CentreArrow(k),
        );
    }

    let lettering = Lettering::standard();
    for (block, b) in t.blocks.iter().enumerate() {
        for (index, rule) in b.rules.iter().enumerate() {
            push(
                line(on(rule.from), on(rule.to), rule.weight),
                FormItem::Rule { block, index },
            );
        }
    }
    for item in t.items().into_iter().filter(|i| i.is_lettering()) {
        let l = t.lettered(item).expect("an item of the template");
        let (paths, run) = letter(t, l, &lettering, placement);
        for path in paths {
            push(path, item);
        }
        runs.push(Run {
            source: Source::Form(item),
            ..run
        });
    }
}

/// One line of lettering at its cap and the form's width factor, in
/// the standard pen: on its measured baseline, its ink's left edge or
/// centre (as it is aligned) on the measured ink's; placed, with its
/// run (its source for the caller to set).
fn letter(
    t: &FormTemplate,
    l: &Lettered,
    lettering: &Lettering,
    placement: FormPlacement,
) -> (Vec<Primitive>, Run) {
    let font = Document::font();
    let set: String = l
        .text
        .chars()
        .map(|c| if font.glyph(c).is_some() { c } else { '?' })
        .collect();
    // The text exists only to be laid out; the template is its record.
    let provenance = Provenance::new(
        Method::Authored,
        Agent::Claude,
        Date::new(2026, 10, 27).expect("a date"),
    );
    let at = t.to_paper([(l.extent[0] + l.extent[1]) / 2.0, l.baseline]);
    let text = Text::new(&[&set], "form", at, provenance)
        .and_then(|x| x.sized(Size::Cap { mm: l.cap }))
        .and_then(|x| x.width_factor(l.width_factor.unwrap_or(t.width_factor)))
        .and_then(|x| match l.weight {
            Some(w) => x.weight(w),
            None => Ok(x),
        })
        .and_then(|x| {
            x.anchored(Anchor::Box {
                at: BoxPoint::BottomCentre,
            })
        })
        .expect("a template's lettering is one checked line");
    let layout = layout(&text, lettering, Pen::STANDARD, font);
    let anchor = layout.anchor(text.anchor());
    // The box is not the ink (side bearings): move the line by the
    // difference, on the template's paper; then place it.
    let ink = ink_bounds(&text, &layout, &Frame::new(at, anchor, 0.0), font)
        .expect("a printed line has ink");
    let dx = match l.align {
        Align::Left => t.to_paper([l.extent[0], 0.0]).x - ink.min().x,
        Align::Centre => at.x - (ink.min().x + ink.max().x) / 2.0,
    };
    // Paper y runs down, a text's angle counter-clockwise on the page.
    let angle = -placement.rotation;
    let frame = Frame::new(placement.apply(P2::new(at.x + dx, at.y)), anchor, angle);
    let set = crate::text::runs(&text, &layout, &frame)
        .into_iter()
        .next()
        .expect("one line");
    let run = Run {
        source: Source::Form(FormItem::Frame(0)),
        text: set.line.to_plain(),
        line: set.line,
        origin: set.origin,
        cap: set.cap,
        width: set.width,
        anchor: set.anchor,
        angle,
        weight: layout.weight,
    };
    let paths = strokes(&text, &layout, &frame, font)
        .into_iter()
        .map(|path| Primitive::Stroke {
            path,
            weight: layout.weight,
            dash: Dash::SOLID,
            anchors: Vec::new(),
        })
        .collect();
    (paths, run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmsheets_document::value::geometry::Rect;

    /// The record's numbers, inches from the frame's top-left corner,
    /// copied from apollo-lunar-module/docs/drawing-survey/forms/
    /// ENG-73.1_6-64.json by hand (not through the converter), each rule
    /// as (x0, y0, x1, y1).
    const RECORD_RULES_IN: [[f64; 4]; 13] = [
        // Title block: top, row 1 bottom, row 2 bottom, left.
        [10.938, 8.818, 16.193, 8.818],
        [10.938, 9.214, 16.193, 9.214],
        [10.938, 9.941, 16.193, 9.941],
        [10.938, 8.818, 10.938, 10.207],
        // Row 2's columns, then row 3's.
        [11.443, 9.214, 11.443, 9.941],
        [12.684, 9.214, 12.684, 9.941],
        [15.675, 9.214, 15.675, 9.941],
        [12.184, 9.941, 12.184, 10.207],
        [14.434, 9.941, 14.434, 10.207],
        // Operations block: left, heading bottom, note bottom, check cell.
        [12.8, 0.0, 12.8, 1.835],
        [12.8, 0.839, 16.193, 0.839],
        [12.8, 1.835, 16.193, 1.835],
        [15.767, 0.839, 15.767, 1.835],
    ];

    #[test]
    fn the_rules_are_where_the_record_puts_them() {
        let t = FormTemplate::builtin("ENG-73.1_6-64").unwrap();
        let plane = draw(&t);
        let o = t.frame.origin;
        let drawn: Vec<[f64; 4]> = plane
            .items
            .iter()
            .filter(|i| matches!(i.source, Source::Form(FormItem::Rule { .. })))
            .map(|i| match &i.primitive {
                Primitive::Stroke {
                    path: Path::Line { a, b },
                    ..
                } => [a.x - o[0], a.y - o[1], b.x - o[0], b.y - o[1]].map(|v| v / 25.4),
                p => panic!("a rule drawn as {p:?}"),
            })
            .collect();
        // The record's rules, then the box-X's six, measured.
        assert_eq!(drawn.len(), RECORD_RULES_IN.len() + 6);
        for (d, r) in drawn.iter().zip(RECORD_RULES_IN) {
            for (d, r) in d.iter().zip(r) {
                assert!((d - r).abs() < 1e-9, "{d:?} drawn for {r:?}");
            }
        }
        // The frame's corners: the record's size, from its origin.
        let frame: Vec<_> = plane
            .items
            .iter()
            .filter(|i| matches!(i.source, Source::Form(FormItem::Frame(_))))
            .collect();
        assert_eq!(frame.len(), 4);
        let Primitive::Stroke {
            path: Path::Line { b, .. },
            ..
        } = &frame[1].primitive
        else {
            panic!("a frame side not a line")
        };
        assert!(((b.x - o[0]) / 25.4 - 16.193).abs() < 1e-9);
        assert!(((b.y - o[1]) / 25.4 - 10.207).abs() < 1e-9);
    }

    #[test]
    fn lettering_sits_on_its_measured_baselines() {
        let t = FormTemplate::builtin("ENG-73.1_6-64").unwrap();
        let o = t.frame.origin;
        let printed: Vec<_> = t.blocks.iter().flat_map(|b| &b.printed).collect();
        // A few measured baselines, inches, as the converter wrote them.
        let find = |s: &str| *printed.iter().find(|l| l.text.starts_with(s)).unwrap();
        for (text, baseline) in [
            ("GRUMMAN", 9.0854),
            ("MANUFACTURING", 0.286),
            ("USE BOTH", 1.7142),
            ("ENG 73.1", 10.3075),
        ] {
            assert!(
                (find(text).baseline / 25.4 - baseline).abs() < 5e-4,
                "{text}"
            );
        }
        // The note lines are the record's pitch apart (0.191 in), to
        // the measuring's hundredth.
        let notes: Vec<f64> = ["RUBBER", "AND ALL", "IN REGION", "USE BOTH"]
            .map(|s| find(s).baseline / 25.4)
            .to_vec();
        for w in notes.windows(2) {
            assert!((w[1] - w[0] - 0.191).abs() < 0.011, "{w:?}");
        }
        // Drawn: each line's ink bottom on its baseline, within its pen
        // and its ink held where the measured ink is.
        for l in &printed {
            let ink = Rect::union_all(
                letter(&t, l, &Lettering::standard(), FormPlacement::IDENTITY)
                    .0
                    .iter()
                    .filter_map(|p| p.ink_bounds()),
            )
            .unwrap();
            let pen = l.weight.unwrap_or_else(|| Pen::STANDARD.width(l.cap));
            // (A comma reaches below the baseline.)
            if !l.text.contains(',') {
                assert!((ink.max().y - o[1] - l.baseline).abs() < pen, "{}", l.text);
            }
            let (drawn, measured) = match l.align {
                Align::Left => (ink.min().x, l.extent[0]),
                Align::Centre => (
                    (ink.min().x + ink.max().x) / 2.0,
                    (l.extent[0] + l.extent[1]) / 2.0,
                ),
            };
            assert!((drawn - o[0] - measured).abs() < 1e-9, "{}", l.text);
        }
    }

    #[test]
    fn every_printed_item_is_drawn_and_named() {
        let t = FormTemplate::builtin("ENG-73.1_6-64").unwrap();
        let plane = draw(&t);
        let drawn = |item| {
            plane
                .items
                .iter()
                .filter(|i| i.source == Source::Form(item))
                .count()
        };
        // Each centre arrow is its head and its stalk.
        for k in 0..4 {
            assert_eq!(drawn(FormItem::CentreArrow(k)), 2);
        }
        // Every item of the template is drawn, under its own name, and
        // each lettered one has its run.
        for item in t.items() {
            assert!(drawn(item) > 0, "{}", item.name());
            assert_eq!(FormItem::parse(&item.name()), Some(item));
            assert_eq!(
                plane
                    .runs
                    .iter()
                    .filter(|r| r.source == Source::Form(item))
                    .count(),
                usize::from(item.is_lettering()),
                "{}",
                item.name()
            );
        }
    }
}
