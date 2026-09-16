use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct NovelBinsSource;

impl Source for NovelBinsSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, _page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted_query = query.replace(' ', "+");
        let search_url = format!("{}/search-results/?query={}", meta.base_url, formatted_query);
        let doc = host::document(&search_url, None)?;

        let item_sel = Selector::parse(".mt-card-item").map_err(|e| e.to_string())?;
        let name_sel = Selector::parse("h3.mt-card-name").map_err(|e| e.to_string())?;
        let avatar_sel = Selector::parse(".mt-card-avatar a").map_err(|e| e.to_string())?;
        let avatar_div_sel = Selector::parse(".mt-card-avatar").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for el in doc.select(&item_sel) {
            let title = el.select(&name_sel).next()
                .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
                .unwrap_or_default();

            let url = el.select(&avatar_sel).next()
                .and_then(|a| a.value().attr("href"))
                .map(|h| if h.starts_with("http") { h.to_string() } else { format!("{}{}", meta.base_url, h) })
                .unwrap_or_default();

            let cover_url = el.select(&avatar_div_sel).next()
                .and_then(|div| div.value().attr("style"))
                .and_then(|style| {
                    if let Some(start) = style.find("url('") {
                        let rest = &style[start + 5..];
                        rest.find("')").map(|end| rest[..end].to_string())
                    } else {
                        None
                    }
                });

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
        let total_start = host::time_ms();
        log_info!("[{}] Starting get_novel_details for {}", meta.id, novel_url);

        let t_doc0 = host::time_ms();
        let doc = host::document(novel_url, None)?;
        let t_doc1 = host::time_ms();
        log_info!("[{}] Fetched and parsed landing page in {}ms", meta.id, t_doc1 - t_doc0);

        let title_sel = Selector::parse(".novel-short-info h1").map_err(|e| e.to_string())?;
        let p_sel = Selector::parse(".novel-short-info p").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img.novel-photo").map_err(|e| e.to_string())?;

        let title = doc.select(&title_sel).next()
            .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        let mut author = None;
        let mut description = None;

        let p_elements: Vec<_> = doc.select(&p_sel).collect();
        for p in &p_elements {
            let text = p.text().collect::<Vec<_>>().join("").trim().to_string();
            if text.contains("Author:") {
                author = Some(text.replace("Author:", "").trim().to_string());
            }
        }
        if p_elements.len() >= 8 {
            description = Some(p_elements[7].text().collect::<Vec<_>>().join("").trim().to_string());
        }

        let cover_url = doc.select(&cover_sel).next()
            .and_then(|img| img.value().attr("src"))
            .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

        // Chapters
        let permalink = novel_url.trim_end_matches('/').split('/').last().unwrap_or("").to_string();
        let mut novel_id = permalink.split('-').last().filter(|s| s.chars().all(|c| c.is_ascii_digit())).unwrap_or("").to_string();

        if novel_id.is_empty() {
            if let Ok(bm_sel) = Selector::parse("a[href^='javascript:bookmark']") {
                if let Some(bm) = doc.select(&bm_sel).next() {
                    if let Some(href) = bm.value().attr("href") {
                        if let Some(start) = href.find('\'') {
                            let rest = &href[start + 1..];
                            if let Some(end) = rest.find('\'') {
                                novel_id = rest[..end].to_string();
                            }
                        }
                    }
                }
            }
        }

        let mut chapters = Vec::new();
        let tab_sel = Selector::parse("a.ch[data-toggle='tab']").map_err(|e| e.to_string())?;
        let tab_links: Vec<_> = doc.select(&tab_sel).collect();

        let t_chap0 = host::time_ms();
        if tab_links.is_empty() {
            if let Ok(ch_sel) = Selector::parse(".chapters .mt-card-item h3.mt-card-name a") {
                for (i, a) in doc.select(&ch_sel).enumerate() {
                    let ch_title = a.text().collect::<Vec<_>>().join("").trim().to_string();
                    let href = a.value().attr("href").unwrap_or("");
                    let ch_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };
                    chapters.push(ChapterDto {
                        url: ch_url,
                        title: ch_title,
                        index: (i + 1) as i32,
                        release_date: None,
                        scanlation: None,
                    });
                }
            }
        } else {
            log_info!("[{}] Found {} chapter tab(s)", meta.id, tab_links.len());
            for (tab_i, tab) in tab_links.into_iter().enumerate() {
                let tab_index = tab.value().attr("href").unwrap_or("").replace('#', "");
                let ajax_url = format!("{}/ajax/", meta.base_url);
                let body = format!("action=get_chapters&id={}&tab={}", novel_id, tab_index);
                let mut hdrs = HashMap::new();
                hdrs.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());
                hdrs.insert("Referer".to_string(), novel_url.to_string());
                hdrs.insert("Origin".to_string(), meta.base_url.clone());
                hdrs.insert("Content-Type".to_string(), "application/x-www-form-urlencoded; charset=UTF-8".to_string());

                let t_tab0 = host::time_ms();
                if let Ok(resp) = host::post(&ajax_url, &body, Some(hdrs)) {
                    let fetch_ms = host::time_ms() - t_tab0;
                    let t_j0 = host::time_ms();
                    if let Ok(json_arr) = serde_json::from_str::<Vec<serde_json::Value>>(&resp) {
                        let parse_ms = host::time_ms() - t_j0;
                        log_info!("[{}] Tab [{}] (id={}): fetched in {}ms, parsed {} chapters in {}ms",
                            meta.id, tab_i + 1, tab_index, fetch_ms, json_arr.len(), parse_ms);

                        for item in json_arr {
                            let chap_num = item.get("chapter").and_then(|v| v.as_str()).unwrap_or("");
                            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let num = chap_num.parse::<i32>().unwrap_or((chapters.len() + 1) as i32);
                            let ch_url = format!("{}/novel/{}/chapter/{}/", meta.base_url, permalink, chap_num);
                            chapters.push(ChapterDto {
                                url: ch_url,
                                title,
                                index: num,
                                release_date: None,
                                scanlation: None,
                            });
                        }
                    }
                }
            }
        }
        log_info!("[{}] Extracted {} chapters in {}ms", meta.id, chapters.len(), host::time_ms() - t_chap0);

        chapters.sort_by_key(|c| c.index);
        log_info!("[{}] TOTAL get_novel_details completed in {}ms", meta.id, host::time_ms() - total_start);

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

        for sel_str in &[".reader", "#chr-content", "#chapter-content"] {
            if let Ok(sel) = Selector::parse(sel_str) {
                if let Some(el) = doc.select(&sel).next() {
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
                    return Ok(Some(content.trim().to_string()));
                }
            }
        }
        Ok(None)
    }

    fn get_listings(&self) -> Vec<ListingDto> {
        vec![
            ListingDto { id: "popular".to_string(), name: "Popular Novels".to_string() },
            ListingDto { id: "latest".to_string(), name: "Latest Release".to_string() },
        ]
    }

    fn get_listing_novels(&self, listing_id: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let sort = if listing_id == "latest" { "latest-release-novel" } else { "hot-novel" };
        let url = format!("{}/sort/{}?page={}", meta.base_url, sort, page);
        let doc = host::document(&url, None)?;

        let row_sel = Selector::parse(".list-novel .row").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse("h3.novel-title a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse("img.cover").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for row in doc.select(&row_sel) {
            let Some(t_el) = row.select(&title_sel).next() else { continue; };
            let title = t_el.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = t_el.value().attr("href") else { continue; };
            let novel_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };

            let cover_url = row.select(&cover_sel).next()
                .and_then(|img| img.value().attr("src"))
                .map(|s| if s.starts_with("http") { s.to_string() } else { format!("{}{}", meta.base_url, s) });

            if !title.is_empty() && !novel_url.is_empty() {
                results.push(SearchResultDto {
                    url: novel_url,
                    title,
                    cover_url,
                    author: None,
                });
            }
        }

        Ok(results)
    }
}

export_source!(NovelBinsSource);
