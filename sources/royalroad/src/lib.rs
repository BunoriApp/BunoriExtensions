use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct RoyalRoadSource;

impl Source for RoyalRoadSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted_query = query.replace(' ', "+");
        let search_url = format!("{}/fictions/search?title={}&page={}", meta.base_url, formatted_query, page);
        let doc = host::document(&search_url, None)?;

        let item_sel = Selector::parse(".fiction-list-item").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse(".fiction-title a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img[data-type='cover']").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".author a").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for element in doc.select(&item_sel) {
            let Some(title_el) = element.select(&title_sel).next() else { continue; };
            let title = title_el.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = title_el.value().attr("href") else { continue; };
            let url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };

            let cover_url = element.select(&cover_sel).next().and_then(|img| {
                img.value().attr("src").map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) })
            });

            let author = element.select(&author_sel).next().map(|a| {
                a.text().collect::<Vec<_>>().join("").trim().to_string()
            });

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

        let title_sel = Selector::parse(".fic-header h1").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".fic-header h4 a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse(".fic-header img.thumbnail").map_err(|e| e.to_string())?;
        let desc_sel_hidden = Selector::parse(".description .hidden-content").map_err(|e| e.to_string())?;
        let desc_sel = Selector::parse(".description").map_err(|e| e.to_string())?;

        let title = doc.select(&title_sel).next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        let author = doc.select(&author_sel).next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string());

        let cover_url = doc.select(&cover_sel).next()
            .and_then(|el| el.value().attr("src"))
            .map(|src| if src.starts_with("http") { src.to_string() } else { format!("{}{}", meta.base_url, src) })
            .filter(|url| !url.contains("nocover"));

        let description = doc.select(&desc_sel_hidden).next()
            .or_else(|| doc.select(&desc_sel).next())
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string());

        // Parse chapters from window.chapters script or table fallback
        let mut chapters = Vec::new();
        let html_str = doc.html();
        if let Some(idx) = html_str.find("window.chapters") {
            let rest = &html_str[idx + "window.chapters".len()..];
            if let Some(start_bracket) = rest.find('[') {
                let array_slice = &rest[start_bracket..];
                if let Some(end_bracket) = array_slice.find("];") {
                    let json_str = &array_slice[..=end_bracket];
                    if let Ok(json_arr) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                        for (i, obj) in json_arr.into_iter().enumerate() {
                            let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let raw_url = obj.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let chap_url = if raw_url.starts_with("http") { raw_url } else { format!("{}{}", meta.base_url, raw_url) };
                            chapters.push(ChapterDto {
                                url: chap_url,
                                title,
                                index: (i + 1) as i32,
                                release_date: None,
                                scanlation: None,
                            });
                        }
                    }
                }
            }
        }

        if chapters.is_empty() {
            if let Ok(tr_sel) = Selector::parse("#chapters tbody tr.chapter-row") {
                if let Ok(a_sel) = Selector::parse("a[href]") {
                    for (i, tr) in doc.select(&tr_sel).enumerate() {
                        if let Some(link) = tr.select(&a_sel).next() {
                            let title = link.text().collect::<Vec<_>>().join("").trim().to_string();
                            let href = link.value().attr("href").unwrap_or("");
                            let chap_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };
                            chapters.push(ChapterDto {
                                url: chap_url,
                                title,
                                index: (i + 1) as i32,
                                release_date: None,
                                scanlation: None,
                            });
                        }
                    }
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
        let Ok(content_sel) = Selector::parse(".chapter-content") else {
            return Ok(None);
        };
        let Some(content_el) = doc.select(&content_sel).next() else {
            return Ok(None);
        };

        let mut inner = content_el.inner_html();
        // Remove anti-piracy warning spans
        for pattern in &["Royal Road is the home of this novel", "support the author", "stolen from"] {
            while let Some(idx) = inner.to_lowercase().find(&pattern.to_lowercase()) {
                // Remove the surrounding tag or text chunk
                let start = inner[..idx].rfind('<').unwrap_or(idx);
                let end = inner[idx..].find('>').map(|e| idx + e + 1).unwrap_or(idx + pattern.len());
                inner.replace_range(start..end, "");
            }
        }

        Ok(Some(inner.trim().to_string()))
    }

    fn get_listings(&self) -> Vec<ListingDto> {
        vec![
            ListingDto { id: "best-rated".to_string(), name: "Best Rated".to_string() },
            ListingDto { id: "trending".to_string(), name: "Trending".to_string() },
            ListingDto { id: "popular".to_string(), name: "Popular this week".to_string() },
        ]
    }

    fn get_listing_novels(&self, listing_id: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let endpoint = match listing_id {
            "best-rated" => "fictions/best-rated",
            "trending" => "fictions/trending",
            "popular" => "fictions/weekly-popular",
            _ => "fictions/best-rated",
        };
        let url = format!("{}/{}?page={}", meta.base_url, endpoint, page);
        let doc = host::document(&url, None)?;

        let item_sel = Selector::parse(".fiction-list-item").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse(".fiction-title a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img[data-type='cover']").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".author a").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for element in doc.select(&item_sel) {
            let Some(title_el) = element.select(&title_sel).next() else { continue; };
            let title = title_el.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = title_el.value().attr("href") else { continue; };
            let novel_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };

            let cover_url = element.select(&cover_sel).next().and_then(|img| {
                img.value().attr("src").map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) })
            });

            let author = element.select(&author_sel).next().map(|a| {
                a.text().collect::<Vec<_>>().join("").trim().to_string()
            });

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

export_source!(RoyalRoadSource);
