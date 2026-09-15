use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct NovelFullSource;

impl Source for NovelFullSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted_query = query.replace(' ', "+");
        let search_url = format!("{}/search?keyword={}&page={}", meta.base_url, formatted_query, page);
        let doc = host::document(&search_url, None)?;

        let row_sel = Selector::parse(".col-truyen-main .list-truyen .row").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse("h3.truyen-title a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img.cover").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".author").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for row in doc.select(&row_sel) {
            let Some(link) = row.select(&title_sel).next() else { continue; };
            let title = link.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = link.value().attr("href") else { continue; };
            let url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };

            let cover_url = row.select(&cover_sel).next()
                .and_then(|img| img.value().attr("src"))
                .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

            let author = row.select(&author_sel).next()
                .map(|a| a.text().collect::<Vec<_>>().join("").trim().to_string());

            if !title.is_empty() && !url.is_empty() {
                results.push(SearchResultDto {
                    url,
                    title,
                    cover_url,
                    author,
                });
            }
        }

        Ok(results)
    }

    fn get_novel_details(&self, novel_url: &str) -> Result<NovelDto, String> {
        let meta = self.metadata();
        let doc = host::document(novel_url, None)?;

        let title_sel = Selector::parse("h3.title").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".info a[href*='/author/']").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse(".book img").map_err(|e| e.to_string())?;
        let desc_sel = Selector::parse(".desc-text").map_err(|e| e.to_string())?;

        let title = doc.select(&title_sel).next()
            .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        let author = doc.select(&author_sel).next()
            .map(|a| a.text().collect::<Vec<_>>().join("").trim().to_string());

        let cover_url = doc.select(&cover_sel).next()
            .and_then(|img| img.value().attr("src"))
            .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

        let description = doc.select(&desc_sel).next()
            .map(|d| d.text().collect::<Vec<_>>().join("").trim().to_string());

        // Chapters
        let mut chapters = Vec::new();
        let mut novel_id = String::new();
        if let Ok(id_sel) = Selector::parse("#rating[data-novel-id], [data-novel-id]") {
            if let Some(el) = doc.select(&id_sel).next() {
                novel_id = el.value().attr("data-novel-id").unwrap_or("").to_string();
            }
        }

        if !novel_id.is_empty() {
            let ajax_url = format!("{}/ajax-chapter-option?novelId={}", meta.base_url, novel_id);
            if let Ok(resp) = host::get(&ajax_url, None) {
                let html_content = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) {
                    json.get("html").and_then(|h| h.as_str()).unwrap_or(&resp).to_string()
                } else {
                    resp
                };

                let ajax_doc = Html::parse_fragment(&html_content);
                if let Ok(opt_sel) = Selector::parse("option[value], a[href]") {
                    for (i, el) in ajax_doc.select(&opt_sel).enumerate() {
                        let path = el.value().attr("value").or_else(|| el.value().attr("href")).unwrap_or("");
                        if path.is_empty() { continue; }
                        let ch_url = if path.starts_with("http") {
                            path.to_string()
                        } else {
                            format!("{}{}", meta.base_url, path)
                        };

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
            }
        }

        // Fallback: table list
        if chapters.is_empty() {
            if let Ok(row_sel) = Selector::parse("ul.list-chapter li a") {
                for (i, el) in doc.select(&row_sel).enumerate() {
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
        }

        Ok(NovelDto {
            url: novel_url.to_string(),
            title,
            author,
            cover_url,
            description,
            status: None,
            genres: Vec::new(),
            chapters,
            extra: HashMap::new(),
        })
    }

    fn get_chapter_content(&self, chapter_url: &str) -> Result<Option<String>, String> {
        let html = host::get(chapter_url, None)?;
        let doc = Html::parse_document(&html);
        let Ok(sel) = Selector::parse("#chapter-content") else {
            return Ok(None);
        };
        let Some(el) = doc.select(&sel).next() else {
            return Ok(None);
        };

        let mut content = el.inner_html();
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

    fn get_listings(&self) -> Vec<ListingDto> {
        vec![
            ListingDto { id: "latest-release-novel".to_string(), name: "Latest Releases".to_string() },
            ListingDto { id: "hot-novel".to_string(), name: "Hot Novels".to_string() },
            ListingDto { id: "completed-novel".to_string(), name: "Completed Novels".to_string() },
        ]
    }

    fn get_listing_novels(&self, listing_id: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let url = format!("{}/{}?page={}", meta.base_url, listing_id, page);
        let doc = host::document(&url, None)?;

        let row_sel = Selector::parse(".col-truyen-main .list-truyen .row").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse("h3.truyen-title a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img.cover").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".author").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for row in doc.select(&row_sel) {
            let Some(link) = row.select(&title_sel).next() else { continue; };
            let title = link.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = link.value().attr("href") else { continue; };
            let novel_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };

            let cover_url = row.select(&cover_sel).next()
                .and_then(|img| img.value().attr("src"))
                .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

            let author = row.select(&author_sel).next()
                .map(|a| a.text().collect::<Vec<_>>().join("").trim().to_string());

            if !title.is_empty() && !novel_url.is_empty() {
                results.push(SearchResultDto {
                    url: novel_url,
                    title,
                    cover_url,
                    author,
                });
            }
        }

        Ok(results)
    }
}

export_source!(NovelFullSource);
