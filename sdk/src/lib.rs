pub mod abi;
pub mod host;
pub mod models;

pub use abi::*;
pub use host::*;
pub use models::*;

// Re-export common dependencies so extensions don't have to declare them individually
pub use regex::{self, Regex};
pub use scraper::{self, Html, Selector};
pub use serde::{self, Deserialize, Serialize};
pub use serde_json;

pub trait Source: Default + Send + Sync + 'static {
    fn metadata(&self) -> SourceMetadata;
    fn search(&self, query: &str, page: i32) -> Result<Vec<SearchResultDto>, String>;
    fn get_novel_details(&self, url: &str) -> Result<NovelDto, String>;
    fn get_chapter_content(&self, url: &str) -> Result<Option<String>, String>;
    fn get_listings(&self) -> Vec<ListingDto> {
        Vec::new()
    }
    fn get_listing_novels(&self, _id: &str, _page: i32) -> Result<Vec<SearchResultDto>, String> {
        Ok(Vec::new())
    }
}

// avoids the need to wasm_js for the get_random function by defining random myself
#[no_mangle]
unsafe extern "Rust" fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), getrandom::Error> {
    let slice = std::slice::from_raw_parts_mut(dest, len);
    let mut x: u64 = 0x853c49e6748fea9b;
    for chunk in slice.chunks_mut(8) {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let bytes = (x.wrapping_mul(0x2545f4914f6cdd1d)).to_le_bytes();
        let l = chunk.len().min(8);
        chunk.copy_from_slice(&bytes[..l]);
    }
    Ok(())
}
