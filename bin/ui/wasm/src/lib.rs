use std::{
    alloc::{Layout, alloc, dealloc},
    ptr::null_mut,
    slice,
};

const BAR_WIDTH: usize = 6;

#[unsafe(no_mangle)]
pub extern "C" fn add(left: i32, right: i32) -> i32 {
    left.wrapping_add(right)
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc_f64(length: usize) -> *mut f64 {
    let Ok(layout) = Layout::array::<f64>(length) else {
        return null_mut();
    };

    if layout.size() == 0 {
        return null_mut();
    }

    // The caller releases this allocation with `free_f64`.
    unsafe { alloc(layout).cast() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free_f64(pointer: *mut f64, length: usize) {
    if pointer.is_null() || length == 0 {
        return;
    }

    let Ok(layout) = Layout::array::<f64>(length) else {
        return;
    };

    // SAFETY: the caller must pass the pointer and length returned to it by
    // `alloc_f64`.
    unsafe { dealloc(pointer.cast(), layout) };
}

/// Resamples packed OHLCV rows in place.
///
/// Each input row is six `f64` values: timestamp in milliseconds, open, high,
/// low, close, and volume. The function returns the number of output rows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn resample_ohlcv(
    pointer: *mut f64,
    bar_count: usize,
    timeframe_ms: u32,
) -> usize {
    if pointer.is_null() || bar_count == 0 || timeframe_ms == 0 {
        return 0;
    }

    let Some(value_count) = bar_count.checked_mul(BAR_WIDTH) else {
        return 0;
    };

    // SAFETY: the caller supplies a buffer containing `bar_count * BAR_WIDTH`
    // writable f64 values.
    let bars = unsafe { slice::from_raw_parts_mut(pointer, value_count) };
    let interval = f64::from(timeframe_ms);
    let first = &bars[..BAR_WIDTH];

    if !valid_bar(first) {
        return 0;
    }

    let mut current = [first[0], first[1], first[2], first[3], first[4], first[5]];
    let mut current_bucket = bucket_start(current[0], interval);
    let mut output_count = 0;

    for index in 1..bar_count {
        let start = index * BAR_WIDTH;
        let next = [
            bars[start],
            bars[start + 1],
            bars[start + 2],
            bars[start + 3],
            bars[start + 4],
            bars[start + 5],
        ];

        if !valid_bar(&next) {
            return 0;
        }

        let next_bucket = bucket_start(next[0], interval);

        if next_bucket == current_bucket {
            current[2] = current[2].max(next[2]);
            current[3] = current[3].min(next[3]);
            current[4] = next[4];
            current[5] += next[5];
            continue;
        }

        let output_start = output_count * BAR_WIDTH;
        bars[output_start..output_start + BAR_WIDTH].copy_from_slice(&current);
        output_count += 1;

        current = next;
        current[0] = next_bucket;
        current_bucket = next_bucket;
    }

    let output_start = output_count * BAR_WIDTH;
    bars[output_start..output_start + BAR_WIDTH].copy_from_slice(&current);

    output_count + 1
}

fn bucket_start(timestamp_ms: f64, timeframe_ms: f64) -> f64 {
    (timestamp_ms / timeframe_ms).floor() * timeframe_ms
}

fn valid_bar(bar: &[f64]) -> bool {
    bar.len() == BAR_WIDTH && bar.iter().all(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::resample_ohlcv;

    #[test]
    fn resamples_ohlcv_into_time_buckets() {
        let mut bars = vec![
            0.0, 10.0, 12.0, 9.0, 11.0, 2.0,
            60_000.0, 11.0, 13.0, 10.0, 12.0, 3.0,
            120_000.0, 12.0, 14.0, 11.0, 13.0, 4.0,
            300_000.0, 13.0, 15.0, 12.0, 14.0, 5.0,
        ];

        // SAFETY: `bars` contains four complete writable OHLCV rows.
        let output_count =
            unsafe { resample_ohlcv(bars.as_mut_ptr(), 4, 300_000) };

        assert_eq!(output_count, 2);
        assert_eq!(&bars[..6], &[0.0, 10.0, 14.0, 9.0, 13.0, 9.0]);
        assert_eq!(&bars[6..12], &[300_000.0, 13.0, 15.0, 12.0, 14.0, 5.0]);
    }
}
