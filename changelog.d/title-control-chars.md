# Title control character validation

Add title validation to reject control characters and invisible formatting characters in page titles.

## Changes

### crates/gw-core/src/title.rs
- Added `title_problem` function that validates titles against control characters and invisible formatting characters
- Checks for: all `c.is_control()` characters, U+2028/U+2029, U+200B..=U+200F, U+202A..=U+202E, U+2060..=U+2064, U+2066..=U+2069, and U+FEFF
- Returns `Some(German-free English reason)` if validation fails, else `None`

### crates/gw-core/src/lib.rs
- Exported `title_problem` function from the title module

### crates/gw-store/src/templates.rs
- Added title validation check in `create_page_for` function (line ~160)
- Returns `Blocked` with validation reason if title contains control/invisible characters

### crates/gw-store/src/moves.rs
- Added title validation check in `move_document` function (line ~124)
- Returns `Blocked` with validation reason if title contains control/invisible characters

## Verification

- Unit tests verify that titles with control/invisible characters are rejected
- Unit tests verify that valid titles pass validation
- All verification gates pass (cargo fmt --check, cargo clippy, cargo test)