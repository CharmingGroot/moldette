const INLINE_SLOTS: usize = 32;

pub(crate) fn with_slots<T, R>(count: usize, fill: impl FnOnce(&mut [Option<T>]) -> R) -> R {
    let mut inline: [Option<T>; INLINE_SLOTS] = [const { None }; INLINE_SLOTS];
    let mut heap: Vec<Option<T>>;
    let slots = match inline.get_mut(..count) {
        Some(slots) => slots,
        None => {
            heap = (0..count).map(|_| None).collect();
            &mut heap
        }
    };
    fill(slots)
}
