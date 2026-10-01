//! The calling thread's return addresses, from the unwinder `std` itself links — T91a.
//!
//! `_Unwind_Backtrace` and `_Unwind_GetIP` are the Itanium C++ ABI's unwind interface: `libgcc_s`
//! on Linux, `libunwind` inside `libSystem` on macOS. `std` unwinds panics through the same
//! library, so naming the two functions here links nothing new. The `backtrace` crate wraps the
//! same two calls and brings a symbol resolver with them that no release build would call —
//! `docs/decisions/0060-a-crash-report-carries-offsets-and-the-release-keeps-the-symbols.md`.

use std::ffi::c_void;

/// `_Unwind_Context`, which is only ever handled by pointer.
#[repr(C)]
struct UnwindContext {
    _private: [u8; 0],
}

/// `_URC_NO_REASON`: keep walking.
const CONTINUE: i32 = 0;

/// `_URC_END_OF_STACK`: stop here. Any value but [`CONTINUE`] stops the walk; this is the one that
/// reads as "done" rather than as a failure.
const STOP: i32 = 5;

#[expect(
    unsafe_code,
    reason = "the unwind interface has no Rust binding: the libc crate does not declare it, and \
              the crates that do bring a symbol resolver with them"
)]
unsafe extern "C" {
    fn _Unwind_Backtrace(
        trace: extern "C" fn(*mut UnwindContext, *mut c_void) -> i32,
        argument: *mut c_void,
    ) -> i32;

    fn _Unwind_GetIP(context: *mut UnwindContext) -> usize;
}

/// What the walk writes into, behind the `void *` the unwinder hands back.
struct Collected<'a> {
    into: &'a mut Vec<usize>,
    left: usize,
}

extern "C" fn one_frame(context: *mut UnwindContext, argument: *mut c_void) -> i32 {
    #[expect(
        unsafe_code,
        reason = "`argument` is the `&mut Collected` `return_addresses` passed, alive for the whole \
                  walk and touched by nothing else meanwhile"
    )]
    let collected = unsafe { &mut *argument.cast::<Collected<'_>>() };

    if collected.left == 0 {
        return STOP;
    }

    #[expect(
        unsafe_code,
        reason = "`context` is the one the unwinder is calling us with, valid for this call"
    )]
    let address = unsafe { _Unwind_GetIP(context) };

    // A zero is the unwinder's way of saying it has no frame here, not an address anybody could
    // look up; the walk goes on past it.
    if address != 0 {
        collected.into.push(address);
        collected.left -= 1;
    }

    CONTINUE
}

/// See [`crate::crash_image::return_addresses`].
pub(crate) fn return_addresses(into: &mut Vec<usize>, max: usize) {
    let mut collected = Collected { into, left: max };

    #[expect(
        unsafe_code,
        reason = "`one_frame` matches the callback's C signature, and the argument is a pointer to a \
                  local that outlives the call"
    )]
    unsafe {
        _Unwind_Backtrace(one_frame, (&raw mut collected).cast::<c_void>());
    }
}
