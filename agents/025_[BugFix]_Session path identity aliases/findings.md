# Findings

- The existing platform path comparison does not reliably protect aliases to an existing session file.
- Unix file identity is available through device and inode metadata.
- Windows file identity can be compared using volume serial number and file index from `GetFileInformationByHandle`.
- Dropping within-cap non-UTF-8 response bytes prevented the server from retaining the raw response; the generic binary receipt field can preserve those bytes without treating them as valid text.
- Rust's Windows `FileType::new` marks a reparse point as a symlink only when the tag has the name-surrogate bit. Mount-point junctions do not satisfy that condition, so `file_type().is_symlink()` alone does not reject them. Checking `FILE_ATTRIBUTE_REPARSE_POINT` closes this root-escape path.
