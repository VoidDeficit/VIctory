# GPU hang in the world (DEVICE_HUNG), Series X and Series S

Status: **open**, cause unconfirmed. Last updated 2026-11-25 (GTX 1660 Super).

Branch fix/nvidia-heap-clamp (PR #2185). Fixed and committed there: the NVIDIA
INVALID_CALL removals (heap clamp 4d91d9d/8d5eaaa, raw structured views 16580cd,
unstorable texture UAVs a81ee21, review fixes 852b207). Still open: the DEVICE_HUNG below.

## Series S
- The old exit-139 segfault does not reproduce (`run --console series-s`).
- It now stops at frame ~1515 with the same DEVICE_HUNG as Series X. It is the same bug,
  not a Series S-only one.

## The hang
- 0x887A0006 between frames ~1340 and ~1530.
- DRED: the hung command is `indirect kind 2 max 1 [compute f607780aa8e502d0]`
  (args at title 0xd9358fe80).
- Skipping only f607780aa8e502d0: no removal through frame 1699.
  Skipping 741447eb077ae6d3 (the voxel grid writer) also avoids it.
- f607 is a BVH traversal (emulated image_bvh64_intersect_ray, LDS short stack, two
  loops each capped at 0xffff by the shader).
- **Loop count ruled out**: with VICTORY_SHADER_LOOP_LIMIT=16 plus DRED it still hangs on
  the same command at frame 1341.
- In captured frames that did not hang, the dispatch args are [0,1,1] or [1,1,1].

- **Group count ruled out**: with the indirect clamp's GROUPS set to 1 (every indirect
  dispatch runs one group or none) it still hangs on the same command, at frame 1467.
  No DRED page fault is reported, so it is a real hang inside a single group.
- A plain args capture cannot show the hang frame: `read_indirect` logs only after the
  list completes, and a hung list never completes.
- f607's disassembly has no `s_barrier`. The two loops are wave-uniform back edges
  (`s_cbranch_scc1` on the s20 and s6 masks). The BVH lowering (emit/lower/bvh.rs) has
  no loops of its own.

## Open leads
1. Loops the per-lane counter misses. Check that both f607 back edges get a counter in
   the translated DXIL, and that the wave-uniform exit (the scc mask built from exec)
   still ends once lanes take the abort block.
2. A wave-op or convergence livelock on NVIDIA, such as ballot-based exec emulation
   inside the loop with lanes that have returned.
3. Data written by 741447 (the voxel grid writer). Skipping it avoids the hang, so the
   BVH or LDS stack input may be what traps a lane. Dump 741447's output buffers in the
   frame before the hang (VICTORY_DUMP_WRITERS=741447eb077ae6d3).
