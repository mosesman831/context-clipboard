# Extension (phase 2)

Chrome Native Messaging host for browser-tab URL enrichment. Not part of v1.

In v1 the daemon stores app name and window title only. Phase 2 will add an MV3 extension that talks to a native messaging host (`com.latticeag.context_clipboard`) so copies from Chrome can attach `source_url` and page title. No open localhost HTTP bridge.

The `manifest.json` here is a placeholder so the directory exists in-tree. Do not ship or load it until phase 2.
