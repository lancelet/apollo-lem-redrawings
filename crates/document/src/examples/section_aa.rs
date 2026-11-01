//! Worked examples from the sheets, for tests, benchmarks and
//! documentation. Each builds a sketch through the public interface.

use crate::sketch;
use crate::solver::equation::{arc, circle, line, point, ArcEnd};
use crate::value::geometry::Model;
use crate::value::measure::Measure;
use crate::value::provenance::{Agent, Citation, Date, ItemRef, Method, Provenance};
use crate::value::relation::*;
use crate::value::sketch::Sketch;

const SHEET: &str = "LDW280-23631 sh3";

fn day() -> Date {
    Date::new(2026, 10, 15).unwrap()
}

fn cite(at: &str) -> Citation {
    Citation::new(SHEET, at)
}

fn read() -> Provenance {
    Provenance::new(Method::Read, Agent::Claude, day()).cite(cite("SECT A-A"))
}

fn traced() -> Provenance {
    Provenance::new(Method::Measured { sigma_mm: 0.3 }, Agent::Claude, day())
        .cite(cite("SECT A-A, scan"))
}

fn shown() -> Provenance {
    Provenance::new(Method::Estimated, Agent::Claude, day()).cite(cite("SECT A-A"))
}

fn measured() -> Provenance {
    Provenance::new(Method::Measured { sigma_mm: 0.15 }, Agent::Claude, day())
        .cite(cite("SECT A-A, ink"))
}

fn from_station() -> Provenance {
    let from = vec![ItemRef::Constraint("station".into())];
    Provenance::new(Method::Derived { from }, Agent::Claude, day())
        .cite(cite("SECT A-A, station labels"))
}

fn inch(t: &str) -> Measure {
    Measure::inches(t).unwrap()
}

fn mm(v: f64) -> Measure {
    Measure::mm(v).unwrap()
}

fn deg(v: f64) -> Measure {
    Measure::deg(v).unwrap()
}

/// Section A-A of Grumman LDW280-23631 sheet 3, whole, as traced: both
/// clips with their lugs, the hidden clip, the reamed pin, the +Y27.00
/// plane, the rivet line and the rivets. Model millimetres, model Y along
/// x, model X up the page. Fully constrained; see `tests/section_aa.rs`.
pub fn section_aa() -> Sketch<Model> {
    // Upper clip and its lug.
    let (u_top_o, u_bend_o, u_leg_o, u_leg_end, u_leg_i, u_bend_i, u_top_i, u_flange_end) = (
        line("u_top_o"),
        arc("u_bend_o"),
        line("u_leg_o"),
        line("u_leg_end"),
        line("u_leg_i"),
        arc("u_bend_i"),
        line("u_top_i"),
        line("u_flange_end"),
    );
    let (u_lug_l, u_f1, u_lug_s, u_lug_arc, u_f2, u_lug_r) = (
        line("u_lug_l"),
        arc("u_f1"),
        line("u_lug_s"),
        arc("u_lug_arc"),
        arc("u_f2"),
        line("u_lug_r"),
    );
    // Lower clip and its lug.
    let (l_leg_o, l_bend_o, l_fl_o, l_fl_end, l_fl_i, l_bend_i, l_leg_i, l_leg_end) = (
        line("l_leg_o"),
        arc("l_bend_o"),
        line("l_fl_o"),
        line("l_fl_end"),
        line("l_fl_i"),
        arc("l_bend_i"),
        line("l_leg_i"),
        line("l_leg_end"),
    );
    let (l_lug_l, l_g1, l_lug_s, l_lug_arc, l_g2, l_lug_r) = (
        line("l_lug_l"),
        arc("l_g1"),
        line("l_lug_s"),
        arc("l_lug_arc"),
        arc("l_g2"),
        line("l_lug_r"),
    );
    // The hidden clip behind the pin.
    let (h_leg_o, h_bend_o, h_fl_o, h_leg_i, h_bend_i, h_fl_i, h_leg_end) = (
        line("h_leg_o"),
        arc("h_bend_o"),
        line("h_fl_o"),
        line("h_leg_i"),
        arc("h_bend_i"),
        line("h_fl_i"),
        line("h_leg_end"),
    );
    let (pin, y27, rivet, rv_u, rv_l) = (
        circle("pin"),
        line("y27"),
        line("rivet_line"),
        point("rv_u"),
        point("rv_l"),
    );

    sketch!(Model {
        entities(layer: "visible", provenance: traced()) {
            u_top_o: line (710.005, 4666.488) -> (692.123, 4666.488);
            u_bend_o: arc (692.123, 4662.613), r = 3.886, from = 1.5712, sweep = 1.5703;
            u_leg_o: line (688.237, 4662.613) -> (688.237, 4653.496);
            u_leg_end: line (688.237, 4653.496) -> (689.509, 4653.496);
            u_leg_i: line (689.509, 4653.496) -> (689.510, 4662.613);
            u_bend_i: arc (692.123, 4662.612), r = 2.613, from = 1.5704, sweep = 1.5711;
            u_top_i: line (692.123, 4665.218) -> (710.005, 4665.218);
            u_flange_end: line (710.005, 4665.218) -> (710.005, 4666.488);
            u_lug_l: line (694.055, 4665.218) -> (694.055, 4656.942);
            u_f1: arc (702.176, 4656.942), r = 8.121, from = 3.1409, sweep = 0.4665;
            u_lug_s: line (694.926, 4653.291) -> (699.508, 4644.239);
            u_lug_arc: arc (705.340, 4647.176), r = 6.533, from = 0.8792, sweep = -3.5544;
            u_f2: arc (714.168, 4657.839), r = 7.333, from = 3.1421, sweep = 0.8792;
            u_lug_r: line (706.836, 4657.839) -> (706.836, 4665.218);
            l_leg_o: line (688.002, 4514.453) -> (688.002, 4504.862);
            l_bend_o: arc (691.485, 4504.862), r = 3.483, from = 3.1409, sweep = 1.5712;
            l_fl_o: line (691.485, 4501.388) -> (708.865, 4501.388);
            l_fl_end: line (708.865, 4501.388) -> (708.865, 4502.659);
            l_fl_i: line (708.865, 4502.659) -> (692.154, 4502.659);
            l_bend_i: arc (692.154, 4505.568), r = 2.916, from = 3.1421, sweep = 1.5704;
            l_leg_i: line (689.238, 4505.568) -> (689.238, 4514.453);
            l_leg_end: line (689.238, 4514.453) -> (688.002, 4514.453);
            l_lug_l: line (693.253, 4502.659) -> (693.252, 4511.234);
            l_g1: arc (700.149, 4511.234), r = 6.897, from = -3.1409, sweep = -0.4741;
            l_lug_s: line (694.016, 4514.381) -> (698.464, 4523.002);
            l_lug_arc: arc (704.319, 4519.998), r = 6.584, from = 2.6675, sweep = -3.5904;
            l_g2: arc (711.968, 4509.893), r = 6.111, from = 2.2187, sweep = 0.9229;
            l_lug_r: line (705.857, 4509.893) -> (705.857, 4502.659);
            pin: circle (704.850, 4583.938), r = 3.175;
        }
        entities(layer: "hidden", provenance: traced()) {
            h_leg_o: line (687.846, 4578.218) -> (687.846, 4589.636);
            h_bend_o: arc (690.418, 4589.636), r = 2.572, from = 1.5712, sweep = 1.5703;
            h_fl_o: line (690.418, 4592.202) -> (696.350, 4592.202);
            h_leg_i: line (689.117, 4578.218) -> (689.117, 4588.745);
            h_bend_i: arc (691.310, 4588.745), r = 2.193, from = 1.5704, sweep = 1.5711;
            h_fl_i: line (691.310, 4590.932) -> (695.083, 4590.932);
            h_leg_end: line (687.846, 4578.218) -> (689.117, 4578.218);
        }
        entities(layer: "phantom", provenance: traced()) {
            y27: line (685.799, 4680.467) -> (685.798, 4447.281);
            rivet_line: line (700.077, 4674.021) -> (700.092, 4507.316);
        }
        entities(layer: "centre", provenance: traced()) {
            rv_u: point (700.118, 4347.895);
            rv_l: point (700.094, 4485.051);
        }
        // What the sheet prints.
        constraints(provenance: read()) {
            station: position(pin.centre(), inch("27.750"), inch("180.470"));
            ream: diameter(pin, inch(".2500"));
            d3200: vertical_distance(pin.centre(), u_top_i.end(), inch("3.200"));
            d6400: vertical_distance(l_fl_i.start(), u_top_i.end(), inch("6.400"));
            d188: horizontal_distance(rivet.start(), pin.centre(), inch(".188"));
            d750: horizontal_distance(y27.start(), pin.centre(), inch(".750")).driven().unwrap();
            x183: vertical_distance(pin.centre(), u_top_o.start(), inch("3.250")).driven().unwrap();
            x177: vertical_distance(l_fl_o.end(), pin.centre(), inch("3.250")).driven().unwrap();
        }
        constraints(provenance: from_station()) {
            y27_at: horizontal_distance(y27.start(), pin.centre(), inch(".750"));
        }
        // What the drawing shows: the upper clip.
        constraints(provenance: shown()) {
            uh1: horizontal(u_top_o);
            uh2: horizontal(u_top_i);
            uh3: horizontal(u_leg_end);
            uv1: vertical(u_leg_o);
            uv2: vertical(u_leg_i);
            uv3: vertical(u_flange_end);
            uv4: vertical(u_lug_l);
            uv5: vertical(u_lug_r);
            uj1: tangent_join(u_top_o, LineEnd::End, u_bend_o, ArcEnd::Start);
            uj2: tangent_join(u_leg_o, LineEnd::Start, u_bend_o, ArcEnd::End);
            uc1: coincident(u_leg_o.end(), u_leg_end.start());
            uc2: coincident(u_leg_end.end(), u_leg_i.start());
            uj3: tangent_join(u_leg_i, LineEnd::End, u_bend_i, ArcEnd::End);
            uj4: tangent_join(u_top_i, LineEnd::Start, u_bend_i, ArcEnd::Start);
            uc3: coincident(u_top_i.end(), u_flange_end.start());
            uc4: coincident(u_flange_end.end(), u_top_o.start());
            ucc: concentric(u_bend_o, u_bend_i);
            uo1: on_line(u_lug_l.start(), u_top_i);
            uo2: on_line(u_lug_r.end(), u_top_i);
            uj5: tangent_join(u_lug_l, LineEnd::End, u_f1, ArcEnd::Start);
            uj6: tangent_join(u_lug_s, LineEnd::Start, u_f1, ArcEnd::End);
            uj7: tangent_join(u_lug_s, LineEnd::End, u_lug_arc, ArcEnd::End);
            uj8: tangent_arc_join(u_lug_arc, ArcEnd::Start, u_f2, ArcEnd::End);
            uj9: tangent_join(u_lug_r, LineEnd::Start, u_f2, ArcEnd::Start);
            // The lower clip.
            lv1: vertical(l_leg_o);
            lh1: horizontal(l_fl_o);
            lv2: vertical(l_fl_end);
            lh2: horizontal(l_fl_i);
            lv3: vertical(l_leg_i);
            lh3: horizontal(l_leg_end);
            lv4: vertical(l_lug_l);
            lv5: vertical(l_lug_r);
            lj1: tangent_join(l_leg_o, LineEnd::End, l_bend_o, ArcEnd::Start);
            lj2: tangent_join(l_fl_o, LineEnd::Start, l_bend_o, ArcEnd::End);
            lc1: coincident(l_fl_o.end(), l_fl_end.start());
            lc2: coincident(l_fl_end.end(), l_fl_i.start());
            lj3: tangent_join(l_fl_i, LineEnd::End, l_bend_i, ArcEnd::End);
            lj4: tangent_join(l_leg_i, LineEnd::Start, l_bend_i, ArcEnd::Start);
            lc3: coincident(l_leg_i.end(), l_leg_end.start());
            lc4: coincident(l_leg_end.end(), l_leg_o.start());
            lcc: concentric(l_bend_o, l_bend_i);
            lo1: on_line(l_lug_l.start(), l_fl_i);
            lo2: on_line(l_lug_r.end(), l_fl_i);
            lj5: tangent_join(l_lug_l, LineEnd::End, l_g1, ArcEnd::Start);
            lj6: tangent_join(l_lug_s, LineEnd::Start, l_g1, ArcEnd::End);
            lj7: tangent_join(l_lug_s, LineEnd::End, l_lug_arc, ArcEnd::Start);
            lj8: tangent_arc_join(l_lug_arc, ArcEnd::End, l_g2, ArcEnd::Start);
            lj9: tangent_join(l_lug_r, LineEnd::Start, l_g2, ArcEnd::End);
            // The hidden clip.
            hv1: vertical(h_leg_o);
            hv2: vertical(h_leg_i);
            hh1: horizontal(h_fl_o);
            hh2: horizontal(h_fl_i);
            hh3: horizontal(h_leg_end);
            hj1: tangent_join(h_fl_o, LineEnd::Start, h_bend_o, ArcEnd::Start);
            hj2: tangent_join(h_leg_o, LineEnd::End, h_bend_o, ArcEnd::End);
            hj3: tangent_join(h_fl_i, LineEnd::Start, h_bend_i, ArcEnd::Start);
            hj4: tangent_join(h_leg_i, LineEnd::End, h_bend_i, ArcEnd::End);
            hc1: coincident(h_leg_end.start(), h_leg_o.start());
            hc2: coincident(h_leg_end.end(), h_leg_i.start());
            hcc: concentric(h_bend_o, h_bend_i);
            // The station lines and the rivets on the rivet line.
            pv1: vertical(y27);
            pv2: vertical(rivet);
            po1: on_line(rv_u, rivet);
            po2: on_line(rv_l, rivet);
        }
        // Measured off the ink.
        constraints(provenance: measured()) {
            ut: length(u_flange_end, inch(".050"));
            urb: radius(u_bend_o, mm(3.886));
            ufl: length(u_top_o, mm(17.882));
            ull: length(u_leg_o, mm(9.117));
            ulat: horizontal_distance(y27.start(), u_leg_o.start(), mm(2.438));
            uslope: angle(u_lug_s, u_top_i, deg(63.16));
            ulx1: horizontal_distance(u_top_i.start(), u_lug_l.start(), mm(1.932));
            ulx2: horizontal_distance(u_top_i.start(), u_lug_r.end(), mm(14.713));
            uly1: length(u_lug_l, mm(8.276));
            uly2: length(u_lug_r, mm(7.379));
            urf1: radius(u_f1, mm(8.121));
            urf2: radius(u_f2, mm(7.333));
            urh: radius(u_lug_arc, mm(6.533));
            lt: length(l_fl_end, inch(".050"));
            lrb: radius(l_bend_o, mm(3.483));
            lfl: length(l_fl_o, mm(17.380));
            lll: length(l_leg_o, mm(9.591));
            llat: horizontal_distance(y27.start(), l_leg_o.start(), mm(2.203));
            lslope: angle(l_lug_s, l_fl_o, deg(-62.72));
            llx1: horizontal_distance(l_fl_i.end(), l_lug_l.start(), mm(1.099));
            llx2: horizontal_distance(l_fl_i.end(), l_lug_r.end(), mm(13.703));
            lly1: length(l_lug_l, mm(8.575));
            lly2: length(l_lug_r, mm(7.234));
            lrg1: radius(l_g1, mm(6.897));
            lrg2: radius(l_g2, mm(6.111));
            lrh: radius(l_lug_arc, mm(6.584));
            ht: length(h_leg_end, inch(".050"));
            hrb: radius(h_bend_o, mm(2.572));
            hll: length(h_leg_o, mm(11.418));
            hfo: length(h_fl_o, mm(5.932));
            hfi: length(h_fl_i, mm(3.773));
            hlat: horizontal_distance(y27.start(), h_leg_o.start(), mm(2.047));
            hup: vertical_distance(pin.centre(), h_fl_o.start(), mm(8.264));
            ye: vertical_distance(pin.centre(), y27.start(), mm(96.529));
            yl: length(y27, mm(233.186));
            re: vertical_distance(pin.centre(), rivet.start(), mm(90.083));
            rl: length(rivet, mm(166.705));
            rvu: vertical_distance(pin.centre(), rv_u, mm(-236.043));
            rvl: vertical_distance(pin.centre(), rv_l, mm(-98.887));
        }
    })
}
