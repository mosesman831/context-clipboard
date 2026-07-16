const samples = [
  {
    title: "Recent item preview",
    meta: "text - Terminal - just now",
    preview: "The Tauri popup will render daemon ClipSummary data here."
  },
  {
    title: "Search result",
    meta: "code - Editor - 5 minutes ago",
    preview: "Clipboard bodies must be inserted as text nodes, never as raw HTML."
  },
  {
    title: "Paused state",
    meta: "settings - Capture controls",
    preview: "The tray icon will show when capture is paused."
  }
];

const list = document.querySelector("#recent");

for (const sample of samples) {
  const item = document.createElement("li");
  item.className = "clip";

  const title = document.createElement("strong");
  title.textContent = sample.title;

  const meta = document.createElement("small");
  meta.textContent = sample.meta;

  const preview = document.createElement("p");
  preview.textContent = sample.preview;

  item.append(title, meta, preview);
  list.append(item);
}
