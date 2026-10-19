# Findings

- Release actions are explicitly authorized. Do not remove tags or rewrite/push refs; root owns later history compression.
- Preserve each full release API response in the external backup. Manifest and chat output must contain metadata summaries and checksums only, never full release bodies.
- Before cleanup GitHub returned 10 releases: v2.0.1 through v2.1.0 (nine old releases) and v2.2.0 (ID 417493925); each had two assets. Release v2.0.0 had a tag but no Release object.
- External backup has all 10 full release API records and all 18 older assets. Every older asset matched the API-reported size and SHA-256 digest.
- After cleanup only release ID 417493925 remains. Its body is exactly `改进了一些内容`; target commitish, tag, title, draft/prerelease flags, asset IDs, sizes, and digests match the saved original record. Both downloaded assets match the original API digests.
- All 11 `v2.*` tags, including v2.0.0, have identical ref names, object IDs, and object types before and after cleanup. Release deletion used no tag cleanup option.
- A PowerShell comparison initially failed because of expression/serialization handling, not differing GitHub data. Direct asset-ID comparison and `Compare-Object` over tag ref/SHA/type confirmed the final state; no additional mutation was performed during verification.
