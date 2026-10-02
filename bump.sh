#!/bin/bash
set -e

CARGO_TOML="cli/Cargo.toml"
CARGO_LOCK="cli/Cargo.lock"

# ── Read current version ─────────────────────────────────────────────────────
CURRENT=$(grep -E '^version = "' "$CARGO_TOML" | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')

if [ -z "$CURRENT" ]; then
  echo "error: could not read version from $CARGO_TOML" >&2
  exit 1
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

# ── Prompt ───────────────────────────────────────────────────────────────────
echo "Current version: $CURRENT"
echo ""
echo "  [1] major  →  $((MAJOR+1)).0.0"
echo "  [2] minor  →  $MAJOR.$((MINOR+1)).0"
echo "  [3] patch  →  $MAJOR.$MINOR.$((PATCH+1))  (default)"
echo "  [4] keep"
echo ""
printf "Choice [3]: "
read -r CHOICE
CHOICE=${CHOICE:-3}

case "$CHOICE" in
  1) NEW="$((MAJOR+1)).0.0" ;;
  2) NEW="$MAJOR.$((MINOR+1)).0" ;;
  3) NEW="$MAJOR.$MINOR.$((PATCH+1))" ;;
  4) echo "Version unchanged."; exit 0 ;;
  *) echo "error: invalid choice." >&2; exit 1 ;;
esac

echo ""
echo "$CURRENT → $NEW"

# ── Update Cargo.toml ────────────────────────────────────────────────────────
tmp=$(mktemp)
sed "s/^version = \"$CURRENT\"/version = \"$NEW\"/" "$CARGO_TOML" > "$tmp" && mv "$tmp" "$CARGO_TOML"

# ── Update Cargo.lock (yolped package entry only) ────────────────────────────
tmp=$(mktemp)
awk -v old="$CURRENT" -v new="$NEW" '
  /^\[\[package\]\]/  { in_pkg = 0 }
  /^name = "yolped"$/ { in_pkg = 1 }
  in_pkg && /^version = "/ { sub(old, new) }
  { print }
' "$CARGO_LOCK" > "$tmp" && mv "$tmp" "$CARGO_LOCK"

# ── Commit ───────────────────────────────────────────────────────────────────
git add "$CARGO_TOML" "$CARGO_LOCK"
git commit -m "bump: version bump to v$NEW"

echo ""
echo "Done. Push to main to trigger the release:"
echo ""
echo "  git push origin main"
