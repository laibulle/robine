use std::panic::{AssertUnwindSafe, catch_unwind};
use unicode_segmentation::UnicodeSegmentation;

/// Counts user-perceived grapheme clusters through an explicitly stable C ABI.
///
/// Returns `usize::MAX` when the pointer is invalid, the bytes are not UTF-8,
/// or the Rust implementation panics. No panic crosses the foreign boundary.
///
/// # Safety
///
/// When `length` is non-zero, `pointer` must address `length` readable bytes
/// that remain valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn robine_demo_grapheme_count(pointer: *const u8, length: usize) -> usize {
    if pointer.is_null() && length != 0 {
        return usize::MAX;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let bytes = if length == 0 {
            &[]
        } else {
            // SAFETY: The foreign caller promises a readable buffer of length
            // bytes for the duration of this call.
            unsafe { std::slice::from_raw_parts(pointer, length) }
        };
        let Ok(text) = std::str::from_utf8(bytes) else {
            return usize::MAX;
        };
        UnicodeSegmentation::graphemes(text, true).count()
    }))
    .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_calls_a_real_rust_ecosystem_crate() {
        let text = "Robine 👩🏽‍💻";
        // SAFETY: The buffer remains valid for the call.
        let count = unsafe { robine_demo_grapheme_count(text.as_ptr(), text.len()) };
        assert_eq!(count, 8);
    }

    #[test]
    fn bridge_rejects_invalid_utf8_without_unwinding() {
        let bytes = [0xff];
        // SAFETY: The buffer remains valid for the call.
        let count = unsafe { robine_demo_grapheme_count(bytes.as_ptr(), bytes.len()) };
        assert_eq!(count, usize::MAX);
    }
}
