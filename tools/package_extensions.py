#!/usr/bin/env python3
"""
Bunori WebAssembly Extension Packager
Converts compiled WebAssembly extension sources into standalone .bext archives and manages repository index.json.

Release Rule:
- A crawler is packaged and released ONLY when its `version` (SemVer x.x.x) is increased compared to
  the index.json published on the LATEST GitHub Pages branch (repo/index.json).
- Unchanged crawlers are completely skipped (zero compilation overhead).
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
import zipfile
from pathlib import Path


def parse_semver(v: str) -> tuple:
    """Parse a version string like '1.2.3' into a comparable tuple of integers (1, 2, 3)."""
    if not v:
        return (0,)
    parts = []
    for part in re.findall(r"\d+", str(v)):
        parts.append(int(part))
    return tuple(parts) if parts else (0,)


def fetch_remote_index(github_repo: str, timeout: int = 15):
    """
    Fetch the currently-published index.json from the GitHub Pages 'repo' branch.
    Returns a dict {id: entry} on success, or None if there's no prior branch yet.
    """
    if not github_repo:
        return None
    url = f"https://raw.githubusercontent.com/{github_repo}/repo/index.json"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "bunori-packager"})
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            entries = data if isinstance(data, list) else data.get("extensions", [])
            index = {e["id"]: e for e in entries}
            print(f"Fetched published baseline index.json from repo branch ({len(index)} extension(s)).")
            return index
    except urllib.error.HTTPError as e:
        if e.code == 404:
            print("No prior published repo branch found (first run) — treating all extensions as new.")
        else:
            print(f"Warning: could not fetch published index.json (HTTP {e.code}). Treating baseline as empty.")
        return None
    except Exception as e:  # noqa: BLE001
        print(f"Warning: could not fetch published index.json ({e}). Treating baseline as empty.")
        return None


def discover_extensions(project_root: Path):
    extensions = []
    sources_dir = project_root / "sources"
    if sources_dir.exists():
        for d in sorted(sources_dir.iterdir()):
            manifest_file = d / "manifest.json"
            cargo_file = d / "Cargo.toml"
            if d.is_dir() and manifest_file.exists() and cargo_file.exists():
                try:
                    with open(manifest_file, "r", encoding="utf-8") as f:
                        manifest = json.load(f)
                        manifest["_source_dir"] = d
                        extensions.append(manifest)
                except Exception as e:  # noqa: BLE001
                    print(f"Warning: Failed to read manifest in {d}: {e}")
    return extensions


def compile_extension(ext: dict, project_root: Path):
    ext_id = ext["id"]
    print(f"  ⚙ Compiling {ext['name']} ({ext_id}) to WebAssembly...")

    cmd = [
        "cargo", "build",
        "--package", ext_id,
        "--target", "wasm32-unknown-unknown",
        "--release"
    ]
    try:
        res = subprocess.run(cmd, cwd=project_root, capture_output=True, text=True, check=True)
    except subprocess.CalledProcessError as e:
        print(e.stderr)
        raise RuntimeError(
            f"Cargo compilation failed for {ext_id}"
        ) from e

    crate_name = ext_id.replace("-", "_")
    wasm_file = project_root / "target" / "wasm32-unknown-unknown" / "release" / f"{crate_name}.wasm"
    if not wasm_file.exists():
        raise RuntimeError(f"Expected wasm file not found at {wasm_file}")
    return wasm_file


def build_bext(ext: dict, wasm_file: Path, output_dir: Path, icons_dir: Path, github_repo: str | None = None):
    ext_id = ext["id"]
    source_dir = ext["_source_dir"]

    # Check for icon
    icon_path = None
    icon_file = None
    for ext_suffix in (".png", ".webp", ".jpg"):
        candidates = [
            source_dir / f"icon{ext_suffix}",
            icons_dir / f"{ext_id}{ext_suffix}"
        ]
        for c in candidates:
            if c.exists():
                icon_file = c
                icon_path = f"assets/icon{ext_suffix}"
                break
        if icon_file:
            break

    manifest = {
        "id": ext_id,
        "name": ext["name"],
        "version": ext["version"],
        "apiVersion": ext.get("apiVersion", 1),
        "lang": ext.get("lang", "en"),
        "baseUrl": ext.get("baseUrl", ""),
        "iconPath": icon_path,
        "iconUrl": ext.get("iconUrl"),
        "webviewNeeded": ext.get("webviewNeeded", False),
        "runnerConcurrency": ext.get("runnerConcurrency", 3),
        "runnerCooldown": ext.get("runnerCooldown", 1000),
        "maxAttempts": ext.get("maxAttempts", 3)
    }

    bext_filename = f"{ext_id}.bext"
    bext_path = output_dir / bext_filename

    with zipfile.ZipFile(bext_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        zf.writestr("manifest.json", json.dumps(manifest, indent=2))
        zf.write(wasm_file, "source.wasm")
        if icon_file and icon_path:
            zf.write(icon_file, icon_path)

    file_bytes = bext_path.read_bytes()
    file_size = len(file_bytes)
    sha256_hash = hashlib.sha256(file_bytes).hexdigest()

    bext_download_url = bext_filename
    icon_url = ext.get("iconUrl")
    if github_repo:
        bext_download_url = f"https://raw.githubusercontent.com/{github_repo}/repo/{bext_filename}"
        if icon_path and not icon_url:
            icon_url = f"https://raw.githubusercontent.com/{github_repo}/repo/{icon_path}"

    print(f"  ✓ Packaged {ext['name']} (v{ext['version']}) -> {bext_filename} ({file_size / 1024:.1f} KB)")

    return {
        "id": ext_id,
        "name": ext["name"],
        "version": ext["version"],
        "apiVersion": ext.get("apiVersion", 1),
        "lang": ext.get("lang", "en"),
        "baseUrl": ext.get("baseUrl", ""),
        "iconPath": icon_path,
        "iconUrl": icon_url,
        "bextUrl": bext_download_url,
        "size": file_size,
        "sha256": sha256_hash,
        "webviewNeeded": ext.get("webviewNeeded", False),
        "runnerConcurrency": ext.get("runnerConcurrency", 3),
        "runnerCooldown": ext.get("runnerCooldown", 1000),
        "maxAttempts": ext.get("maxAttempts", 3),
    }


def main():
    parser = argparse.ArgumentParser(description="Package Bunori WASM extensions into .bext archives and build repository index.")
    parser.add_argument("--out-dir", default="repo", help="Output directory for .bext packages and index.json (default: repo)")
    parser.add_argument("--single", help="ID of single extension to package")
    parser.add_argument("--compile-all", action="store_true", help="Force compilation of all extensions")
    default_repo = os.environ.get("GITHUB_REPOSITORY", "BunoriApp/BunoriExtensions")
    default_tag = os.environ.get("RELEASE_TAG")
    parser.add_argument("--release-tag", default=default_tag, help="Release tag (e.g. v42)")
    parser.add_argument("--github-repo", default=default_repo, help="GitHub repo in owner/name format")
    parser.add_argument("--no-remote-baseline", action="store_true", help="Skip fetching remote baseline index.json")
    args = parser.parse_args()

    project_root = Path(__file__).resolve().parent.parent
    os.chdir(project_root)

    output_dir = project_root / args.out_dir
    output_dir.mkdir(parents=True, exist_ok=True)
    icons_dir = project_root / "icons"

    extensions = discover_extensions(project_root)
    print(f"Found {len(extensions)} extension(s) in source tree.")

    if args.single:
        extensions = [e for e in extensions if e["id"] == args.single]
        if not extensions:
            print(f"Error: No extension found with id '{args.single}'")
            sys.exit(1)

    existing_index = None
    if not args.no_remote_baseline:
        existing_index = fetch_remote_index(args.github_repo)

    if existing_index is None:
        is_ci = bool(os.environ.get("CI") or os.environ.get("GITHUB_ACTIONS"))
        local_index_file = output_dir / "index.json"
        existing_index = {}
        if not is_ci and local_index_file.exists():
            try:
                with open(local_index_file, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    entries = data if isinstance(data, list) else data.get("extensions", [])
                    existing_index = {e["id"]: e for e in entries}
                print(f"Using local repo/index.json as baseline ({len(existing_index)} extension(s)).")
            except Exception:  # noqa: BLE001
                print("Skipping");

    final_entries = {}
    changed_or_new_entries = []

    print("\nChecking extension versions...")
    for ext in extensions:
        ext_id = ext["id"]
        declared_version = ext["version"]
        old_entry = existing_index.get(ext_id)

        should_package = False
        if args.compile_all:
            should_package = True
        elif old_entry is None:
            print(f"  + {ext['name']} (v{declared_version}) - NEW extension")
            should_package = True
        elif parse_semver(declared_version) > parse_semver(old_entry.get("version", "0.0.0")):
            print(f"  ▲ {ext['name']}: v{old_entry.get('version')} -> v{declared_version} (BUMPED)")
            should_package = True
        elif parse_semver(declared_version) < parse_semver(old_entry.get("version", "0.0.0")):
            print(f"  ⚠ {ext['name']}: source declares v{declared_version} but published version is "
                  f"v{old_entry.get('version')} (lower than published — skipping; bump the version to re-release)")
            final_entries[ext_id] = old_entry
        else:
            print(f"  • {ext['name']} (v{declared_version}) - Up-to-date (skipped)")
            final_entries[ext_id] = old_entry

        if should_package:
            wasm_file = compile_extension(ext, project_root)
            entry = build_bext(ext, wasm_file, output_dir, icons_dir, args.github_repo)
            final_entries[ext_id] = entry
            changed_or_new_entries.append(entry)

    all_entries = sorted(final_entries.values(), key=lambda x: x["name"])

    repo_catalog = {
        "repoName": "BunoriExtensions",
        "version": 1,
        "extensions": all_entries
    }

    with open(output_dir / "index.json", "w", encoding="utf-8") as f:
        json.dump(repo_catalog, f, indent=2)

    with open(output_dir / "index.min.json", "w", encoding="utf-8") as f:
        json.dump(all_entries, f, separators=(',', ':'))

    has_release = len(changed_or_new_entries) > 0
    with open(output_dir / "has_release.txt", "w", encoding="utf-8") as f:
        f.write("true" if has_release else "false")

    with open(output_dir / "changed_files.txt", "w", encoding="utf-8") as f:
        if has_release:
            f.write(f"{args.out_dir}/index.json\n")
            f.write(f"{args.out_dir}/index.min.json\n")
            for e in all_entries:
                bext_file = output_dir / f"{e['id']}.bext"
                if bext_file.exists():
                    f.write(f"{args.out_dir}/{e['id']}.bext\n")

    print("\nPackaging summary:")
    print(f"  Total extensions: {len(all_entries)}")
    print(f"  Bumped or new:    {len(changed_or_new_entries)}")
    print(f"  Unchanged:        {len(all_entries) - len(changed_or_new_entries)}")
    print(f"  Release required: {has_release}")


if __name__ == "__main__":
    main()
