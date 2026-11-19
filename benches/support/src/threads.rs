use std::thread;

/// Thread counts of the scaling cases. Each thread runs the whole input, and
/// throughput counts the records of all threads.
pub const THREADS: [usize; 4] = [1, 2, 4, 8];

/// Runs `work` on one scoped thread per input and returns when every thread
/// has finished.
pub fn on_threads<T: Send>(inputs: impl IntoIterator<Item = T>, work: impl Fn(T) + Sync) {
    thread::scope(|scope| {
        for input in inputs {
            let work = &work;
            scope.spawn(move || work(input));
        }
    });
}
