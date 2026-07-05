# SAIOS State Snapshot

Date: 2026-07-05
Scope: Storage/ext4 behavior after native reader and write-path hardening

## Summary

- Native ext4 traversal is active in the storage stack.
- Intermittent directory-read behavior was reduced by sparse-hole handling and per-block directory parsing.
- Mount semantics now honor read-only vs read-write intent.
- Native ext4 write support is available in a limited form.

## Current Native ext4 Capabilities

- Read superblock and parse core geometry.
- Resolve inode table entries (including inode #2 root).
- Traverse extents and resolve logical-to-physical blocks.
- Parse directory entries using `rec_len` with block-scoped validation.
- Read regular files.
- Write existing regular files in place (no file growth).

## Current Native ext4 Limitations

- No native allocator yet for new data blocks.
- No inode allocation for new files/directories.
- No metadata updates for directory entry create/remove/rename.
- No journal updates/replay integration.

Operational result:

- In native ext4 mode, in-place writes to existing regular files can succeed.
- `create`, `mkdir`, `delete`, and `rename` on native ext4 are intentionally unsupported and return explicit errors.

## Diagnostics Added

- Native ext4 debug report path is available to dump:
  - superblock fields
  - root inode fields
  - first extent
  - first directory block hex preview
  - parsed directory entries

## Recommended Next Implementation Slice

1. Add bounded file-growth support for native ext4 writes when free blocks are already pre-reserved in extent ranges.
2. Add minimal metadata update path for truncate-to-smaller and in-directory rename within same directory.
3. Add journal-awareness guardrails before enabling block/inode allocation.
