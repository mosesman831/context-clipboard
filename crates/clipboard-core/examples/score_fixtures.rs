fn main() {
    let path = std::env::args().nth(1).expect("path");
    let (c, t) =
        clipboard_core::categorize::load_fixtures_and_score(std::path::Path::new(&path)).unwrap();
    println!("{c}/{t}");
    use std::io::BufRead;
    let f = std::fs::File::open(&path).unwrap();
    for (i, line) in std::io::BufReader::new(f).lines().enumerate() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let mime = v["mime"].as_str().unwrap_or("");
        let text = v.get("text").and_then(|t| t.as_str());
        let expected = v["category"].as_str().unwrap();
        let got = clipboard_core::categorize::categorize(mime, text).as_str();
        if got != expected {
            let preview: String = text.unwrap_or("").chars().take(60).collect();
            let preview = preview.replace('\n', " ");
            println!(
                "L{} expected={} got={} mime={} text={:?}",
                i + 1,
                expected,
                got,
                mime,
                preview
            );
        }
    }
}
