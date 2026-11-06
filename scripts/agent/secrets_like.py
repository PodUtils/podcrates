"""Emulate two TruffleHog detectors (URI, CloudflareApiToken) over files: print FILE:LINE DETECTOR."""
import re, sys
CF = re.compile(r'(?i:cloudflare)(?:.|[\n\r]){0,40}?\b([A-Za-z0-9_-]{40})\b')
URI = re.compile(r'\b(?:https?:)?//[\S]{3,50}:([\S]{3,50})@[-.%\w/:]+\b')
for path in sys.argv[1:]:
    text = open(path, encoding='utf-8', errors='replace').read()
    lines = text.split('\n')
    def line_of(raw):
        for i, l in enumerate(lines, 1):
            if raw in l:
                return i
        return 0
    seen = set()
    for m in CF.finditer(text):
        raw = m.group(1)
        if raw in seen: continue
        seen.add(raw)
        print(f"CloudflareApiToken {path}:{line_of(raw)} {lines[line_of(raw)-1][:100]}")
    for m in URI.finditer(text):
        raw = m.group(0)
        if raw in seen: continue
        seen.add(raw)
        print(f"URI {path}:{line_of(raw)} {lines[line_of(raw)-1][:100]}")
