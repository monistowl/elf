# Installer & CI polish — November 2025

- Installer now uses either `sha256sum` or `shasum` to validate downloaded tarballs; when neither tool exists, it warns instead of failing, and mismatched hashes still abort the install. The temporary download is cleaned up immediately after extraction.
- CI runs `cargo run -p elf-cli -- dataset-validate --spec test_data/dataset_suite_core.json` after the test suite and before release packaging so regressions in the regression fixture set fail fast, and the release job now reuses the same validation step prior to creating tarballs.
