use bunori_sdk::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct NovelArchiveSource;

#[derive(Deserialize)]
struct NaSearchResponse {
    #[serde(default)]
    novels: Vec<NaSearchNovel>,
}

#[derive(Deserialize)]
struct NaSearchNovel {
    id: String,
    title: String,
    author: Option<String>,
    cover_url: Option<String>,
}

#[derive(Deserialize)]
struct NaDetailResponse {
    novel: Option<NaNovelDetail>,
}

#[derive(Deserialize)]
struct NaNovelDetail {
    title: Option<String>,
    author: Option<String>,
    cover_url: Option<String>,
    description: Option<String>,
    #[serde(default)]
    chapter_names: Vec<String>,
    #[serde(default)]
    total_chapters: Option<serde_json::Value>,
    #[serde(default)]
    sources: Vec<NaSourceItem>,
}

#[derive(Deserialize)]
struct NaSourcesResponse {
    #[serde(default)]
    sources: Vec<NaSourceItem>,
}

#[derive(Deserialize)]
struct NaSourceItem {
    #[serde(default)]
    id: String,
    label: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct NaSourceChaptersResponse {
    #[serde(default)]
    chapters: Vec<NaSourceChapter>,
}

#[derive(Deserialize)]
struct NaSourceChapter {
    number: Option<i32>,
    chapter_number: Option<i32>,
    index: Option<i32>,
    title: Option<String>,
    name: Option<String>,
    url: Option<String>,
}

#[derive(Deserialize)]
struct NaChapterResponse {
    chapter: Option<NaChapterContent>,
    content_html: Option<String>,
    content: Option<String>,
}

#[derive(Deserialize)]
struct NaChapterContent {
    content_html: Option<String>,
    content: Option<String>,
}

impl NovelArchiveSource {
    fn extract_id(url: &str) -> Option<String> {
        if let Some(idx) = url.find("id=") {
            let rest = &url[idx + 3..];
            let end = rest.find('&').unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
        let segments: Vec<&str> = url.split('/').collect();
        if let Some(last) = segments.last() {
            if last.len() == 24 {
                return Some(last.to_string());
            }
        }
        None
    }
}

impl Source for NovelArchiveSource {
    fn metadata(&self) -> SourceMetadata {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("Invalid manifest.json")
    }

    fn search(&self, query: &str, _page: i32) -> Result<Vec<SearchResultDto>, String> {
        let meta = self.metadata();
        let formatted = query.replace(' ', "+");
        let search_url = format!("{}/api/novels?search={}&fuzzy=1", meta.base_url, formatted);
        let resp = host::get(&search_url, None)?;

        let search_data: NaSearchResponse = serde_json::from_str(&resp).map_err(|e| e.to_string())?;

        let results = search_data.novels.into_iter().map(|n| {
            let cover_url = n.cover_url.map(|c| {
                if c.starts_with('/') {
                    format!("{}{}", meta.base_url, c)
                } else {
                    c
                }
            });

            SearchResultDto {
                url: format!("{}/novel?id={}", meta.base_url, n.id),
                title: n.title,
                cover_url,
                author: n.author,
            }
        }).collect();

        Ok(results)
    }

    fn get_novel_details(&self, novel_url: &str) -> Result<NovelDto, String> {
        let meta = self.metadata();
        let novel_id = Self::extract_id(novel_url)
            .ok_or_else(|| format!("Could not extract novel ID from {}", novel_url))?;

        let api_url = format!("{}/api/novels/{}", meta.base_url, novel_id);
        let resp = host::get(&api_url, None)?;
        let detail_resp: NaDetailResponse = serde_json::from_str(&resp).map_err(|e| e.to_string())?;

        let novel = detail_resp.novel.ok_or_else(|| "Missing 'novel' object in response".to_string())?;

        let cover_url = novel.cover_url.map(|c| {
            if c.starts_with('/') {
                format!("{}{}", meta.base_url, c)
            } else {
                c
            }
        });

        let mut chapters = Vec::new();

        // 1. Primary source chapters
        if !novel.chapter_names.is_empty() {
            for (i, name) in novel.chapter_names.into_iter().enumerate() {
                let number = (i + 1) as i32;
                let title = if name.trim().is_empty() { format!("Chapter {}", number) } else { name };
                chapters.push(ChapterDto {
                    url: format!("{}/api/novels/{}/chapters/{}", meta.base_url, novel_id, number),
                    title,
                    index: number,
                    release_date: None,
                    scanlation: Some(meta.name.clone()),
                });
            }
        } else {
            let total = novel.total_chapters.and_then(|v| {
                if let Some(s) = v.as_str() {
                    s.parse::<i32>().ok()
                } else {
                    v.as_i64().map(|n| n as i32)
                }
            }).unwrap_or(0);

            for number in 1..=total {
                chapters.push(ChapterDto {
                    url: format!("{}/api/novels/{}/chapters/{}", meta.base_url, novel_id, number),
                    title: format!("Chapter {}", number),
                    index: number,
                    release_date: None,
                    scanlation: Some(meta.name.clone()),
                });
            }
        }

        // 2. External sources
        let mut sources = novel.sources;
        if sources.is_empty() {
            let sources_url = format!("{}/api/novels/{}/sources", meta.base_url, novel_id);
            if let Ok(src_resp) = host::get(&sources_url, None) {
                if let Ok(parsed) = serde_json::from_str::<NaSourcesResponse>(&src_resp) {
                    sources = parsed.sources;
                }
            }
        }

        let mut seen_source_ids = HashSet::new();
        for src in sources {
            let src_id = src.id.trim().to_string();
            if src_id.is_empty() || seen_source_ids.contains(&src_id) {
                continue;
            }
            seen_source_ids.insert(src_id.clone());
            let src_label = src.label.or(src.name).unwrap_or_else(|| src_id.clone());

            let chapters_url = format!("{}/api/novels/{}/sources/{}/chapters", meta.base_url, novel_id, src_id);
            if let Ok(ch_resp) = host::get(&chapters_url, None) {
                if let Ok(ch_data) = serde_json::from_str::<NaSourceChaptersResponse>(&ch_resp) {
                    for (i, c) in ch_data.chapters.into_iter().enumerate() {
                        let num = c.number.or(c.chapter_number).or(c.index).unwrap_or((i + 1) as i32);
                        let title = c.title.or(c.name).unwrap_or_else(|| format!("Chapter {}", num));
                        let ch_url = c.url.map(|u| {
                            if u.starts_with("http") { u } else { format!("{}{}", meta.base_url, u) }
                        }).unwrap_or_else(|| {
                            format!("{}/api/novels/{}/sources/{}/chapters/{}", meta.base_url, novel_id, src_id, num)
                        });

                        chapters.push(ChapterDto {
                            url: ch_url,
                            title,
                            index: num,
                            release_date: None,
                            scanlation: Some(src_label.clone()),
                        });
                    }
                }
            }
        }

        chapters.sort_by(|a, b| a.index.cmp(&b.index).then_with(|| a.scanlation.cmp(&b.scanlation)));

        Ok(NovelDto {
            url: novel_url.to_string(),
            title: novel.title.unwrap_or_default(),
            author: novel.author,
            cover_url,
            description: novel.description,
            status: None,
            genres: Vec::new(),
            chapters,
            extra: HashMap::new(),
        })
    }

    fn get_chapter_content(&self, chapter_url: &str) -> Result<Option<String>, String> {
        let meta = self.metadata();
        let resp = host::get(chapter_url, None)?;
        let parsed: NaChapterResponse = serde_json::from_str(&resp).map_err(|e| e.to_string())?;

        let html_content = parsed.chapter.as_ref().and_then(|c| c.content_html.clone())
            .or(parsed.content_html);

        if let Some(html) = html_content {
            let fixed = html
                .replace("src=\"/", &format!("src=\"{}/", meta.base_url))
                .replace("src='/", &format!("src='{}/", meta.base_url));
            return Ok(Some(fixed));
        }

        let text_content = parsed.chapter.as_ref().and_then(|c| c.content.clone())
            .or(parsed.content);

        if let Some(text) = text_content {
            let lines: Vec<String> = text.split('\n')
                .filter(|line| !line.trim().is_empty())
                .map(|line| format!("<p>{}</p>", line.trim()))
                .collect();
            let joined = lines.join("\n")
                .replace("src=\"/", &format!("src=\"{}/", meta.base_url))
                .replace("src='/", &format!("src='{}/", meta.base_url));
            return Ok(Some(joined));
        }

        Ok(None)
    }
}

export_source!(NovelArchiveSource);
