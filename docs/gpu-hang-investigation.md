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

## Leading hypothesis (unconfirmed)
In the hang frame the indirect args hold a large group count, perhaps written wrongly
or not yet written by the 741447 pass. The clamp in crates/victory-gpu/src/indirect.rs
allows up to 2^20 groups per dispatch and a 2^27-thread budget per list. That is
enough f607 work to pass the TDR.

## To confirm
One run with VICTORY_D3D12_DRED=1, VICTORY_DUMP_TARGETS=1, VICTORY_CAPTURE and
VICTORY_CAPTURE_FRAMES covering the hang window. Then read the `indirect kind 2 … f607780…`
trace lines. Each run takes about 14 GB of RAM.
