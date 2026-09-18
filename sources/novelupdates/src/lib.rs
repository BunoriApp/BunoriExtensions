use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct NovelUpdatesSource;

impl NovelUpdatesSource {
    fn format_chapter_title(raw: &str) -> String {
        let mut text = raw.trim().to_string();
        if text.is_empty() {
            return "Chapter".to_string();
        }

        text = text
            .replace('v', "Volume ")
            .replace('c', " Chapter ")
            .replace("part", " Part ")
            .replace("ss", " SS ");

        let capitalized = text
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        capitalized
    }

    fn parse_novel_list(doc: &Html, base_url: &str) -> Result<Vec<SearchResultDto>, String> {
        let item_sel = Selector::parse("div.search_main_box_nu, div.w-blog-entry, .search_body_nu").map_err(|e| e.to_string())?;
        let title_sel = Selector::parse(".search_title > a, .w-blog-entry-title a, h2 a").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse(".search_img_nu img, .w-blog-entry-thumbnail img, img").map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for el in doc.select(&item_sel) {
            let Some(title_el) = el.select(&title_sel).next() else { continue; };
            let title = title_el.text().collect::<Vec<_>>().join("").trim().to_string();
            let Some(href) = title_el.value().attr("href") else { continue; };
            let url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{}{}", base_url, href)
            };

            let cover_url = el
                .select(&cover_sel)
                .next()
                .and_then(|img| img.value().attr("src"))
                .map(|s| {
                    if s.starts_with("http") {
                        s.to_string()
                    } else {
                        format!("{}{}", base_url, s)
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

}

impl Source for NovelUpdatesSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted = query.replace(' ', "+");
        let search_url = format!(
            "{}/series-finder/?sf=1&sh={}&sort=srank&order=asc&pg={}",
            meta.base_url, formatted, page
        );
        let doc = host::document(&search_url, None)?;
        Self::parse_novel_list(&doc, &meta.base_url)
    }

    fn get_novel_details(&self, novel_url: &str) -> Result<NovelDto, String> {
        let meta = self.metadata();
        let doc = host::document(novel_url, None)?;

        let title_sel = Selector::parse(".seriestitlenu").map_err(|e| e.to_string())?;
        let desc_sel = Selector::parse("#editdescription").map_err(|e| e.to_string())?;
        let cover_sel = Selector::parse(".wpb_wrapper img").map_err(|e| e.to_string())?;
        let author_sel = Selector::parse("#authtag").map_err(|e| e.to_string())?;
        let genre_sel = Selector::parse("#seriesgenre a").map_err(|e| e.to_string())?;
        let status_sel = Selector::parse("#editstatus").map_err(|e| e.to_string())?;

        let title = doc
            .select(&title_sel)
            .next()
            .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_else(|| "Untitled".to_string());

        let cover_url = doc
            .select(&cover_sel)
            .next()
            .and_then(|img| img.value().attr("src"))
            .map(|s| {
                if s.starts_with("http") {
                    s.to_string()
                } else {
                    format!("{}{}", meta.base_url, s)
                }
            });

        let description = doc
            .select(&desc_sel)
            .next()
            .map(|d| d.text().collect::<Vec<_>>().join("\n").trim().to_string());

        let author = {
            let authors: Vec<String> = doc
                .select(&author_sel)
                .map(|a| a.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if authors.is_empty() {
                None
            } else {
                Some(authors.join(", "))
            }
        };

        let genres: Vec<String> = doc
            .select(&genre_sel)
            .map(|g| g.text().collect::<Vec<_>>().join("").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let status = doc.select(&status_sel).next().map(|s| {
            let text = s.text().collect::<Vec<_>>().join("");
            if text.contains("Ongoing") {
                "Ongoing".to_string()
            } else {
                "Completed".to_string()
            }
        });

        let post_id_sel = Selector::parse("input#mypostid").map_err(|e| e.to_string())?;
        let post_id = doc
            .select(&post_id_sel)
            .next()
            .and_then(|input| input.value().attr("value"))
            .ok_or_else(|| {
                "Could not find 'mypostid' on NovelUpdates. Please open the novel in Webview once to authenticate/bypass protection.".to_string()
            })?;

        let ajax_url = format!("{}/wp-admin/admin-ajax.php", meta.base_url);
        let form_body = format!("action=nd_getchapters&mygrr=0&mypostid={}", post_id);

        let mut headers = HashMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert("Referer".to_string(), novel_url.to_string());

        let chapters_html = host::post(&ajax_url, &form_body, Some(headers))?;
        let chapters_doc = Html::parse_fragment(&chapters_html);

        let li_sel = Selector::parse("li.sp_li_chp").map_err(|e| e.to_string())?;
        let a_sel = Selector::parse("a").map_err(|e| e.to_string())?;

        let mut raw_chapters = Vec::new();
        for el in chapters_doc.select(&li_sel) {
            let a_tags: Vec<_> = el.select(&a_sel).collect();
            // In NovelUpdates AJAX response, the 1st <a> is the group link and the 2nd <a> is the chapter link
            let (scanlation, chapter_a) = if a_tags.len() >= 2 {
                let grp = a_tags[0].text().collect::<Vec<_>>().join("").trim().to_string();
                (if grp.is_empty() { None } else { Some(grp) }, Some(a_tags[1]))
            } else {
                (None, a_tags.first().copied())
            };

            if let Some(a) = chapter_a {
                let raw_href = a.value().attr("href").unwrap_or("");
                if raw_href.is_empty() {
                    continue;
                }

                let ch_url = if raw_href.starts_with("//") {
                    format!("https:{}", raw_href)
                } else if raw_href.starts_with('/') {
                    format!("{}{}", meta.base_url, raw_href)
                } else {
                    raw_href.to_string()
                };

                let name = a.text().collect::<Vec<_>>().join("").trim().to_string();
                let title = Self::format_chapter_title(&name);

                raw_chapters.push((title, ch_url, scanlation));
            }
        }

        // NovelUpdates returns chapters in reverse chronological order (newest first).
        // Reverse so Chapter 1 comes first, then assign 1-based sequential indices.
        raw_chapters.reverse();

        let chapters = raw_chapters
            .into_iter()
            .enumerate()
            .map(|(i, (title, url, scanlation))| ChapterDto {
                url,
                title,
                index: (i + 1) as i32,
                release_date: None,
                scanlation,
            })
            .collect();

        Ok(NovelDto {
            url: novel_url.to_string(),
            title,
            author,
            cover_url,
            description,
            status,
            genres,
            chapters,
            extra: HashMap::new(),
        })
    }

    fn get_chapter_content(&self, chapter_url: &str) -> Result<Option<String>, String> {
        let html = host::get(chapter_url, None)?;
        let doc = Html::parse_document(&html);

        // Check for Cloudflare / bot protection blocks
        if let Ok(title_sel) = Selector::parse("title") {
            if let Some(title_el) = doc.select(&title_sel).next() {
                let title_text = title_el.text().collect::<Vec<_>>().join("").to_lowercase();
                if title_text.contains("just a moment")
                    || title_text.contains("bot verification")
                    || title_text.contains("attention required")
                    || title_text.contains("captcha")
                    || title_text.contains("un instant")
                {
                    return Err("Cloudflare/Captcha detected. Please open in Webview to verify.".to_string());
                }
            }
        }

        // Selectors covering WordPress, Blogspot, and generic translation websites
        let content_selectors = [
            ".chapter__content",
            ".entry-content",
            ".text_story",
            ".post-content",
            ".contenta",
            ".single_post",
            ".main-content",
            ".reader-content",
            "#chapter-content",
            ".chapter-text",
            "#content",
            "#the-content",
            "article.post",
            ".chp_raw",
            ".post-body",
            ".content-post",
            ".halChap--kontenInner",
            "[data-tag='post-card']",
        ];

        let mut matched_content = None;
        for sel_str in &content_selectors {
            if let Ok(sel) = Selector::parse(sel_str) {
                if let Some(el) = doc.select(&sel).next() {
                    matched_content = Some(el.inner_html());
                    break;
                }
            }
        }

        let Some(mut content) = matched_content else {
            return Ok(None);
        };

        // Remove unwanted ads and bloat tags
        for rem in &[
            "script",
            "style",
            "ins",
            "noscript",
            "header",
            "footer",
            "nav",
        ] {
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
            ListingDto {
                id: "popular".to_string(),
                name: "Popular (All)".to_string(),
            },
            ListingDto {
                id: "popmonth".to_string(),
                name: "Popular (Month)".to_string(),
            },
            ListingDto {
                id: "latest".to_string(),
                name: "Latest Releases".to_string(),
            },
        ]
    }

    fn get_listing_novels(&self, listing_id: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let url = match listing_id {
            "popular" => format!("{}/series-ranking/?rank=popular&pg={}", meta.base_url, page),
            "popmonth" => format!("{}/series-ranking/?rank=popmonth&pg={}", meta.base_url, page),
            _ => format!("{}/series-finder/?sf=1&sort=sdate&order=desc&pg={}", meta.base_url, page),
        };
        let doc = host::document(&url, None)?;
        Self::parse_novel_list(&doc, &meta.base_url)
    }
}

export_source!(NovelUpdatesSource);
