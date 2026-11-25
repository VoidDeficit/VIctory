# GPU hang in the world (DEVICE_HUNG), Series X and Series S

Status: **fixed** (GTX 1660 Super). Last updated 2026-11-25.

Branch fix/nvidia-heap-clamp (PR #2185). Fixed and committed there: the NVIDIA
INVALID_CALL removals (heap clamp 4d91d9d/8d5eaaa, raw structured views 16580cd,
unstorable texture UAVs a81ee21, review fixes 852b207), and this DEVICE_HUNG.

## Cause and fix
The 1660 caps the view heap at 10^6 (`host view heap capped at 1000000 count=1245184
kept=934461`). The per-slot view-addresses table (`HostHeap::view_addresses`,
crates/victory-gpu/src/descriptor/host.rs) was sized for `count` = 934461 slots.
Translated shaders read it through a root SRV, which has no bounds checks, at a heap
index clamped only below `heap_views` (10^6). That index can reach 999999, so a read
can land up to ~524 KB past the buffer. On NVIDIA that hung f607 with no DRED page fault.

The strides and marks tables were already sized `count.max(HEAP_VIEWS)`. All three now
use `indexed_slots(count)`. Test: `per_slot_tables_cover_every_index_a_shader_reads`.

Verified with the diag build plus DRED. No run after the fix had a device removal.
Every unfixed run hung between frames 1340 and 1603.
- Series X (vaddr1): no removal. At frame 1699 the run ended in a title crash: seven
  int3s (0x80000003 at victory-image+0x5149aec), then WER.
- Series X (final1, the build after the review fixes): no removal. At frame 1797 it
  ended with exit 139: an AV executing unmapped memory inside the NVIDIA driver
  (nvwgf2umx.dll), called from victory.exe through D3D12Core. The log line before it
  was `slow host submission ms=1684`, followed by mesh pipeline creation.
- Series S (vaddrS): no removal through frame 1929, when the 480 s run timed out. It
  logged one first-chance AV in nvwgf2umx.dll+0x122db1c (read 0x0); the title
  survived it.

## New crashes past the old hang point (open)
These are separate from the hang and not yet triaged: the title int3s, and the driver
AV. Neither run produced DRED data, and the victory.exe frames are not symbolized.

## Series S
- The old exit-139 segfault does not reproduce (`run --console series-s`).
- Before the fix it stopped at frame ~1515 with the same DEVICE_HUNG as Series X.
  It is the same bug, not a Series S-only one.
- After the fix: no removal through frame 1929, when the 480 s run timed out.

## The hang (before the fix)
- 0x887A0006 between frames ~1340 and ~1603.
- DRED: the hung command is `indirect kind 2 max 1 [compute f607780aa8e502d0]`
  (args at title 0xd9358fe80).
- Skipping only f607780aa8e502d0: no removal through frame 1699.
  Skipping 741447eb077ae6d3 (the voxel grid writer) also avoids it.
- f607 is a BVH traversal (emulated image_bvh64_intersect_ray, LDS short stack, two
  loops each capped at 0xffff by the shader).

## Leads ruled out
1. **Loop count.** With VICTORY_SHADER_LOOP_LIMIT=16 it still hung on the same command
   at frame 1341.
2. **Group count.** With every indirect dispatch clamped to one group it still hung at
   frame 1467. No DRED page fault was reported.
3. **Uncounted loops.** f607's only back edges both carry the per-lane counter. There is
   no `s_barrier`.
4. **Wave-op or atomic spin.** waveReadLaneFirst appears only outside the loops. LDS uses
   only store and atomicrmw xchg.
5. **Dirty-bitmap atomics.** Skipping the memory-dirty OR still hung (frame 1455).
6. **LDS out of range.** The groupshared array is 512 dwords and every address is masked
   with `and 2044`.
7. **BVH node data.** Zeroing every node dword still hung on the same command (frame 1463).
8. **Title indices reaching dynamic slots.** `title_guard` (emit/lower.rs) sends a title
   index at or past `title_views()` to the null last slot. It still hung at frame 1603,
   so this was not the cause. It is kept as a correctness fix: such an index read
   another draw's view, perhaps of another kind.

## Open, not addressed
- `heap_handle` clamps an index to the last slot, which holds a Texture2D null SRV.
  A buffer or UAV handle clamped there is a type mismatch. No hang has been seen from it.
- Review follow-up: on a capped host the heap is now exactly `heap_views()` slots unless
  the title plus its dynamic and null slots fit under it. A larger heap would put title
  views at the null slots shaders clamp to. The uncapped fallback keeps
  `views - MIN_DYNAMIC - 3` title slots again.
