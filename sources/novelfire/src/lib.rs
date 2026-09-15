use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct NovelFireSource;

impl Source for NovelFireSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, _page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted = query.replace(' ', "%20");
        let search_url = format!("{}/ajax/searchLive?keyword={}&type=title", meta.base_url, formatted);
        let resp = host::get(&search_url, None)?;

        let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) else {
            return Ok(Vec::new());
        };

        let Some(data_arr) = json.get("data").and_then(|d| d.as_array()) else {
            return Ok(Vec::new());
        };

        let mut results = Vec::new();
        for item in data_arr {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let slug = item.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let image = item.get("image").and_then(|v| v.as_str()).unwrap_or("");

            if !title.is_empty() && !slug.is_empty() {
                let cover_url = if !image.is_empty() {
                    Some(format!("{}/{}", meta.base_url, image.trim_start_matches('/')))
                } else {
                    None
                };
                results.push(SearchResultDto {
                    url: format!("{}/book/{}", meta.base_url, slug),
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
        let clean_url = novel_url.trim_end_matches('/');
        let doc = host::document(clean_url, None)?;

        let title_sel = Selector::parse("h1.novel-title").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse(".author a[itemprop='author'], .author span[itemprop='author']").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse(".fixed-img figure.cover img").map_err(|e| e.to_string())?;
        let desc_sel = Selector::parse(".summary .content").map_err(|e| e.to_string())?;

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
        let mut post_id = String::new();
        if let Ok(id_sel) = Selector::parse("#novel-report, [report-post_id]") {
            if let Some(el) = doc.select(&id_sel).next() {
                post_id = el.value().attr("report-post_id").unwrap_or("").to_string();
            }
        }
        if post_id.is_empty() {
            let html = doc.html();
            if let Some(idx) = html.find("report-post_id=") {
                let rest = &html[idx + "report-post_id=".len()..];
                let rest = rest.trim_start_matches(|c| c == '"' || c == '\'');
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    post_id = digits;
                }
            }
        }

        let mut chapters = Vec::new();
        if !post_id.is_empty() {
            let ajax_url = format!(
                "{}/ajax/listChapterDataAjax?draw=1&start=0&length=-1&post_id={}&order[0][column]=0&order[0][dir]=asc&order[0][name]=cmm_posts_detail.n_sort&columns[0][data]=n_sort&columns[0][name]=cmm_posts_detail.n_sort&columns[0][searchable]=true&columns[0][orderable]=true&columns[0][search][value]=&columns[0][search][regex]=false&columns[1][data]=bookmark_created_at&columns[1][name]=bookmark_chapters.created_at&columns[1][searchable]=false&columns[1][orderable]=true&columns[1][search][value]=&columns[1][search][regex]=false&search[value]=&search[regex]=false&only_bookmark=false",
                meta.base_url, post_id
            );
            let mut hdrs = HashMap::new();
            hdrs.insert("Referer".to_string(), clean_url.to_string());
            hdrs.insert("X-Requested-With".to_string(), "XMLHttpRequest".to_string());

            if let Ok(resp) = host::get(&ajax_url, Some(hdrs)) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp) {
                    if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
                        for (i, item) in arr.iter().enumerate() {
                            let raw_title = item.get("title").and_then(|v| v.as_str())
                                .or_else(|| item.get("slug").and_then(|v| v.as_str()))
                                .unwrap_or("");
                            let n_sort = item.get("n_sort").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;

                            let chap_url = if n_sort > 0 {
                                format!("{}/chapter-{}", clean_url, n_sort)
                            } else {
                                let slug = item.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                                if !slug.is_empty() {
                                    format!("{}/{}", clean_url, slug)
                                } else {
                                    continue;
                                }
                            };

                            let idx = if n_sort > 0 { n_sort } else { (i + 1) as i32 };
                            let title = if raw_title.is_empty() { format!("Chapter {}", idx) } else { raw_title.to_string() };

                            chapters.push(ChapterDto {
                                url: chap_url,
                                title,
                                index: idx,
                                release_date: None,
                                scanlation: None,
                            });
                        }
                    }
                }
            }
        }

        if chapters.is_empty() {
            let list_url = format!("{}/chapters", clean_url);
            if let Ok(chap_doc) = host::document(&list_url, None) {
                if let Ok(a_sel) = Selector::parse("ul.chapter-list li a") {
                    for (i, a) in chap_doc.select(&a_sel).enumerate() {
                        let href = a.value().attr("href").unwrap_or("");
                        let chap_url = if href.starts_with("http") { href.to_string() } else { format!("{}{}", meta.base_url, href) };
                        let title = a.text().collect::<Vec<_>>().join("").trim().to_string();
                        chapters.push(ChapterDto {
                            url: chap_url,
                            title: if title.is_empty() { format!("Chapter {}", i + 1) } else { title },
                            index: (i + 1) as i32,
                            release_date: None,
                            scanlation: None,
                        });
                    }
                }
            }
        }

        chapters.sort_by_key(|c| c.index);

        Ok(NovelDto {
            url: clean_url.to_string(),
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
        let novel_url = chapter_url.rfind('/').map(|i| &chapter_url[..i]).unwrap_or(chapter_url);
        let mut hdrs = HashMap::new();
        hdrs.insert("Referer".to_string(), novel_url.to_string());

        let html = host::get(chapter_url, Some(hdrs))?;
        let doc = Html::parse_document(&html);
        let Ok(sel) = Selector::parse("#content") else {
            return Ok(None);
        };
        let Some(el) = doc.select(&sel).next() else {
            return Ok(None);
        };

        let mut content = el.inner_html();
        for rem in &["script", "style", "iframe"] {
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

export_source!(NovelFireSource);
