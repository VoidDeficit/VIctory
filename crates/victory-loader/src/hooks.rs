//! Opt-in compatibility hooks for known title builds.
//!
//! Only per-build offsets and short byte signatures live here (asset policy:
//! nothing of the title itself). Every hook checks the build's hash and the
//! exact bytes it replaces, and does nothing otherwise.

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// First 16 hex digits of the BLAKE3 of the Series X 1.0.2.1 executable.
const BUILD_1_0_2_1: &str = "b99f876b74d4a77f";

/// Environment variable that turns the watchdog hook on.
pub(crate) const NO_WATCHDOG_ENV: &str = "VICTORY_NO_WATCHDOG";

/// Environment variable that turns the pool-full hook on.
pub(crate) const POOL_FULL_ENV: &str = "VICTORY_POOL_FULL";

/// Environment variable that makes the startup ready-wait answer true.
pub(crate) const SKIP_READY_ENV: &str = "VICTORY_SKIP_READY";

/// One code patch: `rva` must hold exactly `expect` before `new` is written.
struct Patch {
    what: &'static str,
    rva: usize,
    expect: &'static [u8],
    new: &'static [u8],
}

/// The title's watchdog thread ends the process on purpose when its main
/// thread has not advanced for about 30 one-second ticks. Two places do it:
/// the handler it calls (`mov dword ptr [0], imm32`) and, when the handler
/// returns or is unset, a fallback right after the call (`mov dword ptr [0],
/// 0; int3`). Jumping over the whole block returns the loop to its normal
/// path; the first patch makes the handler harmless if something else calls it.
const WATCHDOG: &[Patch] = &[
    Patch {
        what: "watchdog: skip the call and the fallback fault",
        rva: 0x2215c27,
        // mov rax, [rip + disp32] (the handler pointer)
        expect: &[0x48, 0x8B, 0x05, 0x1A, 0x07, 0xFA, 0x04],
        // jmp +0x18 (to the loop's normal continuation), then NOPs
        new: &[0xEB, 0x18, 0x90, 0x90, 0x90, 0x90, 0x90],
    },
    Patch {
        what: "watchdog: handler's deliberate fault",
        rva: 0x23cf683,
        expect: &[0xC7, 0x04, 0x25, 0, 0, 0, 0, 0x21, 0xD4, 0x8B, 0x4D],
        // an eleven-byte NOP
        new: &[0x66, 0x66, 0x2E, 0x0F, 0x1F, 0x84, 0, 0, 0, 0, 0],
    },
];

/// The Building pool's allocator breaks into `int3` when its free list is
/// empty; the instruction after the trap loads zero and returns it, so a NOP
/// in place of the trap turns the failure into a null result the caller can
/// handle (it tests the result). Opt-in: the scene then misses the entity
/// rather than ending the process.
const POOL_FULL: &[Patch] = &[Patch {
    what: "pool full: return null instead of int3",
    rva: 0x5149aec,
    expect: &[0xCC],
    new: &[0x90],
}];

/// The startup wait `title-main` spins in (`+0x23cf716` calls `+0x23cb280`
/// and sleeps 1 ms while it is false). The thunk reads a global and tail-calls
/// the check; forcing `al = 1` lets the loop proceed. Opt-in, and only if the
/// thunk's first bytes match.
const SKIP_READY: &[Patch] = &[
    Patch {
        what: "startup: the ready wait answers true",
        rva: 0x23cb280,
        expect: &[0x48, 0x8B, 0x0D],
        new: &[0xB0, 0x01, 0xC3],
    },
    // The null-service assert that follows the skipped wait (cmp [rcx+0x20],0
    // falls into an int3). Opt-in with the same switch, for the experiment.
    Patch {
        what: "startup: a null service after the wait is not fatal",
        rva: 0x1377e7,
        expect: &[0xCC],
        new: &[0x90],
    },
];

/// Applies the ready-wait patch if the build and bytes match.
///
/// # Safety
/// `base` is the mapped image whose original BLAKE3 is `hash`, and no title
/// code is running yet.
pub(crate) unsafe fn skip_ready(base: usize, hash: &blake3::Hash) -> usize {
    if !hash.to_hex().starts_with(BUILD_1_0_2_1) {
        return 0;
    }
    SKIP_READY
        .iter()
        // SAFETY: forwarded from this function's contract.
        .filter(|p| unsafe { apply(base, p) })
        .count()
}

/// Pool traps the title installs at runtime: the load-time image holds a
/// pointer table there, and unpacking later writes `ret; int3; xor; jmp`.
/// The first is patched at load if it is already `0xCC`; the watcher below
/// patches whichever appears later.
const POOL_TRAPS: &[usize] = &[0x5149aec, 0x63966f];

/// Watches `POOL_TRAPS` for the `int3` the title writes at runtime and turns
/// it into a NOP, so an exhausted pool returns its built-in null instead of
/// ending the process. Runs until both traps have been seen and patched.
///
/// # Safety
/// `base` is the mapped image whose original BLAKE3 is `hash`.
pub(crate) unsafe fn watch_pool_full(base: usize, hash: &blake3::Hash) {
    if !hash.to_hex().starts_with(BUILD_1_0_2_1) {
        return;
    }
    std::thread::spawn(move || {
        let mut patched = [false; POOL_TRAPS.len()];
        loop {
            let mut done = true;
            for (i, &rva) in POOL_TRAPS.iter().enumerate() {
                if patched[i] {
                    continue;
                }
                done = false;
                let at = (base + rva) as *mut u8;
                // SAFETY: the offset is inside the image; the byte is read
                // and only replaced when it is the trap.
                if unsafe { at.read_volatile() } == 0xCC {
                    let mut old = 0;
                    // SAFETY: one byte of the image; the change is restored.
                    let changed = unsafe {
                        if VirtualProtect(at.cast(), 1, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
                            false
                        } else {
                            at.write_volatile(0x90);
                            VirtualProtect(at.cast(), 1, old, &mut old);
                            FlushInstructionCache(GetCurrentProcess(), at.cast(), 1);
                            true
                        }
                    };
                    if changed {
                        tracing::info!(rva = format_args!("{rva:#x}"), "pool-full: trap patched at runtime");
                        patched[i] = true;
                    }
                }
            }
            if done {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
}

/// Applies the pool-full patch if this is the build and the byte matches.
///
/// # Safety
/// `base` is the mapped image whose original BLAKE3 is `hash`, and no title
/// code is running yet.
pub(crate) unsafe fn ignore_pool_full(base: usize, hash: &blake3::Hash) -> usize {
    if !hash.to_hex().starts_with(BUILD_1_0_2_1) {
        return 0;
    }
    POOL_FULL
        .iter()
        // SAFETY: forwarded from this function's contract.
        .filter(|p| unsafe { apply(base, p) })
        .count()
}

/// Applies the watchdog patches whose bytes match. Returns how many applied.
///
/// # Safety
/// `base` is the mapped image whose original BLAKE3 is `hash`, and no title
/// code is running yet.
pub(crate) unsafe fn disarm_watchdog(base: usize, hash: &blake3::Hash) -> usize {
    if !hash.to_hex().starts_with(BUILD_1_0_2_1) {
        return 0;
    }
    WATCHDOG
        .iter()
        // SAFETY: forwarded from this function's contract.
        .filter(|p| unsafe { apply(base, p) })
        .count()
}

/// # Safety
/// See `disarm_watchdog`.
unsafe fn apply(base: usize, p: &Patch) -> bool {
    let at = (base + p.rva) as *mut u8;
    // SAFETY: the offset is inside the mapped .text of this exact build.
    let now = unsafe { std::slice::from_raw_parts(at, p.expect.len()) };
    if now != p.expect || p.new.len() != p.expect.len() {
        tracing::warn!(what = p.what, "hook skipped: the bytes are not the expected ones");
        return false;
    }
    let mut old = 0;
    // SAFETY: changing the protection of those bytes only, restored below.
    if unsafe { VirtualProtect(at as _, p.new.len(), PAGE_EXECUTE_READWRITE, &mut old) } == 0 {
        return false;
    }
    // SAFETY: writable now; the bytes are the ones checked above.
    unsafe {
        std::ptr::copy_nonoverlapping(p.new.as_ptr(), at, p.new.len());
        VirtualProtect(at as _, p.new.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at as _, p.new.len());
    }
    tracing::info!(what = p.what, "hook applied");
    true
}
