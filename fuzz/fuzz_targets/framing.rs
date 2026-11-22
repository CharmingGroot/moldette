#![no_main]

use libfuzzer_sys::fuzz_target;
use moldette::framing::Framer;

fuzz_target!(|data: &[u8]| {
    let Some((&split, input)) = data.split_first() else {
        return;
    };
    let chunk = usize::from(split % 16) + 1;
    let framers = [
        Framer::lines().max_record_len(4096),
        Framer::multiline(r"^\S")
            .expect("valid pattern")
            .max_record_len(4096),
        Framer::json().max_record_len(4096),
    ];
    for mut framer in framers {
        let mut emit = |record: &str| {
            let _ = record.len();
        };
        let pushed = input
            .chunks(chunk)
            .all(|part| framer.push(part, &mut emit).is_ok());
        if pushed {
            let _ = framer.finish(&mut emit);
        }
    }
});
