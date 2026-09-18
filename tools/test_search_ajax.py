import argparse
import sys
import requests
from bs4 import BeautifulSoup

def test_novelupdates(query: str, cookie: str, user_agent: str):
    session = requests.Session()
    session.headers.update({
        "User-Agent": user_agent,
        "Accept": "*/*",
        "Accept-Language": "en-US,en;q=0.9",
        "Referer": "https://www.novelupdates.com/",
        "Origin": "https://www.novelupdates.com",
    })

    if cookie:
        session.headers["Cookie"] = cookie.strip()

    print(f"\n=======================================================")
    print(f"Testing NovelUpdates Search for: '{query}'")
    print(f"User-Agent: {user_agent}")
    has_cf = "cf_clearance" in cookie if cookie else False
    print(f"Cookies provided: {len(cookie)} chars (has cf_clearance: {has_cf})")
    print(f"=======================================================\n")

    # 1. Test AJAX search endpoint
    print("--- [1. Testing POST admin-ajax.php (nd_ajaxsearchmain)] ---")
    ajax_url = "https://www.novelupdates.com/wp-admin/admin-ajax.php"
    payload = {
        "action": "nd_ajaxsearchmain",
        "strType": "desktop",
        "strOne": query,
        "strSearchType": "series"
    }
    ajax_headers = {
        "Content-Type": "application/x-www-form-urlencoded; charset=UTF-8",
        "X-Requested-With": "XMLHttpRequest"
    }

    try:
        resp = session.post(ajax_url, data=payload, headers=ajax_headers, timeout=15)
        print(f"HTTP Status: {resp.status_code}")
        print(f"Cloudflare Mitigated: {resp.headers.get('cf-mitigated', 'None')}")
        print(f"Server: {resp.headers.get('server')}")
        
        if resp.status_code == 200:
            print("\n[SUCCESS] Received 200 OK from admin-ajax.php!")
            soup = BeautifulSoup(resp.text, "html.parser")
            items = soup.select("li")
            print(f"Parsed {len(items)} <li> result item(s):")
            for i, li in enumerate(items[:10], 1):
                a = li.select_one("a")
                img = li.select_one("img")
                title = a.get_text(strip=True) if a else li.get_text(strip=True)
                url = a["href"] if a and a.has_attr("href") else "No URL"
                cover = img["src"] if img and img.has_attr("src") else "No cover"
                print(f"  {i}. {title}")
                print(f"     URL:   {url}")
                print(f"     Cover: {cover}")
        else:
            print(f"\n[BLOCKED / ERROR] Body preview (first 400 chars):")
            print(resp.text[:400])
    except Exception as e:
        print(f"Network error: {e}")

    # 2. Test standard search endpoint
    print("\n--- [2. Testing GET /?s=...&post_type=seriesposts] ---")
    formatted = query.replace(" ", "+")
    web_url = f"https://www.novelupdates.com/?s={formatted}&post_type=seriesposts"
    try:
        resp = session.get(web_url, timeout=15)
        print(f"HTTP Status: {resp.status_code}")
        print(f"Cloudflare Mitigated: {resp.headers.get('cf-mitigated', 'None')}")
        if resp.status_code == 200:
            print("\n[SUCCESS] Received 200 OK from web search page!")
            soup = BeautifulSoup(resp.text, "html.parser")
            items = soup.select("div.search_main_box_nu, div.w-blog-entry, .search_body_nu")
            print(f"Found {len(items)} search card(s).")
            for i, card in enumerate(items[:5], 1):
                a = card.select_one(".search_title > a, .w-blog-entry-title a, h2 a")
                if a:
                    print(f"  {i}. {a.get_text(strip=True)} -> {a.get('href')}")
        else:
            print(f"\n[BLOCKED / ERROR] Body preview (first 400 chars):")
            print(resp.text[:400])
    except Exception as e:
        print(f"Network error: {e}")

def main():
    parser = argparse.ArgumentParser(description="Test NovelUpdates search directly with cookies & user agent")
    parser.add_argument("--query", "-q", default="heya", help="Search query (default: heya)")
    parser.add_argument("--cookie", "-c", default="", help="Cookie header string")
    parser.add_argument("--cookie-file", "-f", help="Path to file containing cookies")
    parser.add_argument("--user-agent", "-u", default="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36", help="User-Agent string")
    args = parser.parse_args()

    cookie = args.cookie
    if args.cookie_file:
        with open(args.cookie_file, "r") as f:
            cookie = f.read().strip()

    if not cookie:
        print("Tip: You can pass cookies using --cookie '...' or --cookie-file cookies.txt")
        print("Reading cookie from input (or press Enter to run without cookies):")
        try:
            line = input().strip()
            if line:
                cookie = line
        except EOFError:
            pass

    test_novelupdates(args.query, cookie, args.user_agent)

if __name__ == "__main__":
    main()
