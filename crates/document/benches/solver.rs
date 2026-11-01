//! Solver benchmarks: Section A-A, dense and local Jacobians. Release
//! build, statistically sound; `cargo bench -p lmsheets-document`.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use lmsheets_document::examples::section_aa;
use lmsheets_document::state::analysis::{Jacobian, Options};
use lmsheets_document::value::params::Params;

fn section_aa_solve(c: &mut Criterion) {
    let mut group = c.benchmark_group("section_aa");
    for how in [Jacobian::Local, Jacobian::Dense] {
        let options = Options::default().with_jacobian(how);
        group.bench_function(format!("solve/{how:?}"), |b| {
            b.iter_batched(
                section_aa,
                |mut s| s.solve(options, &Params::NONE).unwrap(),
                BatchSize::SmallInput,
            )
        });
        let solved = {
            let mut s = section_aa();
            s.solve(options, &Params::NONE).unwrap();
            s
        };
        group.bench_function(format!("analyse/{how:?}"), |b| {
            b.iter(|| solved.analyse(options, &Params::NONE).unwrap())
        });
    }
    group.bench_function("build", |b| b.iter(section_aa));
    group.bench_function("clone", |b| {
        let s = section_aa();
        b.iter(|| s.clone())
    });
    group.finish();
}

criterion_group!(benches, section_aa_solve);
criterion_main!(benches);
