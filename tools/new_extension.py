"""
Scaffolding CLI for creating a new Bunori extension source.
Generates manifest.json, Cargo.toml, and a starter src/lib.rs.

Usage:
  python tools/new_extension.py
  python tools/new_extension.py -n "Novel Hi" -u "https://novelhi.com"
  python tools/new_extension.py --name "Novel Hi" --url "https://novelhi.com" --lang en --id novelhi
"""

import argparse
import json
import re
import sys
from pathlib import Path


def sanitize_id(name: str) -> str:
    """Derive a valid Cargo crate / extension ID from the name."""
    s = name.lower()
    s = re.sub(r"[^a-z0-9_]+", "", s)
    return s


def to_pascal_case(name: str) -> str:
    """Convert a name to a valid Rust PascalCase struct name."""
    words = re.findall(r"[a-zA-Z0-9]+", name)
    pascal = "".join(w.capitalize() for w in words)
    if not pascal or not pascal[0].isalpha():
        pascal = "My" + pascal
    return pascal


def create_extension(name: str, base_url: str, ext_id: str | None = None, lang: str = "en"):
    project_root = Path(__file__).resolve().parent.parent
    sources_dir = project_root / "sources"

    if not ext_id:
        ext_id = sanitize_id(name)

    if not re.match(r"^[a-z0-9_]+$", ext_id):
        raise ValueError(
            f"Invalid extension ID '{ext_id}'. Must contain only lowercase letters, digits, and underscores."
        )

    target_dir = sources_dir / ext_id
    if target_dir.exists():
        print(f"Error: Directory already exists at {target_dir}", file=sys.stderr)
        sys.exit(1)

    # Normalize base_url (remove trailing slash)
    base_url = base_url.strip().rstrip("/")
    if not base_url.startswith("http://") and not base_url.startswith("https://"):
        base_url = "https://" + base_url

    pascal_name = to_pascal_case(name)

    # 1. Create directories
    src_dir = target_dir / "src"
    src_dir.mkdir(parents=True, exist_ok=True)

    # 2. Generate manifest.json
    manifest = {
        "id": ext_id,
        "name": name,
        "version": "1.0.0",
        "apiVersion": 1,
        "lang": lang,
        "baseUrl": base_url,
        "iconUrl": f"{base_url}/favicon.ico",
        "webviewNeeded": False,
        "runnerConcurrency": 3,
        "runnerCooldown": 1000,
        "maxAttempts": 3,
    }
    manifest_path = target_dir / "manifest.json"
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    # 3. Generate Cargo.toml
    cargo_toml = f"""[package]
name = "{ext_id}"
version.workspace = true
edition.workspace = true

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
bunori-sdk.workspace = true
"""
    cargo_path = target_dir / "Cargo.toml"
    with open(cargo_path, "w", encoding="utf-8") as f:
        f.write(cargo_toml)

    # 4. Generate starter src/lib.rs
    lib_template = """use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct __PASCAL_NAME__Source;

impl Source for __PASCAL_NAME__Source {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted = query.replace(' ', "+");
        let search_url = format!("{}/search?keyword={}&page={}", meta.base_url, formatted, page);
        let doc = host::document(&search_url, None)?;

        // TODO: Replace with the website's CSS selectors
        let item_sel = Selector::parse(".search-item").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse("h3.title a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img.cover").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for el in doc.select(&item_sel) {
            let Some(link) = el.select(&title_sel).next() else { continue; };
            let title = link.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = link.value().attr("href") else { continue; };
            let url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };

            let cover_url = el.select(&cover_sel).next()
                .and_then(|img| img.value().attr("src"))
                .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

            if !title.is_empty() && !url.is_empty() {
                results.push(SearchResultDto {
                    url,
                    title,
                    cover_url,
                    author: None,
                });
            }
        }

        Ok(results)
    }

    fn get_novel_details(&self, novel_url: &str) -> Result<NovelDto, String> {
        let meta = self.metadata();
        let doc = host::document(novel_url, None)?;

        // TODO: Replace with novel detail selectors
        let title_sel = Selector::parse("h1.title").map_err(|e| e.to_string())?;
        let desc_sel = Selector::parse(".description").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse(".cover img").map_err(|e| e.to_string())?;

        let title = doc.select(&title_sel).next()
            .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        let cover_url = doc.select(&cover_sel).next()
            .and_then(|img| img.value().attr("src"))
            .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

        let description = doc.select(&desc_sel).next()
            .map(|d| d.text().collect::<Vec<_>>().join("").trim().to_string());

        // TODO: Parse chapters
        let mut chapters = Vec::new();
        if let Ok(ch_sel) = Selector::parse("ul.chapters li a") {
            for (i, el) in doc.select(&ch_sel).enumerate() {
                let Some(href) = el.value().attr("href") else { continue; };
                let ch_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };
                let ch_title = el.text().collect::<Vec<_>>().join("").trim().to_string();

                chapters.push(ChapterDto {
                    url: ch_url,
                    title: if ch_title.is_empty() { format!("Chapter {}", i + 1) } else { ch_title },
                    index: (i + 1) as i32,
                    release_date: None,
                    scanlation: None,
                });
            }
        }

        Ok(NovelDto {
            url: novel_url.to_string(),
            title,
            author: None,
            cover_url,
            description,
            status: None,
            genres: Vec::new(),
            chapters,
            extra: HashMap::new(),
        })
    }

    fn get_chapter_content(&self, chapter_url: &str) -> Result<Option<String>, String> {
        let doc = host::document(chapter_url, None)?;

        // TODO: Replace with chapter content selector
        let Ok(sel) = Selector::parse("#chapter-content, .chapter-text") else {
            return Ok(None);
        };

        let Some(el) = doc.select(&sel).next() else {
            return Ok(None);
        };

        let mut content = el.inner_html();
        // Strip unwanted tags
        for rem in &["script", "style", "ins"] {
            let open = format!("<{}", rem);
            let close = format!("</{}>", rem);
            while let Some(s) = content.find(&open) {
                if let Some(e) = content[s..].find(&close) {
                    content.replace_range(s..s + e + close.len(), "");
                } else {
                    break;
                }
            }
        }

        Ok(Some(content.trim().to_string()))
    }
}

export_source!(__PASCAL_NAME__Source);
"""
    lib_rs = lib_template.replace("__PASCAL_NAME__", pascal_name)
    lib_path = src_dir / "lib.rs"
    with open(lib_path, "w", encoding="utf-8") as f:
        f.write(lib_rs)

    print(f"\n✨ Successfully created new extension: {name} ({ext_id})")
    print(f"  📁 Location:  sources/{ext_id}/")
    print(f"  📄 Manifest:  sources/{ext_id}/manifest.json")
    print(f"  ⚙ Cargo:     sources/{ext_id}/Cargo.toml")
    print(f"  🦀 Code:      sources/{ext_id}/src/lib.rs")
    print("\nNext steps:")
    print(f"  1. Edit sources/{ext_id}/src/lib.rs and add your CSS selectors.")
    print(f"  2. Test compilation: cargo check --package {ext_id} --target wasm32-unknown-unknown")
    print(f"  3. Package it:        python tools/package_extensions.py --single {ext_id}\n")


def main():
    parser = argparse.ArgumentParser(description="Create a new Bunori extension skeleton.")
    parser.add_argument("-n", "--name", help="Human-readable extension name (e.g. 'Novel Hi')")
    parser.add_argument("-u", "--url", help="Base URL for the website (e.g. 'https://novelhi.com')")
    parser.add_argument("-i", "--id", help="Extension identifier (optional, default: derived from name)")
    parser.add_argument("-l", "--lang", default="en", help="Language code (default: 'en')")

    args = parser.parse_args()

    name = args.name
    if not name:
        try:
            name = input("Extension Name (e.g. Novel Hi): ").strip()
        except (KeyboardInterrupt, EOFError):
            print("\nCancelled.")
            sys.exit(0)

    if not name:
        print("Error: Extension name cannot be empty.", file=sys.stderr)
        sys.exit(1)

    url = args.url
    if not url:
        try:
            url = input("Base URL (e.g. https://novelhi.com): ").strip()
        except (KeyboardInterrupt, EOFError):
            print("\nCancelled.")
            sys.exit(0)

    if not url:
        print("Error: Base URL cannot be empty.", file=sys.stderr)
        sys.exit(1)

    create_extension(name=name, base_url=url, ext_id=args.id, lang=args.lang)


if __name__ == "__main__":
    main()
