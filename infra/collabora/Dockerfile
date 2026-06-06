# File Hub — branded Collabora Online (Option A).
#
# Approach: patch the compiled `bundle.js` so the Help-tab content list
# returns empty.  This is more surgical than CSS overrides because the
# items are NEVER rendered into the DOM in the first place — no
# whack-a-mole with browser cache, no chasing per-release id changes,
# no broken keyboard navigation through hidden-but-still-focusable
# elements.
#
# The patch wraps `getHelpTab:function(){...return this.getTabPage(...)}`
# with an early `return this.getTabPage(helpTabName, [])` so Collabora
# still HAS a Help tab (keeps muscle memory + keyboard shortcut intact)
# but the content area shows nothing.  Every menu-item that referenced
# Collabora's bug tracker / forum / About is gone in a single edit.
#
# Build:
#   docker compose build collabora
#   docker compose up -d --force-recreate collabora
FROM collabora/code:latest

USER root

# The patch — Collabora has TWO Help code paths:
#
#   1. `getHelpTab()` returns the notebook-bar `bigtoolitem` array.  We
#      neutralise the result by passing an empty array to `getTabPage`.
#   2. `HelpMenu` is the menu-bar definition (`id:"help", type:"menu",
#      menu:[ ... ]`) shipped per-app (text / spreadsheet /
#      presentation).  This is what the rendered tab actually shows —
#      patching just (1) does nothing visible.
#
# bundle.js is "minified" with hard wraps every ~80 chars, so the
# patterns we rewrite span newlines.  GNU sed's `-z` flag treats the
# whole file as one null-terminated record, which lets a single
# regex span line breaks.
RUN sed -i -z \
    -e 's/getTabPage(helpTabName,\s*content)/getTabPage(helpTabName,[])/g' \
    -e 's/id:"help",type:"menu",menu:\[[^]]*\]/id:"help",type:"menu",menu:[]/g' \
    /usr/share/coolwsd/browser/dist/bundle.js

# Sanity checks — every release nudges these patterns, so we want a
# loud build failure rather than a silent regression.  We assert:
#   - The notebookbar call now uses []
#   - The HelpMenu array is now empty
#   - The unpatched strings are GONE so we know we hit every site
RUN set -e; \
    grep -q 'getTabPage(helpTabName,\[\])' /usr/share/coolwsd/browser/dist/bundle.js; \
    grep -q 'id:"help",type:"menu",menu:\[\]' /usr/share/coolwsd/browser/dist/bundle.js; \
    ! grep -q 'getTabPage(helpTabName,content)' /usr/share/coolwsd/browser/dist/bundle.js; \
    ! grep -q 'id:"help",type:"menu",menu:\[{' /usr/share/coolwsd/browser/dist/bundle.js; \
    echo "✓ Help menu wiped from both getHelpTab + HelpMenu code paths"

# Keep the branding.css mount mechanism alive for cosmetic tweaks
# (logo, accent colors) that still belong outside the source patch.
COPY branding.css /usr/share/coolwsd/browser/dist/branding.css

USER cool
