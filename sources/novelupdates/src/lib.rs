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

    fn extract_chapter_number(raw: &str, fallback: i32) -> i32 {
        let lower = raw.to_lowercase();
        if let Some(c_pos) = lower.find('c') {
            let after_c = lower[c_pos + 1..].trim_start();
            let num_str: String = after_c.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            if let Ok(num) = num_str.parse::<i32>() {
                return num;
            }
        }
        let mut digits = String::new();
        for ch in lower.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if !digits.is_empty() {
                break;
            }
        }
        digits.parse::<i32>().unwrap_or(fallback)
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
        host::log(2, &format!("Fetching novel document: {}", novel_url));
        let doc = host::document(novel_url, None)?;
        host::log(2, "Fetched novel document successfully");

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
        host::log(2, &format!("Found post_id: {}", post_id));

        let ajax_url = format!("{}/wp-admin/admin-ajax.php", meta.base_url);

        let mut headers = HashMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert("Referer".to_string(), novel_url.to_string());

        // 1. Fetch the list of translation groups for this novel
        let group_form = format!("action=nd_getgroupnovel&mygrr=0&mypostid={}", post_id);
        host::log(2, "Fetching groups...");
        let groups = match host::post(&ajax_url, &group_form, Some(headers.clone())) {
            Ok(group_html) => {
                host::log(2, &format!("Group HTML received, length: {}", group_html.len()));
                let group_doc = Html::parse_fragment(&group_html);
                let mut grps = Vec::new();
                if let Ok(input_sel) = Selector::parse("input.grp-filter-attr") {
                    for input in group_doc.select(&input_sel) {
                        if let Some(grp_id) = input.value().attr("value") {
                            let id_attr = input.value().attr("id").unwrap_or("");
                            let label_sel_str = format!("label[for='{}']", id_attr);
                            let group_name = if let Ok(lbl_sel) = Selector::parse(&label_sel_str) {
                                group_doc
                                    .select(&lbl_sel)
                                    .next()
                                    .map(|lbl| lbl.text().collect::<Vec<_>>().join("").trim().to_string())
                                    .unwrap_or_else(|| grp_id.to_string())
                            } else {
                                grp_id.to_string()
                            };
                            let name = if group_name.is_empty() {
                                grp_id.to_string()
                            } else {
                                group_name
                            };
                            grps.push((grp_id.to_string(), name));
                        }
                    }
                }
                grps
            }
            Err(e) => {
                host::log(3, &format!("Error fetching groups: {}", e));
                Vec::new()
            }
        };
        host::log(2, &format!("Found {} groups", groups.len()));

        let li_sel = Selector::parse("li.sp_li_chp").map_err(|e| e.to_string())?;
        let a_sel = Selector::parse("a").map_err(|e| e.to_string())?;

        let mut raw_chapters = Vec::new();

        if !groups.is_empty() {
            // Fetch chapters per translation group so each chapter gets its scanlation source name
            for (grp_id, group_name) in &groups {
                host::log(2, &format!("Fetching chapters for group '{}' (id: {})...", group_name, grp_id));
                let form_body = format!(
                    "action=nd_getchapters&mygrr=0&mygrpfilter={}&mypostid={}",
                    grp_id, post_id
                );
                match host::post(&ajax_url, &form_body, Some(headers.clone())) {
                    Ok(chapters_html) => {
                        host::log(2, &format!("Received {} bytes of chapter HTML for '{}'", chapters_html.len(), group_name));
                        let chapters_doc = Html::parse_fragment(&chapters_html);
                        let mut group_chapters = Vec::new();

                        for el in chapters_doc.select(&li_sel) {
                            for a in el.select(&a_sel) {
                                let href = a.value().attr("href").unwrap_or("");
                                if href.contains("/extnu/") {
                                    let ch_url = if href.starts_with("//") {
                                        format!("https:{}", href)
                                    } else if href.starts_with('/') {
                                        format!("{}{}", meta.base_url, href)
                                    } else {
                                        href.to_string()
                                    };

                                    let name = a.text().collect::<Vec<_>>().join("").trim().to_string();
                                    let title = Self::format_chapter_title(&name);
                                    let index = Self::extract_chapter_number(&name, (group_chapters.len() + 1) as i32);
                                    group_chapters.push(ChapterDto {
                                        url: ch_url,
                                        title,
                                        index,
                                        release_date: None,
                                        scanlation: Some(group_name.clone()),
                                    });
                                    break;
                                }
                            }
                        }

                        host::log(2, &format!("Parsed {} chapters for '{}'", group_chapters.len(), group_name));
                        // NovelUpdates returns chapters in reverse chronological order.
                        // Reverse so Chapter 1 comes first within each group.
                        group_chapters.reverse();
                        raw_chapters.extend(group_chapters);
                    }
                    Err(e) => {
                        host::log(3, &format!("Failed to fetch chapters for group '{}': {}", group_name, e));
                    }
                }
            }
        }

        // Fallback: If no groups were found, fetch all chapters in one call
        if raw_chapters.is_empty() {
            let form_body = format!("action=nd_getchapters&mygrr=0&mypostid={}", post_id);
            let chapters_html = host::post(&ajax_url, &form_body, Some(headers))?;
            let chapters_doc = Html::parse_fragment(&chapters_html);

            let mut fallback_chapters = Vec::new();
            for el in chapters_doc.select(&li_sel) {
                for a in el.select(&a_sel) {
                    let href = a.value().attr("href").unwrap_or("");
                    if href.contains("/extnu/") {
                        let ch_url = if href.starts_with("//") {
                            format!("https:{}", href)
                        } else if href.starts_with('/') {
                            format!("{}{}", meta.base_url, href)
                        } else {
                            href.to_string()
                        };

                        let name = a.text().collect::<Vec<_>>().join("").trim().to_string();
                        let title = Self::format_chapter_title(&name);
                        let index = Self::extract_chapter_number(&name, (fallback_chapters.len() + 1) as i32);
                        fallback_chapters.push(ChapterDto {
                            url: ch_url,
                            title,
                            index,
                            release_date: None,
                            scanlation: None,
                        });
                        break;
                    }
                }
            }
            fallback_chapters.reverse();
            raw_chapters.extend(fallback_chapters);
        }

        let chapters = raw_chapters;

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
            ".content-wrapper",
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
