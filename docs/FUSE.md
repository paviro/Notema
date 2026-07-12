# Mount as a filesystem (FUSE)

Mount an encrypted journal as an ordinary, decrypted directory — grep it, open
entries in any editor, drop images into an entry's assets folder — while the store
stays encrypted on disk. Plaintext only ever lives in memory; nothing decrypted is
written to disk.

```bash
notema mount ~/journal-mnt        # blocks until unmounted
umount ~/journal-mnt              # macOS: diskutil unmount ~/journal-mnt
```

This needs a **FUSE-enabled build** and a FUSE provider installed — Apple Silicon
macOS has a prebuilt `-fuse` download, every other platform builds natively. See
[BUILDING.md](BUILDING.md#fuse-builds). The standard builds omit the `mount`
command.

- Fully read-write: editing, creating, deleting, and renaming files and folders
  (including moving entries between folders and renaming journals) are re-encrypted
  back to disk.
- Deleting through the mount is permanent — unlike deleting inside `notema`, it
  removes the entry outright instead of moving it to `.trash`. The `.trash` folder
  is visible in the mount, so entries the app trashed can be moved back out.
- The mount reads from disk on every access, so changes from another process (a
  second `notema`, a sync client) show up promptly.
- Don't edit the **same entry** in two places at once — through the mount and in
  `notema`, or on another device. There's no merge: whichever side saves last
  overwrites the whole file and silently wins. Editing different entries, or the
  same one at a time, is always safe.
- Renaming or moving an entry file leaves its `<id>.assets` sibling behind — the
  folder is keyed to the entry id, so if the entry needs its assets you have to
  move or rename that folder to match by hand.
