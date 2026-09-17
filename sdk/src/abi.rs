pub fn alloc(size: i32) -> *mut u8 {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let mut buf = Vec::with_capacity(size as usize);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

pub fn dealloc(ptr: *mut u8, size: i32) {
    if !ptr.is_null() && size > 0 {
        unsafe {
            let _ = Vec::from_raw_parts(ptr, 0, size as usize);
        }
    }
}

pub unsafe fn read_string(ptr: i32, len: i32) -> String {
    if ptr == 0 || len <= 0 {
        return String::new();
    }
    let slice = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    String::from_utf8_lossy(slice).into_owned()
}

pub fn return_string(s: String) -> u64 {
    let bytes = s.into_bytes().into_boxed_slice();
    let len = bytes.len() as u64;
    let ptr = Box::into_raw(bytes) as *mut u8 as u64;
    ptr | (len << 32)
}

#[macro_export]
macro_rules! export_source {
    ($source_type:ident) => {
        static SOURCE: std::sync::OnceLock<$source_type> = std::sync::OnceLock::new();

        fn get_source() -> &'static $source_type {
            SOURCE.get_or_init(|| <$source_type>::default())
        }

        #[no_mangle]
        pub extern "C" fn alloc(size: i32) -> *mut u8 {
            $crate::abi::alloc(size)
        }

        #[no_mangle]
        pub extern "C" fn dealloc(ptr: *mut u8, size: i32) {
            $crate::abi::dealloc(ptr, size)
        }

        #[no_mangle]
        pub extern "C" fn get_metadata() -> u64 {
            use $crate::Source;
            let meta = get_source().metadata();
            let json = $crate::serde_json::to_string(&meta).unwrap_or_default();
            $crate::abi::return_string(json)
        }

        #[no_mangle]
        pub extern "C" fn search(query_ptr: i32, query_len: i32, page: i32) -> u64 {
            use $crate::Source;
            let query = unsafe { $crate::abi::read_string(query_ptr, query_len) };
            match get_source().search(&query, page) {
                Ok(results) => {
                    let json = $crate::serde_json::to_string(&results).unwrap_or_default();
                    $crate::abi::return_string(json)
                }
                Err(e) => {
                    $crate::host::log(4, &format!("Error in search: {}", e));
                    0
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn get_novel_details(url_ptr: i32, url_len: i32) -> u64 {
            use $crate::Source;
            let url = unsafe { $crate::abi::read_string(url_ptr, url_len) };
            match get_source().get_novel_details(&url) {
                Ok(details) => {
                    let json = $crate::serde_json::to_string(&details).unwrap_or_default();
                    $crate::abi::return_string(json)
                }
                Err(e) => {
                    $crate::host::log(4, &format!("Error in get_novel_details: {}", e));
                    0
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn get_chapter_content(url_ptr: i32, url_len: i32) -> u64 {
            use $crate::Source;
            let url = unsafe { $crate::abi::read_string(url_ptr, url_len) };
            match get_source().get_chapter_content(&url) {
                Ok(Some(content)) => $crate::abi::return_string(content),
                Ok(None) => 0,
                Err(e) => {
                    $crate::host::log(4, &format!("Error in get_chapter_content: {}", e));
                    0
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn get_listings() -> u64 {
            use $crate::Source;
            let listings = get_source().get_listings();
            let json = $crate::serde_json::to_string(&listings).unwrap_or_default();
            $crate::abi::return_string(json)
        }

        #[no_mangle]
        pub extern "C" fn get_listing_novels(id_ptr: i32, id_len: i32, page: i32) -> u64 {
            use $crate::Source;
            let id = unsafe { $crate::abi::read_string(id_ptr, id_len) };
            match get_source().get_listing_novels(&id, page) {
                Ok(novels) => {
                    let json = $crate::serde_json::to_string(&novels).unwrap_or_default();
                    $crate::abi::return_string(json)
                }
                Err(e) => {
                    $crate::host::log(4, &format!("Error in get_listing_novels: {}", e));
                    0
                }
            }
        }
    };
}
