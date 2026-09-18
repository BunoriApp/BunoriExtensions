import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

from wasmtime import Engine, Func, FuncType, Linker, Memory, Module, Store, ValType


class BunoriHostRunner:
    def __init__(self, wasm_path: Path, cookie: str = "", user_agent: str = ""):
        self.wasm_path = wasm_path
        self.cookie = cookie.strip()
        self.user_agent = user_agent.strip()
        self.engine = Engine()
        self.store = Store(self.engine)
        self.linker = Linker(self.engine)

        self._register_host_functions()

        if not wasm_path.exists():
            raise FileNotFoundError(f"WASM file not found at: {wasm_path}")
        
        self.module = Module.from_file(self.engine, str(wasm_path))
        self.instance = self.linker.instantiate(self.store, self.module)

        raw_memory = self.instance.exports(self.store)["memory"]
        assert isinstance(raw_memory, Memory), "Expected 'memory' export to be a Memory instance"
        self.memory = raw_memory
    
    def _get_func(self, name: str) -> Func:
        fn = self.instance.exports(self.store)[name]
        assert isinstance(fn, Func), f"Export '{name}' is not a Func"
        return fn

    def _register_host_functions(self):
        def host_time_ms(caller):
            return int(time.time() * 1000)
        
        def host_log(caller, level: int, ptr: int, length: int):
            mem = caller.get("memory")
            raw = mem.read(caller, ptr, ptr+length)
            msg = raw.decode("utf-8", errors="replace")
            levels = {1: "DEBUG", 2: "INFO", 3: "WARN", 4: "ERROR"}
            tag = levels.get(level, f"LOG_{level}")
            print(f" [{tag}] {msg}")

        def host_http(caller, req_ptr: int, req_len: int):
            mem = caller.get("memory")
            req_raw = mem.read(caller, req_ptr, req_ptr + req_len)
            req_data = json.loads(req_raw.decode("utf-8"))

            url = req_data["url"]
            method = req_data.get("method", "GET")
            headers = req_data.get("headers", {})
            body = req_data.get("body")

            if self.user_agent:
                headers["User-Agent"] = self.user_agent
            elif "User-Agent" not in headers and "user-agent" not in headers:
                headers["User-Agent"] = (
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36"
                )

            if self.cookie and "Cookie" not in headers and "cookie" not in headers:
                headers["Cookie"] = self.cookie

            data_bytes = body.encode("utf-8") if body else None
            http_req = urllib.request.Request(url, data=data_bytes, headers=headers, method=method)

            try:
                with urllib.request.urlopen(http_req, timeout=15) as resp:
                    resp_body = resp.read().decode("utf-8", errors="replace")
                    status_code = resp.status
                    resp_headers = dict(resp.headers)
            except urllib.error.HTTPError as e:
                status_code = e.code
                resp_body = e.read().decode("utf-8", errors="replace")
                resp_headers = dict(e.headers)
            except Exception as e:  # noqa: BLE001
                status_code = 500
                resp_body = f"Network error: {e}"
                resp_headers = {}

            res_payload = json.dumps({
                 "status_code": status_code,
                 "headers": resp_headers,
                 "body": resp_body
            }).encode("utf-8")

            alloc_func = caller.get("alloc")
            assert isinstance(alloc_func, Func), "Export 'alloc' not found on caller"
            res_ptr = alloc_func(caller, len(res_payload))
            assert isinstance(res_ptr, int)
            mem.write(caller, res_payload, res_ptr)

            return res_ptr | (len(res_payload) << 32)

        self.linker.define_func("bunori", "host_time_ms", FuncType([], [ValType.i64()]), host_time_ms, access_caller=True)
        self.linker.define_func("bunori", "host_log", FuncType([ValType.i32(), ValType.i32(), ValType.i32()], []), host_log, access_caller=True)
        self.linker.define_func("bunori", "host_http", FuncType([ValType.i32(), ValType.i32()], [ValType.i64()]), host_http, access_caller=True)
    

    def _write_string(self, text: str) -> tuple[int, int]:
        alloc_fun = self._get_func("alloc")
        payload = text.encode("utf-8")
        ptr = alloc_fun(self.store, len(payload))
        assert isinstance(ptr, int)
        self.memory.write(self.store, payload, ptr)
        return ptr, len(payload)

    def _read_and_free_string(self, packed_u64: int) -> str:
        if packed_u64 == 0:
            return ""
        ptr = packed_u64 & 0xFFFFFFFF
        length = (packed_u64 >> 32) & 0xFFFFFFFF
        raw = self.memory.read(self.store, ptr, ptr + length)
        result = raw.decode("utf-8", errors="replace")

        dealloc_item = self.instance.exports(self.store).get("dealloc")
        if isinstance(dealloc_item, Func):
            dealloc_item(self.store, ptr, length)

        return result

    def get_metadata(self) -> dict:
        fn = self._get_func("get_metadata")
        packed = fn(self.store)
        assert isinstance(packed, int)
        return json.loads(self._read_and_free_string(packed))
    
    def search(self, query: str, page: int = 1) -> list:
        q_ptr, q_len = self._write_string(query)
        fn = self._get_func("search")
        packed = fn(self.store, q_ptr, q_len, page)
        assert isinstance(packed, int)
        if packed == 0:
            raise RuntimeError(f"Search for '{query}' failed in extension (returned null/0). See [ERROR] logs above.")
        return json.loads(self._read_and_free_string(packed))

    def get_novel_details(self, url: str) -> dict:
        u_ptr, u_len = self._write_string(url)
        fn = self._get_func("get_novel_details")
        packed = fn(self.store, u_ptr, u_len)
        assert isinstance(packed, int)
        if packed == 0:
            raise RuntimeError(f"get_novel_details for '{url}' failed in extension (returned null/0). See [ERROR] logs above.")
        return json.loads(self._read_and_free_string(packed))

    def get_chapter_content(self, url: str) -> str:
        u_ptr, u_len = self._write_string(url)
        fn = self._get_func("get_chapter_content")
        packed = fn(self.store, u_ptr, u_len)
        assert isinstance(packed, int)
        if packed == 0:
            raise RuntimeError(f"get_chapter_content for '{url}' failed in extension (returned null/0). See [ERROR] logs above.")
        return self._read_and_free_string(packed)
    
    def get_listings(self) -> list:
        fn = self._get_func("get_listings")
        packed = fn(self.store)
        assert isinstance(packed, int)
        if packed == 0:
            raise RuntimeError("get_listings failed in extension (returned null/0). See [ERROR] logs above.")
        return json.loads(self._read_and_free_string(packed))

    def get_listing_novels(self, listing_id: str, page: int = 1) -> list:
        id_ptr, id_len = self._write_string(listing_id)
        fn = self._get_func("get_listing_novels")
        packed = fn(self.store, id_ptr, id_len, page)
        assert isinstance(packed, int)
        if packed == 0:
            raise RuntimeError(f"get_listing_novels for '{listing_id}' failed in extension (returned null/0). See [ERROR] logs above.")
        return json.loads(self._read_and_free_string(packed))

def main():
    parser = argparse.ArgumentParser(description="Test Bunori WASM extensions directly in python.")
    parser.add_argument("extension_id", help="Extension crate ID (for instance asianovel, novelarchive)")
    parser.add_argument("--metadata", action="store_true", help="Fetch extension metadata")
    parser.add_argument("--search", help="Test search query")
    parser.add_argument("--page", type=int, default=1, help="Page number (default: 1)")
    parser.add_argument("--details", help="Test fetching novel details by URL")
    parser.add_argument("--chapter", help="Test fetching chapter content by URL")
    parser.add_argument("--listings", action="store_true", help="Fetch supported ranking listings")
    parser.add_argument("--listing-novels", help="Fetch novels for a listing ID")
    parser.add_argument("--wasm-path", help="Explicit path to .wasm binary")
    parser.add_argument("--cookie", "-c", default="", help="Cookie header string to pass to HTTP requests")
    parser.add_argument("--cookie-file", "-f", help="Path to file containing cookies")
    parser.add_argument("--user-agent", "-u", default="", help="Custom User-Agent string")
    args = parser.parse_args()

    cookie = args.cookie
    if args.cookie_file:
        with open(args.cookie_file, "r") as f:
            cookie = f.read().strip()

    project_root = Path(__file__).resolve().parent.parent
    crate_name = args.extension_id.replace("-", "_")
    wasm_path = Path(args.wasm_path) if args.wasm_path else (
        project_root / "target" / "wasm32-unknown-unknown" / "release" / f"{crate_name}.wasm"
    )

    if not wasm_path.exists():
        print(f"Error: WASM file not found {wasm_path}")
        print(f"Please bulid it first with: cargo build -p {args.extension_id} --target wasm32-unknown-unknown --release")
        sys.exit(1)

    print(f"Loading WASM module: {wasm_path.name}")
    runner = BunoriHostRunner(wasm_path, cookie=cookie, user_agent=args.user_agent)

    has_action = any([args.metadata, args.search, args.details, args.chapter, args.listings, args.listing_novels])

    if args.metadata or not has_action:
        print("\n--- [Metadata] ---")
        meta = runner.get_metadata()
        print(json.dumps(meta, indent=2))

    if args.search:
        print(f"\n--- [Search: '{args.search}' (page {args.page})] ---")
        results = runner.search(args.search, args.page)
        print(f"Found {len(results)} result(s):")
        print(json.dumps(results[:5], indent=2))
        if len(results) > 5:
            print(f"... and {len(results) - 5} more results.")

    if args.details:
        print(f"\n--- [Novel Details: '{args.details}'] ---")
        details = runner.get_novel_details(args.details)
        chapters = details.get("chapters", [])
        print(f"Title: {details.get('title')}")
        print(f"Author: {details.get('author')}")
        print(f"Genres: {', '.join(details.get('genres', []))}")
        print(f"Total Chapters: {len(chapters)}")
        if chapters:
            print(f"  First: {chapters[0].get('title')} -> {chapters[0].get('url')}")
            print(f"  Last:  {chapters[-1].get('title')} -> {chapters[-1].get('url')}")

    if args.chapter:
        print(f"\n--- [Chapter Content: '{args.chapter}'] ---")
        content = runner.get_chapter_content(args.chapter)
        print(f"Length: {len(content)} characters")
        print(f"Preview (first 400 chars):\n{content[:400]}...")

    if args.listings:
        print("\n--- [Supported Listings] ---")
        listings = runner.get_listings()
        print(json.dumps(listings, indent=2))

    if args.listing_novels:
        print(f"\n--- [Listing Novels: '{args.listing_novels}' (page {args.page})] ---")
        novels = runner.get_listing_novels(args.listing_novels, args.page)
        print(f"Found {len(novels)} novel(s):")
        print(json.dumps(novels[:5], indent=2))

if __name__ == "__main__":
    main()
