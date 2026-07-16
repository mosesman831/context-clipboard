# Fixtures

Golden inputs for deterministic categorization and (later) FTS checks.

## Categorization format

JSON Lines under `categorization/`. Each line is one object:

```json
{"mime":"text/plain","text":"...","category":"url"}
```

Fields:

- `mime` — clipboard MIME as the categorizer would see it (`text/plain`, `text/uri-list`, `image/png`, …)
- `text` — plaintext body; empty string for pure image payloads
- `category` — expected label: `url` | `code` | `text` | `image` | `file`

Rules match SPEC §7: single URL → `url`; image MIME → `image`; file URL lists / path lists → `file`; fences, shebang, high symbol density → `code`; otherwise `text`.

Categorization is separate from secret filtering. PEM-looking or token-like samples may still be labeled `text` or `code`; skip-store heuristics apply later and must not be confused with category labels.

## CI expectation

Once `clipboard-core` implements the categorizer, CI should score these fixtures and fail below **90%** accuracy. Until that lands, the suite is reference data only; the workflow runs `cargo test` on core and daemon.

Do not commit real clipboard dumps from production machines. Prefer synthetic samples.
