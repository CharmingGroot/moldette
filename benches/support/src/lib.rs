//! Workloads shared by the per-domain benchmarks (#64).
//!
//! Each workload gives the same record in the representation of every layer:
//! the raw text a parser reads, the `FieldRecord` a parser returns and the
//! `Mapped` a mapper returns. A benchmark crate feeds its layer the input of
//! that layer only, and its tests check that the layer turns one
//! representation into the next.

mod allocations;
mod case;
mod workload;

pub use allocations::{CountingAllocator, allocations, per_input};
pub use case::{Case, cases};
pub use workload::{BATCHES, Size, Workload, batch, borrowed, same_fields, same_mapped};
