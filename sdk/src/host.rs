use crate::models::{HttpRequest, HttpResponse};
use std::collections::HashMap;

#[link(wasm_import_module = "bunori")]
extern "C" {
    fn host_http(req_ptr: i32, req_len: i32) -> u64;
    fn host_log(level: i32, msg_ptr: i32, msg_len: i32);
}

pub fn log(level: i32, msg: &str) {
    let bytes = msg.as_bytes();
    unsafe {
        host_log(level, bytes.as_ptr() as i32, bytes.len() as i32);
    }
}

pub fn http_request(req: &HttpRequest) -> Result<HttpResponse, String> {
    let req_json = serde_json::to_string(req).map_err(|e| e.to_string())?;
    let req_bytes = req_json.as_bytes();
    let packed = unsafe { host_http(req_bytes.as_ptr() as i32, req_bytes.len() as i32) };
    if packed == 0 {
        return Err("Host HTTP call returned null/error".to_string());
    }
    let res_ptr = (packed & 0xFFFF_FFFF) as i32;
    let res_len = (packed >> 32) as i32;
    let res_json = unsafe { crate::abi::read_string(res_ptr, res_len) };
    let res: HttpResponse = serde_json::from_str(&res_json)
        .map_err(|e| format!("Failed to parse HTTP response: {}: {}", e, res_json))?;
    Ok(res)
}

pub fn get(url: &str, headers: Option<HashMap<String, String>>) -> Result<String, String> {
    let req = HttpRequest {
        url: url.to_string(),
        method: "GET".to_string(),
        headers: headers.unwrap_or_default(),
        body: None,
    };
    let res = http_request(&req)?;
    if res.status_code < 200 || res.status_code >= 400 {
        return Err(format!("HTTP error status: {}", res.status_code));
    }
    Ok(res.body)
}

pub fn post(url: &str, body: &str, headers: Option<HashMap<String, String>>) -> Result<String, String> {
    let mut hdrs = headers.unwrap_or_default();
    if !hdrs.contains_key("Content-Type") && !hdrs.contains_key("content-type") {
        hdrs.insert("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string());
    }
    let req = HttpRequest {
        url: url.to_string(),
        method: "POST".to_string(),
        headers: hdrs,
        body: Some(body.to_string()),
    };
    let res = http_request(&req)?;
    if res.status_code < 200 || res.status_code >= 400 {
        return Err(format!("HTTP error status: {}", res.status_code));
    }
    Ok(res.body)
}

pub fn document(url: &str, headers: Option<HashMap<String, String>>) -> Result<scraper::Html, String> {
    let html = get(url, headers)?;
    Ok(scraper::Html::parse_document(&html))
}

pub fn clean_html(html: &str, selector_str: &str, remove_selectors: &[&str]) -> String {
    let doc = scraper::Html::parse_fragment(html);
    let Ok(sel) = scraper::Selector::parse(selector_str) else {
        return html.to_string();
    };

    let Some(element) = doc.select(&sel).next() else {
        return String::new();
    };

    let mut inner = element.inner_html();
    for rem in remove_selectors {
        let tag = rem.trim_start_matches('.');
        // Simple and robust stripping for scripts, styles, ads, comments
        if *rem == "script" {
            while let Some(start) = inner.find("<script") {
                if let Some(end) = inner[start..].find("</script>") {
                    inner.replace_range(start..start + end + 9, "");
                } else {
                    break;
                }
            }
        } else if *rem == "style" {
            while let Some(start) = inner.find("<style") {
                if let Some(end) = inner[start..].find("</style>") {
                    inner.replace_range(start..start + end + 8, "");
                } else {
                    break;
                }
            }
        } else if *rem == "ins" || *rem == "iframe" {
            let open = format!("<{}", rem);
            let close = format!("</{}>", rem);
            while let Some(start) = inner.find(&open) {
                if let Some(end) = inner[start..].find(&close) {
                    inner.replace_range(start..start + end + close.len(), "");
                } else {
                    break;
                }
            }
        } else {
            // Strip class or general regex
            let class_pattern = format!("class=\"{}", tag);
            let class_pattern2 = format!("class='{}", tag);
            // If present, remove tag
            if inner.contains(&class_pattern) || inner.contains(&class_pattern2) {
                // remove elements containing this class
            }
        }
    }
    inner.trim().to_string()
}
