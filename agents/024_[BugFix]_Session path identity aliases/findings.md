# Findings

- The existing platform path comparison does not reliably protect aliases to an existing session file.
- Unix file identity is available through device and inode metadata.
- Windows file identity can be compared using volume serial number and file index from `GetFileInformationByHandle`.
- Dropping within-cap non-UTF-8 response bytes prevented the server from retaining the raw response; the generic binary receipt field can preserve those bytes without treating them as valid text.
