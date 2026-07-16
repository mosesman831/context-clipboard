const samples = [
  {
    category: "url",
    app: "Firefox",
    when: "just now",
    preview: "https://docs.rs/arboard/latest/arboard/"
  },
  {
    category: "code",
    app: "Zed",
    when: "2m ago",
    preview: "fn thumbnail_dimensions(w: u32, h: u32, max: u32) -> (u32, u32)"
  },
  {
    category: "image",
    app: "Preview",
    when: "8m ago",
    preview: "[image 800x600]"
  },
  {
    category: "text",
    app: "Terminal",
    when: "14m ago",
    preview: "cargo test -p clipboard-daemon image_capture"
  },
  {
    category: "file",
    app: "Finder",
    when: "1h ago",
    preview: "file:///Users/dev/shots/tray-mock.png"
  }
];

const list = document.querySelector("#recent");
const search = document.querySelector("#q");
const countEl = document.querySelector("#result-count");
const pauseBtn = document.querySelector("#pause");

function render(items) {
  list.replaceChildren();
  if (items.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = "No clips match.";
    list.append(empty);
    countEl.textContent = "0";
    return;
  }

  items.forEach((sample, i) => {
    const item = document.createElement("li");
    item.className = "clip";
    item.tabIndex = 0;
    item.style.setProperty("--i", `${i * 40}ms`);

    const preview = document.createElement("p");
    preview.className = "clip-preview";
    preview.textContent = sample.preview;

    const meta = document.createElement("div");
    meta.className = "clip-meta";

    const cat = document.createElement("span");
    cat.className = "cat";
    cat.textContent = sample.category;

    const app = document.createElement("span");
    app.textContent = sample.app;

    const when = document.createElement("span");
    when.textContent = sample.when;

    meta.append(cat, app, when);
    item.append(preview, meta);
    list.append(item);
  });

  countEl.textContent = String(items.length);
}

function filterClips(query) {
  const q = query.trim().toLowerCase();
  if (!q) return samples;
  return samples.filter((s) =>
    [s.preview, s.category, s.app].some((v) => v.toLowerCase().includes(q))
  );
}

search.addEventListener("input", () => {
  render(filterClips(search.value));
});

pauseBtn.addEventListener("click", () => {
  const pressed = pauseBtn.getAttribute("aria-pressed") === "true";
  pauseBtn.setAttribute("aria-pressed", String(!pressed));
  pauseBtn.textContent = pressed ? "Pause" : "Resume";
});

render(samples);
