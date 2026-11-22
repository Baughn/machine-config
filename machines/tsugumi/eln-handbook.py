"""Render the EA handbook (GuideME Markdown from the Stasis repo) as a static site.

Usage: eln-handbook-render SRC_DIR OUT_DIR

Based on LabAssistant's handbook-html.py (Discord 1574067721782431764). A pure
function of its input: no network. Links to *.md become *.html; YAML front
matter gives the page title; images next to the pages are copied; an
index.html listing every page is written when the handbook has no index.md.
Raw HTML in the Markdown passes through: eln.brage.info's CSP forbids scripts.
"""
import html
import os
import re
import shutil
import sys

import markdown
import yaml

ASSETS = (".png", ".jpg", ".jpeg", ".gif", ".webp")

CSS = """
body{max-width:46rem;margin:2rem auto;padding:0 1rem;font:17px/1.55 Georgia,serif;color:#222;background:#fbf8f1}
a{color:#7a3b00} h1,h2,h3{font-family:system-ui,sans-serif;line-height:1.2}
table{border-collapse:collapse;font-size:.92em;margin:1em 0} td,th{border:1px solid #ccb;padding:.3em .5em;vertical-align:top}
th{background:#eee6d4} code,pre{background:#f0ead9;font-size:.9em} pre{padding:.6em;overflow-x:auto}
blockquote{border-left:4px solid #ccb;margin-left:0;padding-left:1em;color:#555}
img{max-width:100%}
nav.top{font-family:system-ui,sans-serif;font-size:.9em;margin-bottom:1.5em} footer{margin-top:3em;font-size:.8em;color:#777}
"""


def split_front(text):
    if text.startswith("---\n"):
        end = text.find("\n---", 4)
        if end > 0:
            front = yaml.safe_load(text[4:end])
            return (front if isinstance(front, dict) else {}), text[end + 4:].lstrip("\n")
    return {}, text


def md_link(m):
    target = m.group(1)
    if not re.match(r"[a-z]+:", target):
        target = re.sub(r"\.md(#|$)", r".html\1", target)
    return f"]({target})"


def page_html(title, home, content, rev):
    footer = "Electrical Age handbook, as shipped in the Stasis Delvers mod. Generated from the repository"
    if rev:
        footer += f" (Stasis {html.escape(rev[:9])})"
    return (f"<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width'>"
            f"<title>{html.escape(str(title))} | EA Handbook</title><style>{CSS}</style>"
            f"<nav class=top><a href='{home}'>EA Handbook</a></nav>{content}"
            f"<footer>{footer}.</footer></html>\n")


def main(src, out, rev):
    pages, assets = [], []
    for root, _, files in os.walk(src):
        for f in files:
            rel = os.path.relpath(os.path.join(root, f), src)
            if f.endswith(".md"):
                pages.append(rel)
            elif f.lower().endswith(ASSETS):
                assets.append(rel)
    index = []
    for rel in sorted(pages):
        with open(os.path.join(src, rel), encoding="utf-8") as fh:
            front, body = split_front(fh.read())
        nav = front.get("navigation") if isinstance(front.get("navigation"), dict) else {}
        title = nav.get("title") or rel[:-3]
        position = nav.get("position") if isinstance(nav.get("position"), (int, float)) else 1e9
        body = re.sub(r"\]\(([^)\s]+)\)", md_link, body)
        content = markdown.markdown(body, extensions=["tables", "fenced_code", "toc", "sane_lists"], output_format="html5")
        home = "../" * rel.count(os.sep) + "index.html"
        dest = os.path.join(out, rel[:-3] + ".html")
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        with open(dest, "w", encoding="utf-8") as fh:
            fh.write(page_html(title, home, content, rev))
        index.append((position, str(title), rel[:-3] + ".html"))
    for rel in assets:
        dest = os.path.join(out, rel)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copyfile(os.path.join(src, rel), dest)
    if "index.md" not in pages:
        items = "".join(f"<li><a href='{html.escape(href)}'>{html.escape(title)}</a></li>"
                        for _, title, href in sorted(index))
        with open(os.path.join(out, "index.html"), "w", encoding="utf-8") as fh:
            fh.write(page_html("Contents", "index.html", f"<h1>The Electrical Engineer's Ledger</h1><ul>{items}</ul>", rev))
    print(f"{len(pages)} pages, {len(assets)} images -> {out}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "")
