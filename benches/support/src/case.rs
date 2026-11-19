use crate::{BATCHES, Size, Workload, batch};

/// Inputs a benchmark runs on each iteration, with the total length of their
/// raw records. Layers that read the raw text report MB/s against it.
pub struct Case<T> {
    pub name: String,
    pub inputs: Vec<T>,
    pub raw_bytes: u64,
}

/// One case per record size (`record/100B`, `record/10KiB`, `record/1MiB`)
/// and per batch size (`batch/1MiB`, `batch/16MiB`), with the inputs that
/// `input` builds from each record index and size.
pub fn cases<T>(workload: Workload, input: impl Fn(usize, Size) -> T) -> Vec<Case<T>> {
    let records = Size::ALL.into_iter().map(|size| {
        let raw_bytes = size
            .indices()
            .map(|i| workload.raw(i, size).len() as u64)
            .sum();
        Case {
            name: format!("record/{}", size.label()),
            inputs: size.indices().map(|i| input(i, size)).collect(),
            raw_bytes,
        }
    });
    let batches = BATCHES.into_iter().map(|(label, bytes)| {
        let raws = batch(workload, bytes);
        Case {
            name: format!("batch/{label}"),
            inputs: (0..raws.len()).map(|i| input(i, Size::Small)).collect(),
            raw_bytes: raws.iter().map(|raw| raw.len() as u64).sum(),
        }
    });
    records.chain(batches).collect()
}
