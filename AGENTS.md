# Working on CF Gateway

This orphan branch contains only the Rust service. Read README.md and docs/DEVELOPMENT.md first; docs/BOOK_SOURCES.md describes the embedded Legado examples and docs/VALIDATION.md records finite acceptance evidence.

- Preserve the native runtime: no Python, browser process, nokk or general JavaScript execution in the service path.
- Work from current response data. Never replace unknown tasks with recorded answers, stale keys or previous authorization state.
- Keep website responses fresh. Do not introduce HTML/content caching to make latency figures look better.
- Keep the total deadline, response-size limits, session isolation and bounded retries intact.
- Use production helpers in regression tests. Do not add test-only implementations that hide a migration gap.
- Validate parsing changes inside the actual reading client when a device is available. Use only the device the user names.
- Source exports are generated from embedded assets. Do not commit redundant generated sources, captures, user tokens or private proxy settings.
- Keep 69shuba's embedded adapter scoped to its verified render/form/cookie flow. A 200 verification page is not a search result; preserve the site's search-rate errors and actual pagination boundaries.
- Run cargo fmt --check, cargo test --locked and JavaScript syntax checks. Docker changes require both architectures and actual-container smoke tests.
- Report failures and slow samples honestly. Do not claim universal support or a permanent latency guarantee from a finite live sample.
