use bunori_sdk::*;
use std::collections::HashMap;

#[derive(Default)]
pub struct NovelUpdatesSource;

impl NovelUpdatesSource {
    fn format_chapter_title(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return "Chapter".to_string();
        }

        let lower = trimmed.to_lowercase();
        let processed = if lower.starts_with("chapter") || lower.starts_with("volume") {
            trimmed.to_string()
        } else {
            let mut s = trimmed.to_string();
            // Match lnreader transformation: replace 'v' with 'volume ' and 'c' with ' chapter '
            if let Some(pos) = s.to_lowercase().find('v') {
                let after = s[pos + 1..].trim_start();
                if after.starts_with(|c: char| c.is_ascii_digit()) {
                    s = format!("{}Volume {}", &s[..pos], after);
                }
            }
            if let Some(pos) = s.to_lowercase().find('c') {
                let after = s[pos + 1..].trim_start();
                if after.starts_with(|c: char| c.is_ascii_digit()) {
                    s = format!("{} Chapter {}", &s[..pos], after);
                }
            }
            s
        };

        let capitalized = processed
            .replace("part", " Part ")
            .replace("Part", " Part ")
            .replace("ss", " SS ")
            .replace("SS", " SS ")
            .split_whitespace()
            .map(|word| {
                if word.eq_ignore_ascii_case("ss") {
                    "SS".to_string()
                } else {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if capitalized.is_empty() {
            "Chapter".to_string()
        } else {
            capitalized
        }
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

    fn parse_chapters(html: &str, base_url: &str) -> Vec<ChapterDto> {
        let Ok(li_sel) = Selector::parse("li.sp_li_chp") else {
            return Vec::new();
        };
        let Ok(a_sel) = Selector::parse("a") else {
            return Vec::new();
        };
        let doc = Html::parse_fragment(html);

        let mut raw_chapters = Vec::new();
        for el in doc.select(&li_sel) {
            let a_elements: Vec<_> = el.select(&a_sel).collect();
            if a_elements.is_empty() {
                continue;
            }

            // Match lnreader:
            // const chapterPath = 'https:' + chaptersCheerio(el).find('a').first().next().attr('href');
            // The chapter link is the 2nd <a> (first().next()), or any <a> containing /extnu/
            let ch_a = a_elements
                .iter()
                .find(|a| a.value().attr("href").map_or(false, |h| h.contains("/extnu/")))
                .copied()
                .or_else(|| {
                    if a_elements.len() >= 2 {
                        Some(a_elements[1])
                    } else {
                        a_elements.first().copied()
                    }
                });

            let Some(ch_a_el) = ch_a else { continue; };
            let Some(href) = ch_a_el.value().attr("href") else { continue; };
            let href = href.trim();
            if href.is_empty() {
                continue;
            }

            let ch_url = if href.starts_with("//") {
                format!("https:{}", href)
            } else if href.starts_with('/') {
                format!("{}{}", base_url, href)
            } else if href.starts_with("http://") || href.starts_with("https://") {
                href.to_string()
            } else {
                format!("{}/{}", base_url, href)
            };

            // In lnreader: chapter title is parsed from el.text(), or chapter link text
            let link_text = ch_a_el.text().collect::<Vec<_>>().join("").trim().to_string();
            let raw_name = if !link_text.is_empty() {
                link_text
            } else {
                el.text().collect::<Vec<_>>().join("").trim().to_string()
            };

            let title = Self::format_chapter_title(&raw_name);

            // Scanlation group name: if first <a> is a group link distinct from chapter <a>
            let scanlation = if a_elements.len() >= 2 && ch_a != Some(a_elements[0]) {
                let grp = a_elements[0].text().collect::<Vec<_>>().join("").trim().to_string();
                if !grp.is_empty() {
                    Some(grp)
                } else {
                    None
                }
            } else {
                None
            };

            raw_chapters.push((ch_url, title, raw_name, scanlation));
        }

        // NovelUpdates returns chapters in reverse chronological order (newest first).
        // Reverse so Chapter 1 comes first, matching lnreader: novel.chapters = chapters.reverse()
        raw_chapters.reverse();

        let mut chapters = Vec::with_capacity(raw_chapters.len());
        for (i, (ch_url, title, raw_name, scanlation)) in raw_chapters.into_iter().enumerate() {
            let index = Self::extract_chapter_number(&raw_name, (i + 1) as i32);
            chapters.push(ChapterDto {
                url: ch_url,
                title,
                index,
                release_date: None,
                scanlation,
            });
        }

        chapters
    }
}

impl Source for NovelUpdatesSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let longest = query
            .split('*')
            .max_by_key(|s| s.len())
            .unwrap_or(query);
        let cleaned = longest
            .replace(['\u{2018}', '\u{2019}'], "'")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("+");

        let search_url = format!(
            "{}/series-finder/?sf=1&sh={}&sort=srank&order=asc&pg={}",
            meta.base_url, cleaned, page
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

        let type_sel = Selector::parse("#showtype").ok();
        let novel_type = type_sel.and_then(|sel| {
            doc.select(&sel)
                .next()
                .map(|t| t.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty())
        });

        let mut description = doc
            .select(&desc_sel)
            .next()
            .map(|d| d.text().collect::<Vec<_>>().join("\n").trim().to_string());

        if let Some(nt) = novel_type {
            if let Some(ref mut desc) = description {
                desc.push_str(&format!("\n\nType: {}", nt));
            } else {
                description = Some(format!("Type: {}", nt));
            }
        }

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

        let mut headers = HashMap::new();
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert("Referer".to_string(), novel_url.to_string());

        // Match lnreader-plugins: single POST to admin-ajax.php with action=nd_getchapters&mygrr=0&mypostid=...
        let form_body = format!("action=nd_getchapters&mygrr=0&mypostid={}", post_id);
        let chapters = match host::post(&ajax_url, &form_body, Some(headers)) {
            Ok(html) => Self::parse_chapters(&html, &meta.base_url),
            Err(e) => {
                log_warn!("Failed to fetch chapters for novel {}: {}", novel_url, e);
                Vec::new()
            }
        };

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
            "[data-reader-article='true']",
            "[data-reader-article]",
        ];

        let mut matched_content = None;
        for sel_str in &content_selectors {
            if let Ok(sel) = Selector::parse(sel_str) {
                if let Some(el) = doc.select(&sel).next() {
                    let inner = el.inner_html();
                    if inner.trim().len() > 50 {
                        matched_content = Some(inner);
                        break;
                    }
                }
            }
        }

        // Fallback to article, main, or body if no specific content container matched
        if matched_content.is_none() {
            if let Ok(fallback_sel) = Selector::parse("article, main, body") {
                if let Some(el) = doc.select(&fallback_sel).next() {
                    let inner = el.inner_html();
                    if inner.trim().len() > 50 {
                        matched_content = Some(inner);
                    }
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
